// use std::cell::RefCell;
use std::fmt;
use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use crossbeam_channel::{Receiver, Sender};
use failure::Fallible;
use serde::{Deserialize, Serialize};
use slog_scope::{error, info};
use tokio::runtime;

use crate::effects::Interpreter;
use crate::led::Blinker;

pub use err::*;

#[async_trait]
pub trait PlaybackHandle {
    async fn stop(&self) -> Fallible<()>;
    async fn is_complete(&self) -> Fallible<bool>;
    async fn cont(&self, pause_state: PauseState) -> Fallible<()>;
    async fn replay(&self) -> Fallible<()>;
}
#[derive(Debug, Clone)]
pub struct PauseState {
    pub pos: Duration,
}

#[derive(Debug, Clone)]
pub enum PlayerCommand {
    PlaybackCommand {
        tx: Sender<Result<bool, failure::Error>>,
        request: PlaybackRequest,
    },
    PauseContinue {
        tx: Sender<Result<bool, failure::Error>>,
    },
}

// type StopPlayEffect = Box<dyn Fn() -> Result<(), failure::Error>>;
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
    interpreter: Arc<Box<dyn Send + Sync + 'static + Interpreter>>,
    blinker: Option<Blinker>,
    state: PlayerState,
    rx: Receiver<PlayerCommand>,
    config: Config,
}

#[derive(Debug, Clone)]
pub struct Config {
    pub trigger_only_mode: bool,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            trigger_only_mode: false,
        }
    }
}

pub struct PlayerHandle {
    tx: Sender<PlayerCommand>,
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
    pub fn playback(&self, request: PlaybackRequest) -> Fallible<bool> {
        let (tx, rx) = crossbeam_channel::bounded(1);

        self.tx
            .send(PlayerCommand::PlaybackCommand { tx, request })
            .unwrap();
        rx.recv()?
    }

    pub fn pause_continue(&self) -> Fallible<bool> {
        let (tx, rx) = crossbeam_channel::bounded(1);

        self.tx.send(PlayerCommand::PauseContinue { tx }).unwrap();
        rx.recv()?
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
            .map(Arc::new)
    }

    fn playing_led(
        interpreter: Arc<Box<dyn Send + Sync + 'static + Interpreter>>,
        is_playing: bool,
    ) {
        let res = if is_playing {
            interpreter.led_on()
        } else {
            interpreter.led_off()
        };
        if let Err(err) = res {
            error!(
                "Failed to switch LED {}: {}",
                if is_playing { "on" } else { "off" },
                err
            );
        }
    }

    async fn handle_pause_continue_command_tx(
        interpreter: Arc<Box<dyn Send + Sync + 'static + Interpreter>>,
        tx: Sender<Result<bool, failure::Error>>,
        state: &mut PlayerState,
        config: Arc<Config>,
    ) -> Fallible<()> {
        let res = Self::handle_pause_continue_command(interpreter, state, config).await;
        tx.send(res)?;
        Ok(())
    }

    async fn handle_pause_continue_command(
        interpreter: Arc<Box<dyn Send + Sync + 'static + Interpreter>>,
        state: &mut PlayerState,
        _config: Arc<Config>,
    ) -> Fallible<bool> {
        let mut is_playing = false;
        use PlayerState::*;

        match state.clone() {
            Idle => {}

            Paused {
                handle,
                at,
                prev_resource,
            } => {
                let pause_state = PauseState { pos: at };

                if let Err(err) = handle.cont(pause_state).await {
                    error!("Failed to continue playback: {}", err);
                    return Err(err);
                }

                *state = Playing {
                    playing_since: Instant::now(),
                    offset: at,
                    handle,
                    resource: prev_resource,
                };
                is_playing = true;
            }

            Playing {
                playing_since,
                offset,
                resource,
                handle,
            } => {
                if handle.is_complete().await.unwrap_or(true) {
                    // playback finished already, event should trigger new playback.

                    if let Err(err) = handle.stop().await {
                        error!("Failed to stop playback: {}", err);
                        return Err(err);
                    }

                    drop(handle);
                    match Self::play_resource(interpreter.clone(), &resource, None).await {
                        Err(err) => {
                            error!("Failed to initiate new playback: {}", err);
                            *state = Idle;
                            return Err(err);
                        }
                        Ok(handle) => {
                            *state = Playing {
                                playing_since: Instant::now(),
                                handle,
                                offset: Duration::from_secs(0),
                                resource,
                            };
                            is_playing = true;
                        }
                    }
                } else {
                    let played_pos = offset + playing_since.elapsed();

                    if let Err(err) = handle.stop().await {
                        error!("Failed to execute playback stop: {}", err);
                        *state = Idle;
                        return Err(err);
                    }

                    *state = Paused {
                        prev_resource: resource.clone(),
                        at: played_pos,
                        handle,
                    };
                }
            }
        }

        Self::playing_led(interpreter, is_playing);
        Ok(is_playing)
    }

