use std::env;
use std::fmt::{self, Display};
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::thread::{self, JoinHandle};

use failure::{Context, Fallible};

use slog_scope::{error, info, warn};
use std::convert::From;

// use crossbeam_channel::{Receiver, RecvError, RecvTimeoutError, Select, Sender};

use crate::config::Config;
use crate::effects::led::{Led, LedController};

pub use err::*;

pub struct HttpPlayer {
    command: String,
    led_controller: Arc<Box<dyn LedController + 'static + Send + Sync>>,
    child: Option<Child>,
}

pub struct HttpPlayerHandle {}

impl HttpPlayer {
    pub fn new(
        config: &Config,
        led_controller: Arc<Box<dyn LedController + 'static + Send + Sync>>,
    ) -> Fallible<Self> {
        info!("Creating new HttpPlayer...");
        let command = env::var("HTTP_PLAYER_COMMAND").map_err(Context::new)?;

        let player = HttpPlayer {
            command,
            led_controller,
            child: None,
        };

        Ok(player)
    }

    pub fn start_playback(&mut self, url: &str) -> Result<(), Error> {
        let child = Command::new("omxplayer")
            .arg("-o")
            .arg("hdmi")
            .arg("--no-keys")
            .arg(url)
            .stdin(Stdio::null())
            .spawn()?;
        self.child = Some(child);
        self.led_controller.switch_on(Led::Playback);
        Ok(())
    }

    pub fn stop_playback(&mut self) -> Result<(), Error> {
        if let Some(ref mut child) = self.child {
            if let Err(err) = child.kill() {
                warn!("HTTP Player failed to kill child: {}", err);
            }
            info!("Killed HTTP player child");
            self.child = None;
        }
        self.led_controller.switch_off(Led::Playback);

        Ok(())
    }
}

pub mod err {
    use super::*;

    #[derive(Debug)]
    pub enum Error {
        IO(std::io::Error),
    }

    impl Display for Error {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            match self {
                Error::IO(err) => write!(f, "HTTP Player IO Error {}", err),
            }
        }
    }

    impl From<std::io::Error> for Error {
        fn from(err: std::io::Error) -> Self {
            Error::IO(err)
        }
    }

    impl std::error::Error for Error {}
}
