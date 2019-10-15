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
        let mut chip = Chip::new("/dev/gpiochip0")?;
        let line_ids: Vec<u32> = self.map.keys().cloned().collect();
        let lines = chip.get_lines(&line_ids)?;

        //     .request(LineRequestFlags::INPUT, 0, "read-input")?;
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

        std::thread::spawn(|| transmitter.run());

        Ok(Self { rx })
    }
}
