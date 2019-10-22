use failure::Fallible;
use serde::Deserialize;
use slog_scope::{error, info, warn};
use std::collections::HashMap;
use std::convert::From;
use std::sync::mpsc;
use std::sync::mpsc::{Receiver, Sender};
use std::time::{Duration, Instant};
use sysfs_gpio::{Direction, Edge, Pin};

#[derive(Deserialize, Debug, Clone)]
pub struct EnvConfig {
    shutdown_pin: Option<u32>,
}

#[derive(Debug, Clone)]
pub struct Config {
    pub shutdown_pin: Option<u32>,
    pub start_time: Option<Instant>,
}

impl EnvConfig {
    pub fn new_from_env() -> Fallible<Self> {
        Ok(envy::from_env::<EnvConfig>()?)
    }
}

impl From<EnvConfig> for Config {
    fn from(env_config: EnvConfig) -> Self {
        Config {
            shutdown_pin: env_config.shutdown_pin,
            start_time: None,
        }
    }
}

impl Config {
    pub fn new_from_env() -> Fallible<Self> {
        let env_config = EnvConfig::new_from_env()?;
        Ok(env_config.into())
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
    start_time: Option<Instant>,
    map: HashMap<u32, Command>,
    tx: Sender<TransmitterMessage>,
}

const DELAY_BEFORE_ACCEPTING_SHUTDOWN_COMMANDS: Duration = Duration::from_secs(10);

impl GpioTransmitter {
    pub fn new(tx: Sender<TransmitterMessage>, config: &Config) -> Self {
        let mut map = HashMap::new();
        if let Some(shutdown_pin) = config.shutdown_pin {
            map.insert(shutdown_pin, Command::Shutdown);
        }
        Self {
            tx,
            map,
            start_time: config.start_time,
        }
    }

    pub fn run(&self) {
        if let Err(err) = self.run_with_result() {
            error!("GPIO Event Transmitter terminated with error: {}", err);
        }
    }

    fn event_listener(
        tx: Sender<TransmitterMessage>,
        start_time: Option<Instant>,
        input: Pin,
        line_id: u64,
        cmd: Command,
    ) -> Fallible<()> {
        info!("Listening for GPIO events on line {}", line_id);
        input.with_exported(|| {
            input.set_direction(Direction::In)?;
            input.set_edge(Edge::FallingEdge)?;
            let mut poller = input.get_poller()?;
            let mut n_received_during_shutdown_delay = 0;
            loop {
                match poller.poll(1000) {
                    Ok(Some(value)) => {
                        info!("Received GPIO event {} on line {}", value, line_id);
                        // Hacky special handling for shutdown command.
                        if cmd == Command::Shutdown {
                            if let Some(start_time) = start_time {
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

                        if let Err(err) = tx.send(TransmitterMessage::Command(cmd.clone())) {
                            error!("Failed to transmit GPIO event: {}", err);
                        }
                    }
                    Ok(None) => {
                        continue;
                    }
                    Err(err) => {
                        warn!(
                            "Polling for GPIO events for line {} failed: {}",
                            line_id, err
                        );
                    }
                }
            }
        })?;

        Ok(())
    }

    pub fn run_with_result(&self) -> Fallible<()> {
        // Spawn per-line threads;
        for (line_id, cmd) in self.map.iter() {
            info!("Listening for {:?} on GPIO line {}", cmd, line_id);
            let line_id = *line_id as u64;
            let input = Pin::new(line_id);
            let tx = self.tx.clone();
            let start_time = self.start_time.clone();
            let cmd = (*cmd).clone();
            let _handle = std::thread::spawn(move || {
                let res = Self::event_listener(tx, start_time, input, line_id, cmd);
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
        let config = Config::new_from_env()?;
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
