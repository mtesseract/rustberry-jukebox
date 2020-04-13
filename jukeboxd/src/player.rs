use std::cell::RefCell;
use std::fmt;
use std::sync::Arc;
use std::time::{Duration, Instant};

use failure::Fallible;
use replace_with::replace_with_and_return;
use serde::{Deserialize, Serialize};
use slog_scope::{error, info, warn};

use crate::effects::Interpreter;

pub use err::*;

pub trait PlaybackHandle {
    fn stop(&self) -> Fallible<()>;
    fn is_complete(&self) -> Fallible<bool>;
    fn pause(&self) -> Fallible<()>;
    fn cont(&self) -> Fallible<()>;
    fn replay(&self) -> Fallible<()>;
}
#[derive(Debug, Clone)]
pub struct PauseState {
    pub pos: Duration,
}

#[derive(Debug, Clone)]
pub enum PlayerCommand {
    PlaybackRequest(PlaybackRequest),
    Terminate,
}

// type StopPlayEffect = Box<dyn Fn() -> Result<(), failure::Error>>;
type DynPlaybackHandle = Box<dyn PlaybackHandle>;

impl fmt::Display for PlayerState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PlayerState::Idle => write!(f, "Idle"),
            PlayerState::Playing {
                resource, offset, ..
            } => write!(
                f,
                "Playing {{ resource = {:?}, offset = {:?} }}",
                resource, offset
            ),
            PlayerState::Paused {
                at, prev_resource, ..
            } => write!(
                f,
                "Paused {{ prev_resource = {:?}, at = {:?} }}",
                prev_resource, at
            ),
        }
    }
}

