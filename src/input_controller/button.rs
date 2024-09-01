use std::time::Instant;

use anyhow::{Context, Result};
use crossbeam_channel::{self, Receiver, Sender};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    VolumeUp,
    VolumeDown,
    PauseContinue,
}

#[derive(Debug, Clone)]
pub struct Config {
    pub volume_up_pin: Option<u32>,
    pub volume_down_pin: Option<u32>,
    pub pause_pin: Option<u32>,
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

    use gpio_cdev::{Chip, EventRequestFlags, Line, LineRequestFlags};
    use serde::Deserialize;
    use tracing::{error, info, trace};

    use super::*;

    #[derive(Debug, Clone)]
    pub struct CdevGpio<T: Clone> {
        map: HashMap<u32, Command>,
        chip: Arc<RwLock<Chip>>,
        tx: Sender<T>,
    }

    #[derive(Deserialize, Debug, Clone)]
    pub struct EnvConfig {
        volume_up_pin: Option<u32>,
        volume_down_pin: Option<u32>,
        pause_pin: Option<u32>,
    }

    impl From<EnvConfig> for Config {
        fn from(env_config: EnvConfig) -> Self {
            Config {
                volume_up_pin: env_config.volume_up_pin,
                volume_down_pin: env_config.volume_down_pin,
                pause_pin: env_config.pause_pin,
            }
        }
    }

    impl EnvConfig {
        pub fn new_from_env() -> Result<Self> {
            Ok(envy::from_env::<EnvConfig>()?)
        }
    }

    impl<T: Clone + Send + 'static> CdevGpio<T>
    where
        T: From<Command>,
    {
        pub fn new_from_env(input_tx: Sender<T>) -> Result<()> {
            info!("Using CdevGpio based Button Controller");
            let env_config =
                EnvConfig::new_from_env().context("Creating CdevGpio based button controller")?;
            let config: Config = env_config.into();
            let mut map = HashMap::new();
            if let Some(pin) = config.volume_up_pin {
                map.insert(pin, Command::VolumeUp);
            }
            if let Some(pin) = config.volume_down_pin {
                map.insert(pin, Command::VolumeDown);
            }
            if let Some(pin) = config.pause_pin {
                map.insert(pin, Command::PauseContinue);
            }
            let chip = Chip::new("/dev/gpiochip0")
                .map_err(|err| Error::IO(format!("Failed to open Chip: {:?}", err)))?;
            let mut gpio_cdev = Self {
                map,
                chip: Arc::new(RwLock::new(chip)),
                tx: input_tx,
            };

            gpio_cdev.run().context("Running GPIO event listener")?;
            Ok(())
        }

        fn run_single_event_listener(
            self,
            (line, line_id, cmd): (Line, u32, Command),
        ) -> Result<()> {
            let mut ts = Instant::now();

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
                if ts.elapsed() < std::time::Duration::from_millis(500) {
                    trace!("Ignoring GPIO event {:?} on line {} since the last event on this line arrived just {}ms ago",
                          event, line_id, ts.elapsed().as_millis());
                    continue;
                }

                trace!("Received GPIO event {:?} on line {}", event, line_id);
                if let Err(err) = self.tx.send(cmd.clone().into()) {
                    error!("Failed to transmit GPIO event: {}", err);
                }
                ts = Instant::now();
            }
            Ok(())
        }

        fn run(&mut self) -> Result<()> {
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
