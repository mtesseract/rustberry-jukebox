use std::time::{Duration, Instant};

use crossbeam_channel::{self, Receiver, Sender};
use failure::Fallible;

use crate::player::PlaybackRequest;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    Shutdown,
    VolumeUp,
    VolumeDown,
    PauseContinue,
    LockPlayer,
    UnlockPlayer,
    Playback(PlaybackRequest),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ButtonEvent {
    ShutdownPress,
    ShutdownRelease,
    VolumeUpPress,
    VolumeUpRelease,
    VolumeDownPress,
    VolumeDownRelease,
    PauseContinuePress,
    PauseContinueRelease,
}

#[derive(Debug, Clone)]
pub struct Config {
    pub shutdown_pin: Option<u32>,
    pub volume_up_pin: Option<u32>,
    pub volume_down_pin: Option<u32>,
    pub pause_pin: Option<u32>,
    pub start_time: Option<Instant>,
}

pub struct Handle<T> {
    channel: Receiver<T>,
}

impl<T> Handle<T> {
    pub fn channel(&self) -> Receiver<T> {
        self.channel.clone()
    }
}

pub mod cdev_gpio {
    use std::collections::HashMap;
    use std::convert::From;
    use std::sync::{Arc, RwLock};

    use gpio_cdev::{Chip, EventRequestFlags, EventType, Line, LineEvent, LineRequestFlags};
    use serde::Deserialize;
    use slog_scope::{debug, error, info};

    use super::*;

    struct EventEmitter<T> {
        last: Option<EventType>,
        msg_transformer: Arc<dyn Fn(ButtonEvent) -> Option<T> + 'static + Send>,
        tx: Sender<T>,
        ev_pressed: ButtonEvent,
        ev_released: ButtonEvent,
    }

    impl<T: std::fmt::Debug> EventEmitter<T> {
        pub fn new<F>(
            msg_transformer: Arc<F>,
            ev_pressed: ButtonEvent,
            ev_released: ButtonEvent,
            tx: Sender<T>,
        ) -> Self
        where
            F: Fn(ButtonEvent) -> Option<T> + 'static + Send,
        {
            Self {
                last: None,
                msg_transformer,
                tx,
                ev_pressed,
                ev_released,
            }
        }

        pub fn emit(&mut self, event: EventType) -> Fallible<()> {
            match (event, self.last) {
                (a, Some(b)) if a == b => {
                    debug!("EventEmitter: ignoring duplicate event {:?}", a);
                    Ok(())
                }
                (a, _) => {
                    self.last = Some(a);
                    let aa = if a == EventType::FallingEdge {
                        self.ev_pressed
                    } else {
                        self.ev_released
                    };
                    let t = (self.msg_transformer)(aa);
                    if t.is_none() {
                        debug!("EventEmitter: message transformer skips event {:?}", aa);
                        return Ok(());
                    }
                    let t = t.unwrap();
                    debug!("EventEmitter: emitting event {:?}", t);
                    self.tx.send(t).unwrap();
                    Ok(())
                }
            }
        }
    }

    struct EventReader {
        events: Arc<RwLock<Vec<LineEvent>>>,
        notif: Arc<RwLock<Option<Sender<()>>>>,
        last: Arc<RwLock<Option<EventType>>>,
    }