#[derive(Clone)]
enum PlayerState {
    Idle,
    Playing {
        resource: PlaybackResource,
        playing_since: std::time::Instant,
        offset: Duration,
        handle: Arc<DynPlaybackHandle>,
        // stop_eff: StopPlayEffect,
    },
    Paused {
        handle: Arc<DynPlaybackHandle>,
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

// FIXME
impl PlaybackHandle for () {
    fn stop(&self) -> Fallible<()> {
        unimplemented!()
    }
    fn is_complete(&self) -> Fallible<bool> {
        unimplemented!()
    }
    fn pause(&self) -> Fallible<()> {
        unimplemented!()
    }
    fn cont(&self) -> Fallible<()> {
        unimplemented!()
    }
    fn replay(&self) -> Fallible<()> {
        unimplemented!()
    }
}

impl Player {
    fn play_resource(
        interpreter: Arc<Box<dyn Send + Sync + 'static + Interpreter>>,
        resource: &PlaybackResource,
        pause_state: Option<PauseState>,
    ) -> Result<Arc<DynPlaybackHandle>, failure::Error> {
        match resource {
            PlaybackResource::SpotifyUri(ref spotify_uri) => {
                let interpreter = interpreter.clone();
                match interpreter.play_spotify(spotify_uri, pause_state) {
                    Err(err) => {
                        error!("Failed to play Spotify URI '{}': {}", spotify_uri, err);
                        Err(Error::Spotify(err).into())
                    }
                    Ok(handle) => Ok(Arc::new(Box::new(handle) as DynPlaybackHandle)),
                }
            }
            PlaybackResource::Http(ref url) => {
                let interpreter = interpreter.clone();
                match interpreter.play_http(url, pause_state) {
                    Err(err) => {
                        error!("Failed to play HTTP URI '{}': {}", url, err);
                        Err(Error::HTTP(err).into())
                    }
                    Ok(handle) => Ok(Arc::new(Box::new(handle) as DynPlaybackHandle)),
                }
            }
        }
    }

    fn state_machine(
        interpreter: Arc<Box<dyn Send + Sync + 'static + Interpreter>>,
        req: PlaybackRequest,
        state: PlayerState,
    ) -> (Result<(), failure::Error>, PlayerState) {
        use PlayerState::*;

        match req {
            self::PlaybackRequest::Start(resource) => {
                let playing_since = Instant::now();
                match state {
                    Idle => {
                        let offset = Duration::from_secs(0);
                        match Self::play_resource(interpreter, &resource, None) {
                            Ok(handle) => (
                                Ok(()),
                                Playing {
                                    playing_since,
                                    offset,
                                    handle,
                                    resource,
                                },
                            ),
                            Err(err) => (Err(err), Idle),
                        }
                    }
                    Playing {
                        resource: current_resource,
                        playing_since,
                        offset,
                        handle,
                    } => {
                        // This code path should atually not happen.
                        // It means that the player has received two consecutive Playback-Start-Requests,
                        // i.e. without a Playback-Stop-Request in between. The main application logic should
                        // guarantee that this does not happen.
                        // Nevertheless we handle the case here inside the player: We keep it simple and update
                        // the playback.
                        let offset = Duration::from_secs(0);
                        if let Err(err) = handle.stop() {
                            error!("Failed to stop playback: {}", err);
                            (
                                Err(err),
                                Playing {
                                    resource: current_resource,
                                    playing_since,
                                    offset,
                                    handle,
                                },
                            )
                        } else {
                            drop(handle);
                            match Self::play_resource(interpreter, &resource, None) {
                                Ok(handle) => (
                                    Ok(()),
                                    Playing {
                                        playing_since,
                                        handle,
                                        offset,
                                        resource,
                                    },
                                ),
                                Err(err) => {
                                    error!("Failed to initiate new playback: {}", err);
                                    (Err(err), Idle)
                                }
                            }
                        }
                    }

                    Paused {
                        handle,
                        at,
                        prev_resource,
                    } => {
                        if resource == prev_resource && handle.is_complete().unwrap_or(true) {
                            // start from beginning
                            if let Err(err) = handle.replay() {
                                error!("Failed to initiate replay: {}", err);
                                (
                                    Err(err),
                                    Paused {
                                        handle,
                                        at,
                                        prev_resource,
                                    },
                                )
                            } else {
                                (
                                    Ok(()),
                                    Playing {
                                        playing_since,
                                        offset: Duration::from_secs(0),
                                        handle,
                                        resource,
                                    },
                                )
                            }
                        } else if resource == prev_resource {
                            // continue at position
                            if let Err(err) = handle.cont() {
                                error!("Failed to continue playback: {}", err);
                                (
                                    Err(err),
                                    Paused {
                                        handle,
                                        at,
                                        prev_resource,
                                    },
                                )
                            } else {
                                (
                                    Ok(()),
                                    Playing {
                                        playing_since,
                                        offset: at,
                                        handle,
                                        resource,
                                    },
                                )
                            }
                        } else {
                            // new resource
                            if let Err(err) = handle.stop() {
                                error!("Failed to stop playback: {}", err);
                                (
                                    Err(err),
                                    Paused {
                                        handle,
                                        at,
                                        prev_resource,
                                    },
                                )
                            } else {
                                // drop(handle);
                                match Self::play_resource(interpreter, &resource, None) {
                                    Ok(handle) => (
                                        Ok(()),
                                        PlayerState::Playing {
                                            playing_since,
                                            handle,
                                            offset: Duration::from_secs(0),
                                            resource,
                                        },
                                    ),
                                    Err(err) => {
                                        error!("Failed to initiate new playback: {}", err);
                                        (Err(err), PlayerState::Idle)
                                    }
                                }
                            }
                        }
                    }
                }
            }
            self::PlaybackRequest::Stop => {
                match state {
                    Idle | Paused { .. } => {
                        // Unexpected code path.
                        error!("Player received Playback-Stop-Request while not playing");
                        (Ok(()), Idle)
                    }
                    Playing {
                        playing_since,
                        offset,
                        resource,
                        handle,
                    } => {
                        let now = std::time::Instant::now();
                        let played_pos = now.duration_since(playing_since);
                        if let Err(err) = handle.pause() {
                            error!("Failed to execute playback pause: {}", err);
                            (Err(err), Idle)
                        } else {
                            (
                                Ok(()),
                                Paused {
                                    prev_resource: resource.clone(),
                                    at: played_pos,
                                    handle,
                                },
                            )
                        }
                    }
                }
            }
        }
    }

    pub fn playback(&self, req: PlaybackRequest) -> Fallible<()> {
        let (tx, rx) = crossbeam_channel::bounded(1);
        let interpreter = Arc::clone(&self.interpreter);
        self.state.replace_with(move |state| {
            let current_state = state.clone();
            let (res, new_state) = Self::state_machine(interpreter, req, current_state);
            tx.send(res).unwrap();
            new_state
        });
        rx.recv().unwrap()
        //     &mut *state,
        //     || PlayerState::Idle,
        //     move |state| Self::state_machine(interpreter, req, state),
        // ) {
        //     error!("Failed to produce new state: {}", err);
        //     Err(err)
        // } else {
        //     Ok(())
        // }
        // let interpreter = Arc::clone(&self.interpreter);
        // let mut state = self.state.borrow_mut();
        // if let Err(err) = replace_with_and_return(
        //     &mut *state,
        //     || PlayerState::Idle,
        //     move |state| Self::state_machine(interpreter, req, state),
        // ) {
        //     error!("Failed to produce new state: {}", err);
        //     Err(err)
        // } else {
        //     Ok(())
        // }
        // let (res, new_state) = Self::state_machine(interpreter, req, *state);
        // *state = new_state;
        // if let Err(err) = res {
        //     error!("Failed to produce new state: {}", err);
        //     Err(err)
        // } else {
        //     Ok(())
        // }
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
