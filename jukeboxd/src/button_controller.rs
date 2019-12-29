use failure::Fallible;
use slog_scope::{error, info};
use std::sync::mpsc;
use std::sync::mpsc::{Receiver, Sender};
use std::time::{Duration, Instant};

pub trait ButtonControllerBackend {
    fn run_event_listener(&mut self, tx: Sender<TransmitterMessage>) -> Fallible<()>;
    fn description(&self) -> String;
}

#[derive(Debug, Clone)]
pub struct Config {
    pub shutdown_pin: Option<u32>,
    pub volume_up_pin: Option<u32>,
    pub volume_down_pin: Option<u32>,
    pub start_time: Option<Instant>,
}

pub mod backends {
    pub mod sysfs_gpio {
        use failure::Fallible;
        use serde::Deserialize;
        use slog_scope::{error, info, warn};
        use std::collections::HashMap;
        use std::convert::From;
        use std::sync::mpsc::Sender;
        use std::time::{Duration, Instant};
        use sysfs_gpio::{Direction, Edge, Pin};

        use super::super::{
            ButtonControllerBackend, Command, Config, TransmitterMessage,
            DELAY_BEFORE_ACCEPTING_SHUTDOWN_COMMANDS,
        };

        #[derive(Clone, Debug)]
        pub struct SysFsGpio {
            map: HashMap<u32, Command>,
            config: Config,
        }

        #[derive(Deserialize, Debug, Clone)]
        pub struct EnvConfig {
            shutdown_pin: Option<u32>,
            volume_up_pin: Option<u32>,
            volume_down_pin: Option<u32>,
        }

        impl From<EnvConfig> for Config {
            fn from(env_config: EnvConfig) -> Self {
                Config {
                    shutdown_pin: env_config.shutdown_pin,
                    start_time: Some(Instant::now()),
                    volume_up_pin: env_config.volume_up_pin,
                    volume_down_pin: env_config.volume_down_pin,
                }
            }
        }

        impl EnvConfig {
            pub fn new_from_env() -> Fallible<Self> {
                Ok(envy::from_env::<EnvConfig>()?)
            }
        }

        impl SysFsGpio {
            pub fn new_from_env() -> Fallible<Self> {
                info!("Using SysFsGpio backend in Button Controller");
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
                Ok(Self { map, config })
            }

            pub fn run_single_event_listener(
                &self,
                tx: Sender<TransmitterMessage>,
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
        }

        impl ButtonControllerBackend for SysFsGpio {
            fn description(&self) -> String {
                "SysFS GPIO Backend".to_string()
            }

            fn run_event_listener(&mut self, tx: Sender<TransmitterMessage>) -> Fallible<()> {
                for (line_id, cmd) in self.map.iter() {
                    info!("Listening for {:?} on GPIO line {}", cmd, line_id);
                    let line_id = *line_id as u64;
                    let input = Pin::new(line_id);
                    let tx = tx.clone();
                    let cmd = (*cmd).clone();
                    let copy = self.clone();
                    let _handle = std::thread::spawn(move || {
                        let res = copy.run_single_event_listener(tx, input, line_id, cmd);
                        error!("GPIO Listener loop terminated: {:?}", res);
                    });
                }
                Ok(())
            }
        }
    }

    pub mod cdev_gpio {
        use super::super::{
            ButtonControllerBackend, Command, Config, Error, TransmitterMessage,
            DELAY_BEFORE_ACCEPTING_SHUTDOWN_COMMANDS,
        };
        use failure::Fallible;
        use gpio_cdev::{Chip, EventRequestFlags, Line, LineRequestFlags};
        use serde::Deserialize;
        use slog_scope::{error, info, warn};
        use std::collections::HashMap;
        use std::convert::From;
        use std::sync::mpsc::Sender;
        use std::time::{Duration, Instant};

        #[derive(Debug)]
        pub struct CdevGpio {
            map: HashMap<u32, Command>,
            chip: Chip,
            config: Config,
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
            pub fn new_from_env() -> Fallible<Self> {
                info!("Using CdevGpio backend in Button Controller");
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
                Ok(Self { map, chip, config })
            }

            pub fn run_single_event_listener(
                config: Config,
                tx: Sender<TransmitterMessage>,
                line: Line,
                line_id: u32,
                cmd: Command,
            ) -> Fallible<()> {
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
                        if let Some(start_time) = config.start_time {
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
                Ok(())
            }
        }

        impl ButtonControllerBackend for CdevGpio {
            fn description(&self) -> String {
                "cdev GPIO Backend".to_string()
            }

            fn run_event_listener(&mut self, tx: Sender<TransmitterMessage>) -> Fallible<()> {
                for (line_id, cmd) in self.map.iter() {
                    info!("Listening for {:?} on GPIO line {}", cmd, line_id);
                    let line_id = *line_id as u32;
                    let line = self
                        .chip
                        .get_line(line_id)
                        .map_err(|err| Error::IO(format!("Failed to get GPIO line: {:?}", err)))?;
                    let tx = tx.clone();
                    let cmd = (*cmd).clone();
                    let config = self.config.clone();
                    let _handle = std::thread::spawn(move || {
                        let res =
                            CdevGpio::run_single_event_listener(config, tx, line, line_id, cmd);
                        error!("GPIO Listener loop terminated: {:?}", res);
                    });
                }
                Ok(())
            }
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    Shutdown,
    VolumeUp,
    VolumeDown,
}

#[derive(Debug, Clone)]
pub enum TransmitterMessage {
    Command(Command),
}

const DELAY_BEFORE_ACCEPTING_SHUTDOWN_COMMANDS: Duration = Duration::from_secs(10);

pub struct ButtonController {
    rx: Receiver<TransmitterMessage>,
}

impl Iterator for ButtonController {
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

impl ButtonController {
    pub fn new<BCB: ButtonControllerBackend>(mut backend: BCB) -> Fallible<Self> {
        info!(
            "Creating Button Controller with backend {}",
            backend.description()
        );
        let (tx, rx): (Sender<TransmitterMessage>, Receiver<TransmitterMessage>) = mpsc::channel();
        backend.run_event_listener(tx)?;
        Ok(Self { rx })
    }
}