    impl EventReader {
        pub fn new(line: Line, line_id: u32) -> Fallible<Self> {
            let events = Arc::new(RwLock::new(Vec::new()));
            let events_cp = Arc::clone(&events);
            let notif: Arc<RwLock<Option<Sender<()>>>> = Arc::new(RwLock::new(None));
            let notif_cp = Arc::clone(&notif);
            let last = Arc::new(RwLock::new(None));
            let last_cp = Arc::clone(&last);
            let mut line_event_handle = line
                .events(
                    LineRequestFlags::INPUT,
                    EventRequestFlags::BOTH_EDGES,
                    "read-input",
                )
                .map_err(|err| {
                    Error::IO(format!(
                        "Failed to request events from GPIO line {}: {}",
                        line_id, err
                    ))
                })?;

            let f = move || loop {
                let event = match line_event_handle.get_event() {
                    Err(err) => {
                        error!("Ignoring erronous event on line {}: {}", line_id, err);
                        continue;
                    }
                    Ok(ev) => ev,
                };
                debug!(
                    "EventReader: received event {:?} on line {}",
                    event.event_type(),
                    line_id,
                );
                let et = event.event_type();
                {
                    let mut w_events = events_cp.write().unwrap();
                    w_events.push(event);
                }
                {
                    let mut w_last = last_cp.write().unwrap();
                    *w_last = Some(et);
                }
                {
                    // Try to send notification.
                    let mut w_notif = notif_cp.write().unwrap();
                    if let Some(tx) = &*w_notif {
                        let _ = tx.send(()); // Fine to fail due to channel not being connected anymore.
                        *w_notif = None;
                    }
                }
            };
            std::thread::spawn(f);
            Ok(Self {
                events,
                notif,
                last,
            })
        }

        pub fn next(&self) -> Fallible<EventType> {
            let (tx, rx) = crossbeam_channel::bounded(1);
            {
                // Register channel for being notified about new events.
                let mut w_notif = self.notif.write().unwrap();
                *w_notif = Some(tx);
            }
            let is_empty = {
                let r_events = self.events.read().unwrap();
                r_events.is_empty()
            };

            if is_empty {
                let _ = rx.recv().unwrap();
            }

            {
                let mut w_events = self.events.write().unwrap();
                let ev = w_events.remove(0);
                Ok(ev.event_type())
            }
        }

        pub fn last(&mut self) -> Option<EventType> {
            let mut w_last = self.last.write().unwrap();
            w_last.take()
        }

        pub fn skip_past(&self) {
            let mut w_events = self.events.write().unwrap();
            w_events.clear();
        }
    }

    #[derive(Debug, Clone)]
    pub struct CdevGpio<T: Clone> {
        map: HashMap<u32, (ButtonEvent, ButtonEvent)>,
        chip: Arc<RwLock<Chip>>,
        config: Config,
        tx: Sender<T>,
    }

    #[derive(Deserialize, Debug, Clone)]
    pub struct EnvConfig {
        shutdown_pin: Option<u32>,
        volume_up_pin: Option<u32>,
        volume_down_pin: Option<u32>,
        pause_pin: Option<u32>,
    }

    impl From<EnvConfig> for Config {
        fn from(env_config: EnvConfig) -> Self {
            let start_time = Some(Instant::now());
            Config {
                shutdown_pin: env_config.shutdown_pin,
                volume_up_pin: env_config.volume_up_pin,
                volume_down_pin: env_config.volume_down_pin,
                pause_pin: env_config.pause_pin,
                start_time,
            }
        }
    }

    impl EnvConfig {
        pub fn new_from_env() -> Fallible<Self> {
            Ok(envy::from_env::<EnvConfig>()?)
        }
    }

    impl<T: std::fmt::Debug + Clone + Send + 'static> CdevGpio<T> {
        pub fn new_from_env<F>(msg_transformer: F) -> Fallible<Handle<T>>
        where
            F: Fn(ButtonEvent) -> Option<T> + 'static + Send + Sync,
        {
            info!("Using CdevGpio based in Button Controller");
            let env_config = EnvConfig::new_from_env()?;
            let config: Config = env_config.into();
            let mut map = HashMap::new();
            if let Some(shutdown_pin) = config.shutdown_pin {
                map.insert(
                    shutdown_pin,
                    (ButtonEvent::ShutdownPress, ButtonEvent::ShutdownRelease),
                );
            }
            if let Some(pin) = config.volume_up_pin {
                map.insert(
                    pin,
                    (ButtonEvent::VolumeUpPress, ButtonEvent::VolumeUpRelease),
                );
            }
            if let Some(pin) = config.volume_down_pin {
                map.insert(
                    pin,
                    (ButtonEvent::VolumeDownPress, ButtonEvent::VolumeDownRelease),
                );
            }
            if let Some(pin) = config.pause_pin {
                map.insert(
                    pin,
                    (
                        ButtonEvent::PauseContinuePress,
                        ButtonEvent::PauseContinueRelease,
                    ),
                );
            }
            let chip = Chip::new("/dev/gpiochip0")
                .map_err(|err| Error::IO(format!("Failed to open Chip: {:?}", err)))?;
            let (tx, rx) = crossbeam_channel::bounded(1);
            let mut gpio_cdev = Self {
                map,
                chip: Arc::new(RwLock::new(chip)),
                config,
                tx,
            };

            gpio_cdev.run(msg_transformer)?;
            Ok(Handle { channel: rx })
        }

