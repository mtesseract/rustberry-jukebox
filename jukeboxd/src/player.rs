use std::cell::RefCell;
use std::fmt;
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use slog_scope::{error, info};

use crate::effects::Interpreter;

pub use err::*;

#[derive(Debug, Clone)]
pub struct PauseState {
    pub pos: Duration,
}

#[derive(Debug, Clone)]
pub enum PlayerCommand {
    PlaybackRequest(PlaybackRequest),
    Terminate,
}

type StopPlayEffect = Box<dyn Fn() -> Result<(), failure::Error>>;

impl fmt::Display for PlayerState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PlayerState::Idle => write!(f, "Idle"),
            PlayerState::Playing {
                resource, since, ..
            } => write!(
                f,
                "Playing {{ resource = {:?}, since = {:?} }}",
                resource, since
            ),
            PlayerState::Paused { at, prev_resource } => write!(
                f,
                "Paused {{ prev_resource = {:?}, at = {:?} }}",
                prev_resource, at
            ),
        }
    }
}
enum PlayerState {
    Idle,
    Playing {
        resource: PlaybackResource,
        since: std::time::Instant,
        stop_eff: StopPlayEffect,
    },
    Paused {
        at: std::time::Duration,
        prev_resource: PlaybackResource,
    },
}
pub struct Player {
    interpreter: Arc<Box<dyn Send + Sync + 'static + Interpreter>>,
    state: RefCell<PlayerState>,
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
    fn play_resource(
        &self,
        resource: &PlaybackResource,
        pause_state: Option<PauseState>,
    ) -> Result<StopPlayEffect, failure::Error> {
        match resource {
            PlaybackResource::SpotifyUri(ref spotify_uri) => {
                let interpreter = self.interpreter.clone();
                if let Err(err) = self.interpreter.play_spotify(spotify_uri, pause_state) {
                    error!("Failed to play Spotify URI '{}': {}", spotify_uri, err);
                    Err(Error::Spotify(err).into())
                } else {
                    Ok(Box::new(move || interpreter.stop_spotify()) as StopPlayEffect)
                }
            }
            PlaybackResource::Http(ref url) => {
                let interpreter = self.interpreter.clone();
                if let Err(err) = self.interpreter.play_http(url, pause_state) {
                    error!("Failed to play HTTP URI '{}': {}", url, err);
                    Err(Error::HTTP(err).into())
                } else {
                    Ok(Box::new(move || interpreter.stop_http()) as StopPlayEffect)
                }
            }
        }
    }

    pub fn playback(&self, req: PlaybackRequest) -> Result<(), failure::Error> {
        let old_state = (*self.state.borrow()).to_string();

        match req {
            self::PlaybackRequest::Start(resource) => {
                let mut state = self.state.borrow_mut();
                match &*state {
                    PlayerState::Idle => {
                        let stop_eff = self.play_resource(&resource, None)?;
                        *state = PlayerState::Playing {
                            since: std::time::Instant::now(),
                            stop_eff,
                            resource,
                        };
                    }
                    PlayerState::Playing { resource, .. } => {
                        // This code path should atually not happen.
                        let stop_eff = self.play_resource(&resource, None)?;
                        *state = PlayerState::Playing {
                            since: std::time::Instant::now(),
                            stop_eff,
                            resource: (*resource).clone(),
                        };
                    }
                    PlayerState::Paused { at, prev_resource } => {
                        let pause_state = if resource == *prev_resource {
                            // continue at position
                            Some(PauseState { pos: *at })
                        } else {
                            // new resource, play from beginning
                            None
                        };
                        let stop_eff = self.play_resource(&resource, pause_state)?;
                        let now = Instant::now();
                        let since = now.checked_sub(*at).unwrap();
                        *state = PlayerState::Playing {
                            since,
                            stop_eff,
                            resource,
                        };
                    }
                }
            }
            self::PlaybackRequest::Stop => {
                let mut state = self.state.borrow_mut();
                match &*state {
                    PlayerState::Idle | PlayerState::Paused { .. } => {
                        // nothing to do here.
                    }
                    PlayerState::Playing {
                        since,
                        resource,
                        stop_eff,
                    } => {
                        if let Err(err) = stop_eff() {
                            error!("Failed to execute playback stop: {}", err);
                            return Err(err);
                        }
                        let now = std::time::Instant::now();
                        let paused_at = now.duration_since(*since);
                        *state = PlayerState::Paused {
                            prev_resource: resource.clone(),
                            at: paused_at,
                        };
                    }
                }
            }
        }

        let new_state = (*self.state.borrow()).to_string();

        info!(
            "Player State Transition from: {} -> {}",
            old_state, new_state
        );
        Ok(())
    }

    pub fn new(interpreter: Arc<Box<dyn Send + Sync + 'static + Interpreter>>) -> Self {
        let player = Player {
            interpreter,
            state: RefCell::new(PlayerState::Idle),
        };
        player
    }
}

pub mod err {
    use std::convert::From;
    use std::fmt::{self, Display};

    #[derive(Debug)]
    pub enum Error {
        Spotify(failure::Error),
        HTTP(failure::Error),
        SendError(String),
    }

    impl Display for Error {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            match self {
                Error::Spotify(err) => write!(f, "Spotify Error {}", err),
                Error::HTTP(err) => write!(f, "HTTP Error {}", err),
                Error::SendError(err) => {
                    write!(f, "Failed to transmit command via channel: {}", err)
                }
            }
        }
    }

    impl From<reqwest::Error> for Error {
        fn from(err: reqwest::Error) -> Self {
            Error::HTTP(err.into())
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
            PlaybackRequest::Start(PlaybackResource::SpotifyUri(
                "spotify:track:5j6ZZwA9BnxZi5Bk0Ng4jB".to_string(),
            )),
            PlaybackRequest::Stop,
        ];
        let effects_expected = vec![
            Effects::PlaySpotify {
                spotify_uri: "spotify:track:5j6ZZwA9BnxZi5Bk0Ng4jB".to_string(),
            },
            Effects::StopSpotify,
        ];
        for req in playback_requests.iter() {
            player_handle.playback(req.clone()).unwrap();
        }
        let produced_effects: Vec<_> = effects_rx.iter().collect();

        assert_eq!(produced_effects, effects_expected);
        Ok(())
    }
}
