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

    use gpio_cdev::{
        Chip, EventRequestFlags, EventType, Line, LineEvent, LineEventHandle, LineRequestFlags,
    };
    use serde::Deserialize;
    use slog_scope::{debug, error, info};

    use super::*;

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

    struct BufferedLineEventHandle {
        buffered: Vec<LineEvent>,
        line_event_handle: LineEventHandle,
    }

    impl BufferedLineEventHandle {
        pub fn unread(&mut self, event: LineEvent) {
            self.buffered.push(event);
        }
        pub fn next(&mut self) -> Fallible<LineEvent> {
            // if let Some(e) = self.buffered.first_mut() {
            //     return Ok(*e);
            // }
            if !self.buffered.is_empty() {
                return Ok(self.buffered.remove(0));
            }
            return Ok(self.line_event_handle.get_event()?);
        }
        pub fn new(leh: LineEventHandle) -> Self {
            return Self {
                buffered: Vec::new(),
                line_event_handle: leh,
            };
        }
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

    impl<T: Clone + Send + 'static> CdevGpio<T> {
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
            // let mut n_received_during_shutdown_delay = 0;
            let mut ts = std::time::Instant::now();
            let epsilon = Duration::from_millis(500);
            const PRESS_EVENT_TYPE: EventType = EventType::FallingEdge;
            const RELEASE_EVENT_TYPE: EventType = EventType::RisingEdge;

            info!("Listening for GPIO events on line {}", line_id);

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

            let mut pressed: bool = false;

            loop {
                // before we block on reading events, we compare the last state (realized by emitting events) with the current state of the line
                // to check if the inconsistency requires us to emit another event.
                // this case occurs when the button is pressed very shortly, resulting in press- and release-event being emitted quickly after
                // one another.
                let current = match line_event_handle.get_value() {
                    Err(err) => {
                        error!("Failed to get current value of line {}: {}", line_id, err);
                        continue;
                    }
                    Ok(c) => c == 0,
                };

                if !current {
                    if pressed {
                        // Inconsistency: Last state is pressed and the current value of the line is inactive.
                        // Emit release event!
                        debug!("Emitting release event on line {}", line_id);
                        if let Some(ev) = msg_transformer(release_ev) {
                            if let Err(err) = self.tx.send(ev) {
                                error!(
                                    "Failed to transmit GPIO event ... derived from {:?}: {}",
                                    release_ev, err
                                );
                                continue;
                            }
                            pressed = false;
                        }
                    }
                } else {
                    if !pressed {
                        // Inconsistency: Last state is released and the current value of the line is active.
                        // Emit press event.
                        debug!("Emitting press event on line {}", line_id);
                        if let Some(ev) = msg_transformer(press_ev) {
                            if let Err(err) = self.tx.send(ev) {
                                error!(
                                    "Failed to transmit GPIO event ... derived from {:?}: {}",
                                    press_ev, err
                                );
                                continue;
                            }
                            pressed = true;
                        }
                    }
                }

                // Any inconsistencies resolved, retrieve next event in a blocking way.
                // Ok to block here since the current state of the line reflects what has last been emitted as an event.
                let event = match line_event_handle.get_event() {
                    Err(err) => {
                        error!("Ignoring erronous event on line {}: {}", line_id, err);
                        continue;
                    }
                    Ok(ev) => ev,
                };

                if ts.elapsed() < epsilon {
                    // We have a timestamp already. This denotes the fact that we have received a "primary" event (at that timestamp) recently.
                    //
                    // another event occured very shortly after the last timestamp, this could be a noise event.
                    // we do not emit any events for this
                    debug!("Ignoring GPIO event {:?} on line {}", event, line_id);
                    continue;
                } else {
                    debug!("Received GPIO event {:?} on line {}", event, line_id);

                    // We are outside of an epsilon-timeframe around some "primary" event.
                    // Regard the new event as being a new primary event.
                    // Emit event for it.
                    ts = std::time::Instant::now();

                    let event_type = event.event_type();
                    if event_type == PRESS_EVENT_TYPE {
                        if let Some(ev) = msg_transformer(press_ev) {
                            if let Err(err) = self.tx.send(ev) {
                                error!(
                                    "Failed to transmit GPIO event ... derived from {:?}: {}",
                                    press_ev, err
                                );
                            }
                            pressed = true;
                        }
                    } else if event_type == RELEASE_EVENT_TYPE {
                        if let Some(ev) = msg_transformer(release_ev) {
                            if let Err(err) = self.tx.send(ev) {
                                error!(
                                    "Failed to transmit GPIO event ... derived from {:?}: {}",
                                    release_ev, err
                                );
                            }
                            pressed = false;
                        }
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
