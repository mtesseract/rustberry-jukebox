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
pub struct Handle {
    handle: Arc<JoinHandle<()>>,
    commands: Sender<PlayerCommand>,
}

pub struct Player {
    effects: Sender<Effects>,
    commands: Receiver<PlayerCommand>,
}

impl Drop for Handle {
    fn drop(&mut self) {
        println!("Destroying Player");
        let _ = self.commands.send(PlayerCommand::Terminate);
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

impl Handle {
    pub fn playback(&self, request: PlaybackRequest) -> Result<(), Error> {
        self.send_command(PlayerCommand::PlaybackRequest(request))
    }
    pub fn send_command(&self, cmd: PlayerCommand) -> Result<(), Error> {
        self.commands.send(cmd)?;
        Ok(())
    }
}

impl Player {
    fn main(self) {
        use PlayerCommand::*;

        let mut stop_effect = None;

        loop {
            let res = self.commands.recv();

            match res {
                Err(err) => {
                    error!("Player: Failed to receive input command: {:?}", err);
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

    pub fn new(effects: Sender<Effects>) -> Handle {
        let (tx, rx) = crossbeam_channel::bounded(1);

        let player = Player {
            commands: rx,
            effects,
        };

        let handle = thread::spawn(|| player.main());

        Handle {
            handle: Arc::new(handle),
            commands: tx,
        }
    }
}

pub mod err {
    use std::convert::From;
    use std::fmt::{self, Display};

    #[derive(Debug)]
    pub enum Error {
        HTTP(reqwest::Error),
        SendError(String),
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

#[cfg(test)]
mod test {
    use failure::Fallible;

    use crate::effects::Effects;

    use super::*;

    #[test]
    fn player_plays_resource_on_playback_request() -> Fallible<()> {
        let (effects_tx, effects_rx) = crossbeam_channel::bounded(10);
        let player_handle = Player::new(effects_tx);
        let playback_requests = vec![
            PlayerCommand::PlaybackRequest(PlaybackRequest::Start(PlaybackResource::SpotifyUri(
                "spotify:track:5j6ZZwA9BnxZi5Bk0Ng4jB".to_string(),
            ))),
            PlayerCommand::PlaybackRequest(PlaybackRequest::Stop),
            PlayerCommand::Terminate,
        ];
        let effects_expected = vec![
            Effects::PlaySpotify {
                spotify_uri: "spotify:track:5j6ZZwA9BnxZi5Bk0Ng4jB".to_string(),
            },
            Effects::StopSpotify,
        ];
        for req in playback_requests.iter() {
            player_handle.send_command(req.clone()).unwrap();
        }
        let produced_effects: Vec<_> = effects_rx.iter().collect();

        assert_eq!(produced_effects, effects_expected);
        Ok(())
    }
}
