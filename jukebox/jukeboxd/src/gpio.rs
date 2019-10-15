use failure::Fallible;
use gpio_cdev::{Chip, LineRequestFlags};
use serde::Deserialize;
use slog_scope::error;
use std::collections::HashMap;
use std::sync::mpsc;
use std::sync::mpsc::{Receiver, Sender};

#[derive(Deserialize, Debug, Clone)]
struct Config {
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
        self.run_with_result().expect("run with result");
    }

    pub fn run_with_result(&self) -> Fallible<()> {
        let mut chip = Chip::new("/dev/gpiochip0")
            .map_err(|_fixme| Error::IO("Failed to open Chip".to_string()))?;
        let line_ids: Vec<u32> = self.map.keys().cloned().collect();
        let n_lines = line_ids.len();
        let line_defaults: Vec<u8> = (0..n_lines).map(|_| 0).collect();
        let lines = chip
            .get_lines(&line_ids)
            .map_err(|_fixme| Error::IO("Failed to get GPIO lines".to_string()))?;

        let req = lines
            .request(LineRequestFlags::INPUT, &line_defaults, "read-input")
            .map_err(|_fixme| Error::IO("Failed to request events from line".to_string()))?;
        // for _ in 1..4 {
        //     println!("Value: {:?}", handle.get_value()?);
        // }
        Ok(())
    }
}
struct GpioController {
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
