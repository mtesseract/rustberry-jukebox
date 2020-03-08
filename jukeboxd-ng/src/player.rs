use std::sync::Arc;
use std::thread::{self, JoinHandle};

use crossbeam_channel::{Receiver, Select, Sender};
use serde::{Deserialize, Serialize};
use slog_scope::{error, info};

use crate::effects::Effects;

pub use err::*;

#[derive(Debug, Clone)]
pub enum PlayerCommand {
    PlaybackRequest(PlaybackRequest),
    Terminate,
}

#[derive(Debug, Clone)]
pub struct PlayerHandle {
    handle: Arc<JoinHandle<()>>,
    commands: Sender<PlayerCommand>,
}

pub struct Player {
    effects: Sender<Effects>,
    commands: Receiver<PlayerCommand>,
}

impl Drop for PlayerHandle {
    fn drop(&mut self) {
        println!("Destroying Player");
        // FIXME?
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum PlaybackRequest {
    Start(PlaybackResource),
    Stop,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum PlaybackResource {
    SpotifyUri(String),
    Http(String),
}

#[derive(Debug, Clone, Serialize)]
struct StartPlayback {
    #[serde(skip_serializing_if = "Option::is_none")]
    context_uri: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    uris: Option<Vec<String>>,
}

impl PlayerHandle {
    pub fn playback(&self, request: PlaybackRequest) -> Result<(), Error> {
        self.commands
            .send(PlayerCommand::PlaybackRequest(request))?;
        Ok(())
    }
}

impl Player {
    fn main(self) {
        use PlayerCommand::*;

        let mut stop_effect = None;

        let rs = vec![self.commands.clone()];
        // Build a list of operations.
        let mut sel = Select::new();
        for r in &rs {
            sel.recv(r);
        }

        loop {
            // Wait until a receive operation becomes ready and try executing it.
            let index = sel.ready();
            let res = rs[index].try_recv();

            match res {
                Err(err) => {
                    if err.is_empty() {
                        // If the operation turns out not to be ready, retry.
                        continue;
                    } else {
                        error!("Player: Failed to receive input command: {:?}", err);
                    }
                }
                Ok(cmd) => match cmd {
                    PlayerCommand::PlaybackRequest(req) => match req {
                        self::PlaybackRequest::Start(resource) => match resource {
                            PlaybackResource::SpotifyUri(spotify_uri) => {
                                stop_effect = Some(Effects::StopSpotify);
                                self.effects
                                    .send(Effects::PlaySpotify { spotify_uri })
                                    .unwrap();
                            }
                            PlaybackResource::Http(url) => {
                                stop_effect = Some(Effects::StopHttp);
                                self.effects.send(Effects::PlayHttp { url }).unwrap();
                            }
                        },
                        self::PlaybackRequest::Stop => {
                            let eff = stop_effect.clone().unwrap();
                            self.effects.send(eff).unwrap();
                        }
                    },
                    Terminate => {
                        info!("Player received Terminate command, terminating");
                        break;
                    }
                },
            }
        }
    }

    pub fn new(effects: Sender<Effects>) -> PlayerHandle {
        let (commands_tx, commands_rx) = crossbeam_channel::bounded(1);

        let player = Player {
            commands: commands_rx,
            effects,
        };

        let handle = thread::spawn(|| player.main());

        PlayerHandle {
            handle: Arc::new(handle),
            commands: commands_tx,
        }
    }
}

pub mod err {
    use std::convert::From;
    use std::fmt::{self, Display};

    // use crossbeam_channel::RecvError;

    #[derive(Debug)]
    pub enum Error {
        HTTP(reqwest::Error),
        SendError(String),
    }

    impl Error {
        // pub fn is_client_error(&self) -> bool {
        //     match self {
        //         Error::HTTP(err) => err.status().map(|s| s.is_client_error()).unwrap_or(false),
        //         Error::SendError(_) => false,
        //     }
        // }
    }

    impl Display for Error {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            match self {
                Error::HTTP(err) => write!(f, "Spotify HTTP Error {}", err),
                Error::SendError(err) => {
                    write!(f, "Failed to transmit command via channel: {}", err)
                }
            }
        }
    }

    impl From<reqwest::Error> for Error {
        fn from(err: reqwest::Error) -> Self {
            Error::HTTP(err)
        }
    }

    impl<T> From<crossbeam_channel::SendError<T>> for Error {
        fn from(err: crossbeam_channel::SendError<T>) -> Self {
            Error::SendError(err.to_string())
        }
    }
    impl std::error::Error for Error {}
}
