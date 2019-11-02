use failure::Fallible;

pub mod backends {

    pub mod gpio_cdev {
        use super::super::{Error, LedControllerBackend};
        use failure::Fallible;
        use gpio_cdev::{Chip, LineRequestFlags};


        struct Config {
            playback_led_gpio_line: Option<u32>,
        }

        pub struct GpioCdev {
            chip: Chip,
        }

        impl GpioCdev {
            pub fn new() -> Fallible<Self> {
                let chip = Chip::new("/dev/gpiochip0")
                    .map_err(|err| Error::IO(format!("Failed to open Chip: {:?}", err)))?;
                Ok(GpioCdev { chip })
            }
        }

        impl LedControllerBackend for GpioCdev {
            fn description(&self) -> String {
                "gpio-cdev backend".to_string()
            }
            fn switch_on(&mut self, led: Led) -> Fallible<()> {
                match self.led {
                    Nonw => {
                        
                    }
                }
                let output = self
                    .chip
                    .get_line(line)
                    .map_err(|err| Error::IO(format!("Failed to get GPIO line: {:?}", err)))?;
                let output_handle = output
                    .request(LineRequestFlags::OUTPUT, 0, "led-gpio")
                    .map_err(|err| {
                        Error::IO(format!(
                            "Failed to request output handle for GPIO line: {:?}",
                            err
                        ))
                    })?;
                output_handle.set_value(1).map_err(|err| {
                    Error::IO(format!(
                        "Failed to activate output GPIO line {}, {:?}",
                        line, err
                    ))
                })?;
                Ok(())
            }
            fn switch_off(&mut self, led Led) -> Fallible<()> {
                let output = self
                    .chip
                    .get_line(line)
                    .map_err(|err| Error::IO(format!("Failed to get GPIO line: {:?}", err)))?;
                let output_handle = output
                    .request(LineRequestFlags::OUTPUT, 0, "led-gpio")
                    .map_err(|err| {
                        Error::IO(format!(
                            "Failed to request output handle for GPIO line: {:?}",
                            err
                        ))
                    })?;
                output_handle.set_value(0).map_err(|err| {
                    Error::IO(format!(
                        "Failed to activate output GPIO line {}, {:?}",
                        line, err
                    ))
                })?;
                Ok(())
            }
        }

    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Led {
    Playback
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
        Ok(LedController {
            backend: Box::new(backend),
        })
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