    async fn handle_playback_command_tx(
        interpreter: Arc<Box<dyn Send + Sync + 'static + Interpreter>>,
        request: PlaybackRequest,
        tx: Sender<Result<bool, failure::Error>>,
        state: &mut PlayerState,
        config: Arc<Config>,
    ) -> Fallible<()> {
        let res = Self::handle_playback_command(interpreter, request, state, config).await;
        tx.send(res)?;
        Ok(())
    }

    async fn handle_playback_command(
        interpreter: Arc<Box<dyn Send + Sync + 'static + Interpreter>>,
        request: PlaybackRequest,
        state: &mut PlayerState,
        config: Arc<Config>,
    ) -> Fallible<bool> {
        let mut is_playing = false;
        use PlaybackRequest::*;
        use PlayerState::*;

        match request {
            Start(resource) => {
                match state.clone() {
                    Idle => {
                        let offset = Duration::from_secs(0);
                        match Self::play_resource(interpreter.clone(), &resource, None).await {
                            Err(err) => {
                                error!("Failed to initiate new playback: {}", err);
                                return Err(err);
                            }
                            Ok(handle) => {
                                *state = Playing {
                                    playing_since: Instant::now(),
                                    offset,
                                    handle,
                                    resource,
                                };
                                is_playing = true;
                            }
                        }
                    }

                    Playing { handle, .. } if !config.trigger_only_mode => {
                        // This code path should atually not happen.
                        // It means that the player has received two consecutive Playback-Start-Requests,
                        // i.e. without a Playback-Stop-Request in between. The main application logic should
                        // guarantee that this does not happen.
                        // Nevertheless we handle the case here inside the player: We keep it simple and update
                        // the playback.
                        let offset = Duration::from_secs(0);

                        // Stop current playback.
                        if let Err(err) = handle.stop().await {
                            error!("Failed to stop playback: {}", err);
                            return Err(err);
                        }
                        drop(handle);

                        match Self::play_resource(interpreter.clone(), &resource, None).await {
                            Err(err) => {
                                error!("Failed to initiate new playback: {}", err);
                                *state = Idle;
                                return Err(err);
                            }
                            Ok(handle) => {
                                *state = Playing {
                                    playing_since: Instant::now(),
                                    handle,
                                    offset,
                                    resource,
                                };
                                is_playing = true;
                            }
                        }
                    }

                    Playing {
                        resource: current_resource,
                        handle,
                        ..
                    } if config.trigger_only_mode && current_resource != resource => {
                        // Different RFID tag presented, replace playback.

                        if let Err(err) = handle.stop().await {
                            error!("Failed to stop playback: {}", err);
                            return Err(err);
                        }

                        drop(handle);
                        match Self::play_resource(interpreter.clone(), &resource, None).await {
                            Err(err) => {
                                error!("Failed to initiate new playback: {}", err);
                                *state = Idle;
                                return Err(err);
                            }
                            Ok(handle) => {
                                *state = Playing {
                                    playing_since: Instant::now(),
                                    handle,
                                    offset: Duration::from_secs(0),
                                    resource,
                                };
                                is_playing = true;
                            }
                        }
                    }

                    Playing { handle, .. } => {
                        // Same resource presented while playing already, trigger playback if already completed,
                        // otherwise do nothing.unimplemented!

                        if handle.is_complete().await.unwrap_or(true) {
                            if let Err(err) = handle.stop().await {
                                error!("Failed to stop playback: {}", err);
                                return Err(err);
                            }

                            drop(handle);
                            match Self::play_resource(interpreter.clone(), &resource, None).await {
                                Err(err) => {
                                    error!("Failed to initiate new playback: {}", err);
                                    *state = Idle;
                                    return Err(err);
                                }
                                Ok(handle) => {
                                    *state = Playing {
                                        playing_since: Instant::now(),
                                        handle,
                                        offset: Duration::from_secs(0),
                                        resource,
                                    };
                                }
                            }
                        }
                        is_playing = true;
                    }

                    Paused {
                        handle,
                        at,
                        prev_resource,
                    } if resource == prev_resource => {
                        // Currently paused, last resource is presented again, continue playing.
                        if handle.is_complete().await.unwrap_or(true) {
                            if let Err(err) = handle.stop().await {
                                error!("Failed to stop playback: {}", err);
                                return Err(err);
                            }
                            drop(handle);
                            match Self::play_resource(interpreter.clone(), &resource, None).await {
                                Err(err) => {
                                    error!("Failed to initiate new playback: {}", err);
                                    *state = Idle;
                                    return Err(err);
                                }
                                Ok(handle) => {
                                    *state = Playing {
                                        playing_since: Instant::now(),
                                        handle,
                                        offset: Duration::from_secs(0),
                                        resource,
                                    };
                                }
                            }
                        } else {
                            let pause_state = PauseState { pos: at };
                            info!(
                                "Same resource, not completed, continuing with pause state {:?}",
                                &pause_state
                            );
                            if let Err(err) = handle.cont(pause_state).await {
                                error!("Failed to continue playback: {}", err);
                                *state = Paused {
                                    handle,
                                    at,
                                    prev_resource,
                                };
                                return Err(err);
                            }
                            *state = Playing {
                                playing_since: Instant::now(),
                                offset: at,
                                handle,
                                resource,
                            };
                        }
                        is_playing = true;
                    }

                    Paused {
                        handle,
                        at,
                        prev_resource,
                    } => {
                        // new resource
                        info!("New resource, playing from beginning");
                        if let Err(err) = handle.stop().await {
                            error!("Failed to stop playback: {}", err);
                            *state = Paused {
                                handle,
                                at,
                                prev_resource,
                            };
                            return Err(err);
                        }

                        drop(handle);
                        match Self::play_resource(interpreter.clone(), &resource, None).await {
                            Err(err) => {
                                error!("Failed to initiate new playback: {}", err);
                                *state = Idle;
                                return Err(err);
                            }
                            Ok(handle) => {
                                *state = Playing {
                                    playing_since: Instant::now(),
                                    handle,
                                    offset: Duration::from_secs(0),
                                    resource,
                                };
                                is_playing = true;
                            }
                        }
                    }
                }
            }

            Stop => {
                // RFID tag removed.

                match state.clone() {
                    Idle => {}

                    Paused { .. } => {}

                    Playing {
                        playing_since,
                        offset,
                        resource,
                        handle,
                    } => {
                        if config.trigger_only_mode {
                            is_playing = true;
                        } else {
                            let is_completed = handle.is_complete().await.unwrap_or(true);
                            let played_pos = offset + playing_since.elapsed();

                            if let Err(err) = handle.stop().await {
                                error!("Failed to execute playback pause: {}", err);
                                *state = Idle;
                                return Err(err);
                            }

                            if is_completed {
                                *state = Idle;
                            } else {
                                *state = Paused {
                                    prev_resource: resource.clone(),
                                    at: played_pos,
                                    handle,
                                };
                            }
                        }
                    }
                }
            }
        }

        Self::playing_led(interpreter, is_playing);
        Ok(is_playing)
    }

