use failure::{Context, Fallible};
use reqwest;
use rodio::Sink;
use slog_scope::{error, info, warn};
use std::convert::From;
use std::env;
use std::fmt::{self, Display};
use std::io::BufReader;
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::thread::{self, Builder, JoinHandle};

use crossbeam_channel::{self, Receiver, Sender};
use tokio::runtime::Runtime;

use crate::config::Config;
use crate::effects::led::{Led, LedController};

pub use err::*;

use crate::components::stream::FiniteStream;

pub struct HttpPlayer {
    led_controller: Option<Arc<Box<dyn LedController + 'static + Send + Sync>>>,
    handle: Option<JoinHandle<()>>,
    tx: Option<Sender<()>>,
}

impl HttpPlayer {
    pub fn new(
        led_controller: Option<Arc<Box<dyn LedController + 'static + Send + Sync>>>,
    ) -> Fallible<Self> {
        info!("Creating new HttpPlayer...");
        let player = HttpPlayer {
            led_controller,
            handle: None,
            tx: None,
        };

        Ok(player)
    }

    pub fn start_playback(&mut self, url: &str) -> Result<(), Error> {
        let url = url.clone().to_string();
        let (tx, rx) = crossbeam_channel::bounded(1);
        let led_controller = self.led_controller.as_ref().map(|x| Arc::clone(&x));

        let handle = Builder::new()
            .name("http-player".to_string())
            .spawn(move || {
                let mut rt = Runtime::new().unwrap();
                let device = rodio::default_output_device().unwrap();
                let sink = Sink::new(&device);
                let f = async {
                    let response = reqwest::get(&url).await.unwrap();
                    let stream = FiniteStream::from_response(response).unwrap();
                    let source = rodio::Decoder::new(BufReader::new(stream)).unwrap();
                    sink.append(source);
                    sink.play();
                    if let Some(ref led_controller) = led_controller {
                        let _ = led_controller.switch_on(Led::Playback);
                    }
                    let _msg = rx.recv();
                    if let Some(ref led_controller) = led_controller {
                        let _ = led_controller.switch_off(Led::Playback);
                    }
                };
                rt.block_on(f);
            })
            .unwrap();

        self.handle = Some(handle);
        self.tx = Some(tx);
        Ok(())
    }

    pub fn stop_playback(&mut self) -> Result<(), Error> {
        match self.tx {
            Some(ref tx) => {
                info!("Cancelling HTTP Player");
                let tx = tx.clone();
                self.tx = None;
                tx.send(()).unwrap();
            }
            None => {
                warn!("HTTP Player: Nothing to stop");
            }
        }
        Ok(())
    }
}

pub mod err {
    use super::*;

    #[derive(Debug)]
    pub enum Error {
        IO(std::io::Error),
        Http(reqwest::Error),
    }

    impl Display for Error {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            match self {
                Error::IO(err) => write!(f, "HTTP Player IO Error {}", err),
                Error::Http(err) => write!(f, "HTTP Player HTTP Error {}", err),
            }
        }
    }

    impl From<std::io::Error> for Error {
        fn from(err: std::io::Error) -> Self {
            Error::IO(err)
        }
    }

    impl From<reqwest::Error> for Error {
        fn from(err: reqwest::Error) -> Self {
            Error::Http(err)
        }
    }

    impl std::error::Error for Error {}
}

// pub mod external_command {
//     use std::env;
//     use std::fmt::{self, Display};
//     use std::process::{Child, Command, Stdio};
//     use std::sync::Arc;
//     use std::thread::{self, JoinHandle};

//     use failure::{Context, Fallible};

//     use slog_scope::{error, info, warn};
//     use std::convert::From;

//     // use crossbeam_channel::{Receiver, RecvError, RecvTimeoutError, Select, Sender};

//     use crate::config::Config;
//     use crate::effects::led::{Led, LedController};

//     pub use err::*;

//     pub struct HttpPlayer {
//         command: String,
//         led_controller: Option<Arc<Box<dyn LedController + 'static + Send + Sync>>>,
//         child: Option<Child>,
//     }

//     pub struct HttpPlayerHandle {}

//     impl HttpPlayer {
//         pub fn new(
//             config: &Config,
//             led_controller: Option<Arc<Box<dyn LedController + 'static + Send + Sync>>>,
//         ) -> Fallible<Self> {
//             info!("Creating new HttpPlayer...");
//             let command = env::var("HTTP_PLAYER_COMMAND").map_err(Context::new)?;

//             let player = HttpPlayer {
//                 command,
//                 led_controller,
//                 child: None,
//             };

//             Ok(player)
//         }

//         pub fn start_playback(&mut self, url: &str) -> Result<(), Error> {
//             let child = Command::new("omxplayer")
//                 .arg("-o")
//                 .arg("alsa")
//                 .arg("--no-keys")
//                 .arg(url)
//                 .stdin(Stdio::null())
//                 .spawn()?;
//             self.child = Some(child);
//             if let Some(ref led_controller) = self.led_controller {
//                 led_controller.switch_on(Led::Playback);
//             }
//             Ok(())
//         }

//         pub fn stop_playback(&mut self) -> Result<(), Error> {
//             if let Some(ref mut child) = self.child {
//                 if let Err(err) = child.kill() {
//                     warn!("HTTP Player failed to kill child: {}", err);
//                 }
//                 info!("Killed HTTP player child");
//                 self.child = None;
//             }
//             if let Some(ref led_controller) = self.led_controller {
//                 led_controller.switch_off(Led::Playback);
//             }
//             Ok(())
//         }
//     }

//     pub mod err {
//         use super::*;

//         #[derive(Debug)]
//         pub enum Error {
//             IO(std::io::Error),
//             Http(reqwest::Error),
//         }

//         impl Display for Error {
//             fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
//                 match self {
//                     Error::IO(err) => write!(f, "HTTP Player IO Error {}", err),
//                     Error::Http(err) => write!(f, "HTTP Player HTTP Error {}", err),
//                 }
//             }
//         }

//         impl From<std::io::Error> for Error {
//             fn from(err: std::io::Error) -> Self {
//                 Error::IO(err)
//             }
//         }

//         impl From<reqwest::Error> for Error {
//             fn from(err: reqwest::Error) -> Self {
//                 Error::Http(err)
//             }
//         }

//         impl std::error::Error for Error {}
//     }
// }
