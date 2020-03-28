use std::cell::RefCell;
use std::sync::Arc;

use failure::Fallible;

use serde::{Deserialize, Serialize};
use slog_scope::error;

use crate::effects::Interpreter;

pub use err::*;

#[derive(Debug, Clone)]
pub enum PlayerCommand {
    PlaybackRequest(PlaybackRequest),
    Terminate,
}

pub struct Player {
    interpreter: Arc<Box<dyn Send + Sync + 'static + Interpreter>>,
    stop_eff: RefCell<Option<Box<dyn Fn() -> Fallible<()>>>>,
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

impl Player {
    pub fn playback(&self, req: PlaybackRequest) -> Result<(), Error> {
        match req {
            self::PlaybackRequest::Start(resource) => match resource {
                PlaybackResource::SpotifyUri(spotify_uri) => {
                    let interpreter = self.interpreter.clone();
                    *(self.stop_eff.borrow_mut()) =
                        Some(Box::new(move || interpreter.stop_spotify())
                            as Box<dyn Fn() -> Fallible<()>>);
                    if let Err(err) = self.interpreter.play_spotify(&spotify_uri) {
                        error!("Failed to play Spotify URI '{}': {}", spotify_uri, err);
                    }
                }
                PlaybackResource::Http(url) => {
                    let interpreter = self.interpreter.clone();
                    *(self.stop_eff.borrow_mut()) =
                        Some(Box::new(move || interpreter.stop_http())
                            as Box<dyn Fn() -> Fallible<()>>);
                    if let Err(err) = self.interpreter.play_http(&url) {
                        error!("Failed to play HTTP URI '{}': {}", url, err);
                    }
                }
            },
            self::PlaybackRequest::Stop => {
                // if let Some(stop_eff) = self.stop_eff.borrow() {
                //     if let Err(err) = stop_eff() {
                //         error!("Failed to stop playback: {}", err);
                //     }
                //     stop_effect = None;
                // }
            }
        }
        Ok(())
    }
    pub fn new(interpreter: Arc<Box<dyn Send + Sync + 'static + Interpreter>>) -> Self {
        let player = Player {
            interpreter,
            stop_eff: RefCell::new(None),
        };
        player
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

    use crate::effects::{test::TestInterpreter, Effects};

    use super::*;

    #[test]
    fn player_plays_resource_on_playback_request() -> Fallible<()> {
        let (interpreter, effects_rx) = TestInterpreter::new();
        let interpreter =
            Arc::new(Box::new(interpreter) as Box<dyn Interpreter + Send + Sync + 'static>);
        let player_handle = Player::new(interpreter);
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
