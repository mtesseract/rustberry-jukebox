use failure::Fallible;
use gpio_cdev::{Chip, EventRequestFlags, Line, LineRequestFlags};
use serde::Deserialize;
use slog_scope::{error, info};
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

#[derive(Debug, Clone)]
enum TransmitterMessage {
    Command(Command),
    // TransmitterTerminated,
}

struct GpioTransmitter {
    map: HashMap<u32, Command>,
    tx: Sender<TransmitterMessage>,
}

impl GpioTransmitter {
    pub fn new(tx: Sender<TransmitterMessage>, config: &Config) -> Self {
        let mut map = HashMap::new();
        if let Some(shutdown_pin) = config.shutdown_pin {
            map.insert(shutdown_pin, Command::Shutdown);
        }
        Self { tx, map }
    }

    pub fn run(&self) {
        if let Err(err) = self.run_with_result() {
            error!("GPIO Event Transmitter terminated with error: {}", err);
        }
    }

    fn event_listener(
        tx: Sender<TransmitterMessage>,
        line: Line,
        line_id: u32,
        cmd: Command,
    ) -> Fallible<()> {
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
            if let Err(err) = tx.send(TransmitterMessage::Command(cmd.clone())) {
                error!("Failed to transmit GPIO event: {}", err);
            }
        }
        Ok(())
    }

    pub fn run_with_result(&self) -> Fallible<()> {
        let mut chip = Chip::new("/dev/gpiochip0")
            .map_err(|err| Error::IO(format!("Failed to open Chip: {:?}", err)))?;
        // Spawn per-line threads;
        for (line_id, cmd) in self.map.iter() {
            info!("Listening for {:?} on GPIO line {}", cmd, line_id);
            let line = chip
                .get_line(*line_id)
                .map_err(|err| Error::IO(format!("Failed to get GPIO line: {:?}", err)))?;
            let tx = self.tx.clone();
            let cmd = (*cmd).clone();
            let line_id = *line_id;
            let _handle = std::thread::spawn(move || {
                let res = Self::event_listener(tx, line, line_id, cmd);
                error!("GPIO Listener loop terminated: {:?}", res);
            });
        }
        Ok(())
    }
}

pub struct GpioController {
    rx: Receiver<TransmitterMessage>,
}

impl Iterator for GpioController {
    type Item = Command;

    fn next(&mut self) -> Option<Self::Item> {
        match self.rx.recv() {
            Ok(TransmitterMessage::Command(next_command)) => Some(next_command),
            // Ok(TransmitterMessage::TransmitterTerminated) => {
            //     error!("Transmitter terminated");
            //     None
            // }
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
        let (tx, rx): (Sender<TransmitterMessage>, Receiver<TransmitterMessage>) = mpsc::channel();
        let config = (*config).clone();
        let transmitter = GpioTransmitter::new(tx, &config);

        // Spawn threads per GPIO line.
        transmitter.run();

        Ok(Self { rx })
    }
}
