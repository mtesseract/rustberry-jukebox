use std::time::{Duration, Instant};

use failure::Fallible;
use tokio::sync::broadcast::{channel, Receiver, Sender};

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
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

#[derive(Clone)]
pub struct Handle {
    channel: Sender<Command>,
}

impl Handle {
    pub fn receiver(self) -> Receiver<Command> {
        self.channel.subscribe()
    }
}

pub mod cdev_gpio {
    use std::collections::HashMap;
    use std::convert::From;
    use std::sync::{Arc, RwLock};

    use gpio_cdev::{Chip, EventRequestFlags, Line, LineRequestFlags};
    use serde::Deserialize;
    use slog_scope::{error, info, warn};

    use super::*;

    #[derive(Debug, Clone)]
    pub struct CdevGpio {
        map: HashMap<u32, Command>,
        chip: Arc<RwLock<Chip>>,
        config: Config,
        tx: Sender<Command>,
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

    impl CdevGpio {
        pub fn new_from_env() -> Fallible<Handle> {
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
            let (tx, _rx) = channel(128);
            let mut gpio_cdev = Self {
                map,
                chip: Arc::new(RwLock::new(chip)),
                config,
                tx: tx.clone(),
            };

            gpio_cdev.run()?;
            Ok(Handle { channel: tx })
        }

        fn run_single_event_listener(
            self,
            (line, line_id, cmd): (Line, u32, Command),
        ) -> Fallible<()> {
            let mut n_received_during_shutdown_delay = 0;
            info!("Listening for GPIO events on line {}", line_id);
            let mut ts: Option<std::time::Instant> = None;

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

                if let Some(ref ts) = ts {
                    let elapsed = ts.elapsed();
                    if elapsed < std::time::Duration::from_millis(500) {
                        info!("Ignoring GPIO event {:?} on line {} since the last event on this line arrived just {}ms ago", event, line_id, elapsed.as_millis());
                        continue;
                    }
                }

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

                if self.tx.receiver_count() > 0 {
                    let tx = self.tx.clone();
                    if let Err(err) = tx.send(cmd) {
                        error!("Failed to transmit GPIO event: {:?}", err);
                    }
                    ts = Some(std::time::Instant::now());
                } else {
                    warn!("Skpping transmitting of GPIO event since no receiver connected");
                }
            }
            Ok(())
        }

        fn run(&mut self) -> Fallible<()> {
            let chip = &mut *(self.chip.write().unwrap());
            // Spawn threads for requested GPIO lines.
            for (line_id, cmd) in self.map.iter() {
                info!("Listening for {:?} on GPIO line {}", cmd, line_id);
                let line_id = *line_id as u32;
                let line = chip
                    .get_line(line_id)
                    .map_err(|err| Error::IO(format!("Failed to get GPIO line: {:?}", err)))?;
                let cmd = (*cmd).clone();
                let clone = self.clone();
                let _handle = std::thread::Builder::new()
                    .name(format!("button-controller-{}", line_id))
                    .spawn(move || {
                        let res = clone.run_single_event_listener((line, line_id, cmd));
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

// This is just a safety measure, preventing immediate shutdowns if the button
// controller is (for whatever reason) transmitting immediate shutdown events.
const DELAY_BEFORE_ACCEPTING_SHUTDOWN_COMMANDS: Duration = Duration::from_secs(10);
