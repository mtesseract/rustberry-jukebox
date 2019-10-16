use failure::Fallible;
use gpio_cdev::{Chip, EventRequestFlags, LineRequestFlags};
use serde::Deserialize;
use slog_scope::error;
use std::collections::HashMap;
use std::sync::mpsc;
use std::sync::mpsc::{Receiver, Sender};

#[derive(Deserialize, Debug, Clone)]
pub struct Config {
    shutdown_pin: Option<u32>,
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    Shutdown,
}

struct GpioTransmitter {
    map: HashMap<u32, Command>,
    tx: Sender<Command>,
}

impl GpioTransmitter {
    pub fn new(tx: Sender<Command>, config: &Config) -> Self {
        let mut map = HashMap::new();
        if let Some(shutdown_pin) = config.shutdown_pin {
            map.insert(shutdown_pin, Command::Shutdown);
        }
        Self { tx, map }
    }

    pub fn run(&self) {
        match self.run_with_result() {
            Ok(_) => {
                error!("GPIO Event Transmitter terminated.");
            }
            Err(err) => {
                error!("GPIO Event Transmitter terminated with error: {}", err);
            }
        }
    }

    pub fn run_with_result(&self) -> Fallible<()> {
        let mut chip = Chip::new("/dev/gpiochip0")
            .map_err(|_fixme| Error::IO("Failed to open Chip".to_string()))?;

        for (line_id, cmd) in self.map.iter() {
            let line = chip
                .get_line(*line_id)
                .map_err(|_fixme| Error::IO("Failed to get GPIO line".to_string()))?;
            let tx = self.tx.clone();
            let cmd = (*cmd).clone();
            std::thread::spawn(move || {
                for _event in line.events(
                    LineRequestFlags::INPUT,
                    EventRequestFlags::RISING_EDGE,
                    "read-input",
                ) {
                    if let Err(err) = tx.send(cmd.clone()) {
                        error!("Failed to transmit GPIO event: {}", err);
                    }
                }
            });
        }
        Ok(())
    }
}

pub struct GpioController {
    rx: Receiver<Command>,
    // config: Config,
}

impl Iterator for GpioController {
    type Item = Command;

    fn next(&mut self) -> Option<Self::Item> {
        match self.rx.recv() {
            Ok(next_item) => Some(next_item),
            Err(err) => {
                error!("Failed to receive next command: {}", err);
                None
            }
        }
    }
}

impl GpioController {
    pub fn new_from_env() -> Fallible<Self> {
        let config = envy::from_env::<Config>()?;
        Self::new(&config)
    }

    pub fn new(config: &Config) -> Fallible<Self> {
        let (tx, rx): (Sender<Command>, Receiver<Command>) = mpsc::channel();
        let config = (*config).clone();
        let transmitter = GpioTransmitter::new(tx, &config);

        std::thread::spawn(move || transmitter.run());

        Ok(Self { rx })
    }
}
