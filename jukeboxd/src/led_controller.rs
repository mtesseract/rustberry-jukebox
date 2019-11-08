use failure::Fallible;
use slog_scope::{info, warn};

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Led {
    Playback,
}

pub trait LedControllerBackend {
    fn description(&self) -> String;
    fn switch_on(&mut self, line: Led) -> Fallible<()>;
    fn switch_off(&mut self, line: Led) -> Fallible<()>;
}

pub struct LedController {
    backend: Box<dyn LedControllerBackend + Send + Sync + 'static>,
}

impl LedController {
    pub fn new<LCB: LedControllerBackend + Send + Sync + 'static>(backend: LCB) -> Fallible<Self> {
        info!(
            "Creating LED Controller using backend {}",
            backend.description()
        );
        Ok(LedController {
            backend: Box::new(backend),
        })
    }

    pub fn switch_on(&mut self, led: Led) {
        if let Err(err) = self.backend.switch_on(led) {
            warn!("Failed to switch on LED {:?}: {}", led, err);
        }
    }

    pub fn switch_off(&mut self, led: Led) {
        if let Err(err) = self.backend.switch_off(led) {
            warn!("Failed to switch off LED {:?}: {}", led, err);
        }
    }
}

pub mod backends {
    pub mod gpio_cdev {
        use super::super::{Error, Led, LedControllerBackend};
        use failure::Fallible;
        use gpio_cdev::{Chip, LineHandle, LineRequestFlags};
        use serde::Deserialize;
        use slog_scope::{info, warn};

        #[derive(Deserialize)]
        struct Config {
            playback_led_gpio_line: Option<u32>,
        }

        pub struct GpioCdev {
            config: Config,
            chip: Chip,
            playback_led: Option<LineHandle>,
        }

        impl GpioCdev {
            pub fn new() -> Fallible<Self> {
                let config: Config = envy::from_env()?;
                if !led_to_line(&config, Led::Playback).is_some() {
                    warn!("No GPIO line configured for LED {:?}. Skipping all future requests for this LED.", Led::Playback);
                }
                let mut chip = Chip::new("/dev/gpiochip0")
                    .map_err(|err| Error::IO(format!("Failed to open Chip: {:?}", err)))?;
                let playback_led = match config.playback_led_gpio_line {
                    Some(line) => {
                        let output = chip.get_line(line).map_err(|err| {
                            Error::IO(format!("Failed to get Playback LED GPIO line: {:?}", err))
                        })?;
                        let playback_led = output
                            .request(LineRequestFlags::OUTPUT, 0, "led-gpio")
                            .map_err(|err| {
                                Error::IO(format!(
                                    "Failed to request output handle for GPIO line: {:?}",
                                    err
                                ))
                            })?;
                        Some(playback_led)
                    }
                    None => None,
                };

                Ok(GpioCdev {
                    chip,
                    config,
                    playback_led,
                })
            }
        }

        fn led_to_line(config: &Config, led: Led) -> Option<u32> {
            match led {
                Led::Playback => config.playback_led_gpio_line,
            }
        }

        impl LedControllerBackend for GpioCdev {
            fn description(&self) -> String {
                "gpio-cdev backend".to_string()
            }
            fn switch_on(&mut self, led: Led) -> Fallible<()> {
                if let Some(ref playback_led) = self.playback_led {
                    playback_led.set_value(1).map_err(|err| {
                        Error::IO(format!(
                            "Failed to activate Playback LED GPIO line, {:?}",
                            err
                        ))
                    })?;
                    info!("Switched on GPIO line for LED {:?}", led);
                }
                Ok(())
            }
            fn switch_off(&mut self, led: Led) -> Fallible<()> {
                if let Some(ref playback_led) = self.playback_led {
                    playback_led.set_value(0).map_err(|err| {
                        Error::IO(format!(
                            "Failed to deactivate Playback LED GPIO line {:?}",
                            err
                        ))
                    })?;
                    info!("Switched off GPIO line for LED {:?}", led);
                }
                Ok(())
            }
        }
    }
}