    async fn handle_command(
        interpreter: Arc<Box<dyn Send + Sync + 'static + Interpreter>>,
        cmd: PlayerCommand,
        state: &mut PlayerState,
        config: Arc<Config>,
    ) -> Fallible<()> {
        use PlayerCommand::*;

        match cmd {
            PlaybackCommand { request, tx } => {
                Self::handle_playback_command_tx(interpreter, request, tx, state, config).await
            }

            PlayerCommand::PauseContinue { tx } => {
                Self::handle_pause_continue_command_tx(interpreter, tx, state, config).await
            }
        }
    }

    async fn player_loop(mut player: Player) {
        let config = Arc::new(player.config.clone());
        loop {
            let command = player.rx.recv().unwrap();
            let mut state = player.state.clone();
            let res = Self::handle_command(
                player.interpreter.clone(),
                command,
                &mut state,
                config.clone(),
            )
            .await;
            if let Err(ref err) = res {
                error!(
                    "Player State Transition Failure: {}, staying in State {}",
                    err, &state
                );
            } else {
                info!("Player State Transition: {} -> {}", player.state, state);
            }
            player.state = state;
        }
    }

    pub fn new(
        blinker: Option<Blinker>,
        runtime: &runtime::Handle,
        interpreter: Arc<Box<dyn Send + Sync + 'static + Interpreter>>,
        config: Config,
    ) -> Fallible<PlayerHandle> {
        let (tx, rx) = crossbeam_channel::bounded(1);

        let player = Player {
            blinker,
            interpreter,
            state: PlayerState::Idle,
            rx,
            config,
        };

        runtime.spawn(Self::player_loop(player));

        let player_handle = PlayerHandle { tx };

        Ok(player_handle)
    }
}

pub mod err {
    use std::convert::From;
    use std::fmt::{self, Display};

    #[derive(Debug)]
    pub enum Error {
        Spotify(failure::Error),
        Http(failure::Error),
        SendError(String),
    }

    impl Display for Error {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            match self {
                Error::Spotify(err) => write!(f, "Spotify Error {}", err),
                Error::Http(err) => write!(f, "HTTP Error {}", err),
                Error::SendError(err) => {
                    write!(f, "Failed to transmit command via channel: {}", err)
                }
            }
        }
    }

    impl From<reqwest::Error> for Error {
        fn from(err: reqwest::Error) -> Self {
            Error::Http(err.into())
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
    use tokio::runtime::Runtime;

    use super::*;
    use crate::effects::{test::TestInterpreter, Effects};
    use crate::player;

    #[test]
    fn player_plays_resource_on_playback_request() -> Fallible<()> {
        let runtime = runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();
        let (interpreter, effects_rx) = TestInterpreter::new();
        let interpreter =
            Arc::new(Box::new(interpreter) as Box<dyn Interpreter + Send + Sync + 'static>);
        let player_handle = Player::new(
            None,
            &runtime.handle(),
            interpreter,
            player::Config::default(),
        )
        .unwrap();
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
        let produced_effects: Vec<_> = effects_rx
            .iter()
            .filter(|x| x.is_spotify_effect())
            .take(2)
            .collect();

        assert_eq!(produced_effects, effects_expected);
        Ok(())
    }
}