        fn run_single_event_listener<F>(
            self,
            (line, line_id, (press_ev, release_ev)): (Line, u32, (ButtonEvent, ButtonEvent)),
            msg_transformer: Arc<F>,
        ) -> Fallible<()>
        where
            F: Fn(ButtonEvent) -> Option<T> + 'static + Send,
        {
            let mut er = EventReader::new(line, line_id)?;
            let mut ee = EventEmitter::new(msg_transformer, press_ev, release_ev, self.tx.clone());
            let mut skip = false;
            let epsilon = Duration::from_millis(100);

            loop {
                let ev = if skip {
                    debug!("run_single_event_listener: skip=true");
                    skip = false;
                    std::thread::sleep(epsilon);
                    er.skip_past();
                    er.last().unwrap()
                } else {
                    debug!("run_single_event_listener: skip=false");
                    skip = true;
                    er.next().unwrap()
                };
                debug!("Event to emit: {:?}", ev);
                if let Err(err) = ee.emit(ev) {
                    error!("Failed to emit event {:?}: {}", ev, err);
                }
            }

            // TODO, reenable this logic:
            //
            // if press_ev == ButtonEvent::ShutdownPress {
            //     if let Some(start_time) = self.config.start_time {
            //         let now = Instant::now();
            //         let dt: Duration = now - start_time;
            //         if dt < DELAY_BEFORE_ACCEPTING_SHUTDOWN_COMMANDS {
            //             warn!(
            //                 "Ignoring shutdown event (time elapsed since start: {:?})",
            //                 dt
            //             );
            //             n_received_during_shutdown_delay += 1;
            //             continue;
            //         }
            //     }

            //     if n_received_during_shutdown_delay > 10 {
            //         warn!("Received too many shutdown events right after startup, shutdown functionality has been disabled");
            //         continue;
            //     }
            // }
        }

        fn run<F>(&mut self, msg_transformer: F) -> Fallible<()>
        where
            F: Fn(ButtonEvent) -> Option<T> + 'static + Send + Sync,
        {
            let chip = &mut *(self.chip.write().unwrap());
            let msg_transformer = Arc::new(msg_transformer);
            // Spawn threads for requested GPIO lines.
            for (line_id, (press_ev, release_ev)) in self.map.iter() {
                info!(
                    "Listening for button events {:?}/{:?} on GPIO line {}",
                    press_ev, release_ev, line_id
                );
                let line_id = *line_id as u32;
                let line = chip
                    .get_line(line_id)
                    .map_err(|err| Error::IO(format!("Failed to get GPIO line: {:?}", err)))?;
                let press_ev = (*press_ev).clone();
                let release_ev = (*release_ev).clone();
                let clone = self.clone();
                let msg_transformer = Arc::clone(&msg_transformer);
                let _handle = std::thread::Builder::new()
                    .name(format!("button-controller-{}", line_id))
                    .spawn(move || {
                        let res = clone.run_single_event_listener(
                            (line, line_id, (press_ev, release_ev)),
                            msg_transformer,
                        );
                        error!("GPIO Listener loop terminated unexpectedly: {:?}", res);
                    })
                    .unwrap();
            }
            Ok(())
        }
    }
}

#[derive(Debug)]
pub enum Error {
    IO(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::IO(s) => write!(f, "IO Error: {}", s),
        }
    }
}

impl std::error::Error for Error {}

// const DELAY_BEFORE_ACCEPTING_SHUTDOWN_COMMANDS: Duration = Duration::from_secs(10);
