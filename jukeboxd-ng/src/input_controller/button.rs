use crossbeam_channel::{self, Receiver, Sender};
use failure::Fallible;
use slog_scope::{error, info};
use std::time::{Duration, Instant};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    Shutdown,
    VolumeUp,
    VolumeDown,
}

#[derive(Debug, Clone)]
pub struct Config {
    pub shutdown_pin: Option<u32>,
    pub volume_up_pin: Option<u32>,
    pub volume_down_pin: Option<u32>,
    pub start_time: Option<Instant>,
}

pub struct Handle<T> {
    channel: Receiver<T>,
    // thread: Arc<JoinHandle<()>>,
}

impl<T> Handle<T> {
    pub fn channel(&self) -> Receiver<T> {
        self.channel.clone()
    }
}

pub mod cdev_gpio {
    use super::*;
    use gpio_cdev::{Chip, EventRequestFlags, Line, LineRequestFlags};
    use serde::Deserialize;
    use slog_scope::{error, info, warn};
    use std::collections::HashMap;
    use std::convert::From;
    use std::sync::{Arc, RwLock};

    #[derive(Debug, Clone)]
    pub struct CdevGpio<T: Clone> {
        map: HashMap<u32, Command>,
        chip: Arc<RwLock<Chip>>,
        config: Config,
        tx: Sender<T>,
    }

    #[derive(Deserialize, Debug, Clone)]
    pub struct EnvConfig {
        shutdown_pin: Option<u32>,
        volume_up_pin: Option<u32>,
        volume_down_pin: Option<u32>,
    }

    impl From<EnvConfig> for Config {
        fn from(env_config: EnvConfig) -> Self {
            let start_time = Some(Instant::now());
            Config {
                shutdown_pin: env_config.shutdown_pin,
                volume_up_pin: env_config.volume_up_pin,
                volume_down_pin: env_config.volume_down_pin,
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
            F: Fn(Command) -> Option<T> + 'static + Send + Sync,
        {
            info!("Using CdevGpio based in Button Controller");
            let env_config = EnvConfig::new_from_env()?;
            let config: Config = env_config.into();
            let mut map = HashMap::new();
            if let Some(shutdown_pin) = config.shutdown_pin {
                map.insert(shutdown_pin, Command::Shutdown);
            }
            if let Some(pin) = config.volume_up_pin {
                map.insert(pin, Command::VolumeUp);
            }
            if let Some(pin) = config.volume_down_pin {
                map.insert(pin, Command::VolumeDown);
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
            mut self,
            (line, line_id, cmd): (Line, u32, Command),
            msg_transformer: Arc<F>,
        ) -> Fallible<()>
        where
            F: Fn(Command) -> Option<T> + 'static + Send,
        {
            let mut n_received_during_shutdown_delay = 0;
            info!("Listening for GPIO events on line {}", line_id);
            for event in line
                .events(
                    LineRequestFlags::INPUT,
                    EventRequestFlags::FALLING_EDGE,
                    "read-input",
                )
                .map_err(|err| {
                    Error::IO(format!(
                        "Failed to request events from GPIO line {}: {}",
                        line_id, err
                    ))
                })?
            {
                info!("Received GPIO event {:?} on line {}", event, line_id);
                if cmd == Command::Shutdown {
                    if let Some(start_time) = self.config.start_time {
                        let now = Instant::now();
                        let dt: Duration = now - start_time;
                        if dt < DELAY_BEFORE_ACCEPTING_SHUTDOWN_COMMANDS {
                            warn!(
                                "Ignoring shutdown event (time elapsed since start: {:?})",
                                dt
                            );
                            n_received_during_shutdown_delay += 1;
                            continue;
                        }
                    }

                    if n_received_during_shutdown_delay > 10 {
                        warn!("Received too many shutdown events right after startup, shutdown functionality has been disabled");
                        continue;
                    }
                }

                if let Some(cmd) = msg_transformer(cmd.clone()) {
                    if let Err(err) = self.tx.send(cmd) {
                        error!("Failed to transmit GPIO event: {}", err);
                    }
                } else {
                    info!("Dropped button command message: {:?}", cmd);
                }
            }
            Ok(())
        }

        fn run<F>(&mut self, msg_transformer: F) -> Fallible<()>
        where
            F: Fn(Command) -> Option<T> + 'static + Send + Sync,
        {
            let chip = &mut *(self.chip.write().unwrap());
            let msg_transformer = Arc::new(msg_transformer);
            // Spawn thread for requested GPIO lines.
            for (line_id, cmd) in self.map.iter() {
                info!("Listening for {:?} on GPIO line {}", cmd, line_id);
                let line_id = *line_id as u32;
                let line = chip
                    .get_line(line_id)
                    .map_err(|err| Error::IO(format!("Failed to get GPIO line: {:?}", err)))?;
                let cmd = (*cmd).clone();
                let config = self.config.clone();
                let clone = self.clone();
                let msg_transformer = Arc::clone(&msg_transformer);
                let _handle = std::thread::spawn(move || {
                    let res =
                        clone.run_single_event_listener((line, line_id, cmd), msg_transformer);
                    error!("GPIO Listener loop terminated: {:?}", res);
                });
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

const DELAY_BEFORE_ACCEPTING_SHUTDOWN_COMMANDS: Duration = Duration::from_secs(10);
