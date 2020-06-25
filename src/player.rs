use std::fmt;
use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use failure::Fallible;
use serde::{Deserialize, Serialize};
use slog_scope::{error, info, warn};
use tokio::sync::mpsc::{channel, Receiver, Sender};

use crate::effects::{DynInterpreter, Interpreter};

pub use err::*;

#[async_trait]
pub trait PlaybackHandle {
    async fn stop(&self) -> Fallible<()>;
    async fn is_complete(&self) -> Fallible<bool>;
    async fn pause(&self) -> Fallible<()>;
    async fn cont(&self, pause_state: PauseState) -> Fallible<()>;
    async fn replay(&self) -> Fallible<()>;
}
#[derive(Debug, Clone)]
pub struct PauseState {
    pub pos: Duration,
}

#[derive(Debug, Clone)]
pub struct PlayerCommand {
    result_transmitter: Sender<Result<(), failure::Error>>,
    request: PlaybackRequest,
}

pub type DynPlaybackHandle = Box<dyn PlaybackHandle + Send + Sync + 'static>;

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
    },
    Paused {
        handle: Arc<DynPlaybackHandle>,
        at: std::time::Duration,
        prev_resource: PlaybackResource,
    },
}
pub struct Player {
    interpreter: Arc<DynInterpreter>,
    state: PlayerState,
    rx: Receiver<PlayerCommand>,
}

impl Drop for Player {
    fn drop(&mut self) {
        warn!("Dropping Player")
    }
}

impl Drop for PlayerHandle {
    fn drop(&mut self) {
        warn!("Dropping PlayerHandle")
    }
}

#[derive(Clone)]
pub struct PlayerHandle {
    tx: Sender<PlayerCommand>,
    guard: Arc<PlayerGuard>,
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

impl PlayerHandle {
    pub async fn playback(&self, req: PlaybackRequest) -> Fallible<()> {
        let (tx, mut rx) = channel(10);
        let mut xtx = self.tx.clone();
        xtx.send(PlayerCommand {
            result_transmitter: tx,
            request: req,
        })
        .await
        .unwrap();
        rx.recv().await.unwrap()
    }
}

impl Player {
    async fn play_resource(
        interpreter: Arc<Box<dyn Send + Sync + 'static + Interpreter>>,
        resource: &PlaybackResource,
        pause_state: Option<PauseState>,
    ) -> Result<Arc<DynPlaybackHandle>, failure::Error> {
        let interpreter = interpreter.clone();
        interpreter
            .play(resource.clone(), pause_state)
            .await
            .map(|x| Arc::new(x))
    }

    async fn state_machine(
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
                        match Self::play_resource(interpreter, &resource, None).await {
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
                        handle,
                        ..
                    } => {
                        // This code path should atually not happen.
                        // It means that the player has received two consecutive Playback-Start-Requests,
                        // i.e. without a Playback-Stop-Request in between. The main application logic should
                        // guarantee that this does not happen.
                        // Nevertheless we handle the case here inside the player: We keep it simple and update
                        // the playback.
                        let offset = Duration::from_secs(0);
                        if let Err(err) = handle.stop().await {
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
                            match Self::play_resource(interpreter, &resource, None).await {
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
                        if resource == prev_resource {
                            // continue at position
                            let pause_state = PauseState { pos: at };
                            info!(
                                "Same resource, not completed, continuing with pause state {:?}",
                                &pause_state
                            );
                            if let Err(err) = handle.cont(pause_state).await {
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
                            info!("New resource, playing from beginning");
                            if let Err(err) = handle.stop().await {
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
                                match Self::play_resource(interpreter, &resource, None).await {
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
                        let is_completed = handle.is_complete().await.unwrap_or(true);

                        let now = std::time::Instant::now();
                        let played_pos = offset + now.duration_since(playing_since);

                        if let Err(err) = handle.pause().await {
                            error!("Failed to execute playback pause: {}", err);
                            (Err(err), Idle)
                        } else {
                            if is_completed {
                                (Ok(()), Idle)
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
    }

    async fn player_loop(mut player: Player) {
        loop {
            info!("player loop");
            match player.rx.recv().await {
                None => {
                    break;
                }
                Some(command) => match command {
                    PlayerCommand {
                        result_transmitter,
                        request,
                    } => {
                        let mut result_transmitter = result_transmitter;
                        let current_state = player.state.clone();
                        let (res, new_state) =
                            Self::state_machine(player.interpreter.clone(), request, current_state)
                                .await;
                        if let Err(ref err) = res {
                            error!(
                                "Player State Transition Failure: {}, staying in State {}",
                                err, &player.state
                            );
                        } else {
                            info!("Player State Transition: {} -> {}", player.state, new_state);
                        }
                        player.state = new_state;
                        result_transmitter.send(res).await.unwrap();
                    }
                },
            }
        }
        warn!("Terminating Player Loop")
    }

    pub async fn new(
        interpreter: Arc<Box<dyn Send + Sync + 'static + Interpreter>>,
    ) -> Fallible<PlayerHandle> {
        let (tx, rx) = channel(1);

        let player = Player {
            interpreter,
            state: PlayerState::Idle,
            rx,
        };

        let (f, abort_handle) = futures::future::abortable(Self::player_loop(player));
        tokio::spawn(f);
        // tokio::time::delay_for(std::time::Duration::from_secs(0)).await; // FIXME: why is this necessary??

        let guard = Arc::new(PlayerGuard { abort_handle });
        let player_handle = PlayerHandle { tx, guard };

        Ok(player_handle)
    }
}

use futures::future::AbortHandle;

struct PlayerGuard {
    abort_handle: AbortHandle,
}

impl Drop for PlayerGuard {
    fn drop(&mut self) {
        info!("Dropping PlayerGuard, terminating Player task");
        self.abort_handle.abort();
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

    impl std::error::Error for Error {}
}

#[cfg(test)]
mod test {
    use failure::Fallible;
    use futures::stream::StreamExt;
    use slog::{self, o, Drain};
    use tokio::runtime::Runtime;

    use super::*;
    use crate::effects::{test::TestInterpreter, Effects};

    #[test]
    fn player_plays_resource_on_playback_request() -> Fallible<()> {
        let decorator = slog_term::TermDecorator::new().build();
        let drain = slog_term::FullFormat::new(decorator).build().fuse();
        let drain = slog_async::Async::new(drain).build().fuse();
        let logger = slog::Logger::root(drain, o!());
        let _guard = slog_scope::set_global_logger(logger);
        slog_scope::scope(&slog_scope::logger().new(o!()), || {
            let mut runtime = tokio::runtime::Builder::new()
                .threaded_scheduler()
                .enable_all()
                .build()?;
            runtime.block_on(async {
                let (interpreter, effects_rx) = TestInterpreter::new();
                let interpreter =
                    Arc::new(Box::new(interpreter) as Box<dyn Interpreter + Send + Sync + 'static>);
                let player_handle = Player::new(interpreter).await?;
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
                    player_handle.playback(req.clone()).await?;
                }
                tokio::time::delay_for(std::time::Duration::from_millis(100)).await;
                drop(player_handle);

                let produced_effects: Vec<_> = effects_rx.collect().await;

                assert_eq!(produced_effects, effects_expected);
                Ok(())
            })
        })
    }
}
