// use std::cell::RefCell;
use std::fmt;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Context,Result};
use async_trait::async_trait;
use crossbeam_channel::{Receiver, Sender};
use serde::{Deserialize, Serialize};
use slog_scope::{error, info};
use tokio::runtime;

use crate::components::rfid::Tag;
use crate::components::tag_mapper::{TagConf, TagMapperHandle};
use crate::effects::Interpreter;
use crate::led::Blinker;

pub use err::*;

#[async_trait]
pub trait PlaybackHandle {
    async fn stop(&self) -> Result<()>;
    async fn is_complete(&self) -> Result<bool>;
    async fn cont(&self, pause_state: PauseState) -> Result<()>;
    async fn replay(&self) -> Result<()>;
}
#[derive(Debug, Clone)]
pub struct PauseState {
    pub pos: Duration,
}

#[derive(Debug, Clone)]
pub enum PlayerCommand {
    PlaybackCommand {
        tx: Sender<Result<bool, anyhow::Error>>,
        request: PlaybackRequest,
    },
    PauseContinue {
        tx: Sender<Result<bool, anyhow::Error>>,
    },
}

// type StopPlayEffect = Box<dyn Fn() -> Result<(), anyhow::Error>>;
pub type DynPlaybackHandle = Box<dyn PlaybackHandle + Send + Sync + 'static>;

impl fmt::Display for PlayerState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PlayerState::Idle => write!(f, "Idle"),
            PlayerState::Playing {
                tag_conf, offset, ..
            } => write!(
                f,
                "Playing {{ tag_conf = {:?}, offset = {:?} }}",
                tag_conf, offset
            ),
            PlayerState::Paused {
                at, prev_tag_conf, ..
            } => write!(
                f,
                "Paused {{ prev_tag_conf = {:?}, at = {:?} }}",
                prev_tag_conf, at
            ),
        }
    }
}

#[derive(Clone)]
enum PlayerState {
    Idle,
    Playing {
        tag_conf: TagConf,
        playing_since: std::time::Instant,
        offset: Duration,
        handle: Arc<DynPlaybackHandle>,
    },
    Paused {
        handle: Arc<DynPlaybackHandle>,
        at: std::time::Duration,
        prev_tag_conf: TagConf,
    },
}

pub struct Player {
    interpreter: Arc<Box<dyn Send + Sync + 'static + Interpreter>>,
    blinker: Option<Blinker>,
    state: PlayerState,
    rx: Receiver<PlayerCommand>,
    config: Config,
    tag_mapper: TagMapperHandle,
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
    Start(Tag),
    Stop,
}

pub type PlaybackResource = Tag;

impl PlayerHandle {
    pub fn playback(&self, request: PlaybackRequest) -> Result<bool> {
        let (tx, rx) = crossbeam_channel::bounded(1);

        self.tx
            .send(PlayerCommand::PlaybackCommand { tx, request })
            .unwrap();
        rx.recv()?
    }

    pub fn pause_continue(&self) -> Result<bool> {
        let (tx, rx) = crossbeam_channel::bounded(1);

        self.tx.send(PlayerCommand::PauseContinue { tx }).unwrap();
        rx.recv()?
    }
}

impl Player {
    async fn play_resource(
        interpreter: Arc<Box<dyn Send + Sync + 'static + Interpreter>>,
        tag_conf: &TagConf,
        pause_state: Option<PauseState>,
    ) -> Result<Arc<DynPlaybackHandle>, anyhow::Error> {
        let interpreter = interpreter.clone();
        interpreter
            .play(tag_conf.clone(), pause_state)
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
        tx: Sender<Result<bool, anyhow::Error>>,
        state: &mut PlayerState,
        config: Arc<Config>,
        tag_mapper: &TagMapperHandle,
    ) -> Result<()> {
        let res = Self::handle_pause_continue_command(interpreter, state, config, tag_mapper).await;
        tx.send(res)?;
        Ok(())
    }

    async fn handle_pause_continue_command(
        interpreter: Arc<Box<dyn Send + Sync + 'static + Interpreter>>,
        state: &mut PlayerState,
        _config: Arc<Config>,
        _tag_mapper: &TagMapperHandle,
    ) -> Result<bool> {
        let mut is_playing = false;
        use PlayerState::*;

        match state.clone() {
            Idle => {}

            Paused {
                handle,
                at,
                prev_tag_conf,
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
                    tag_conf: prev_tag_conf,
                };
                is_playing = true;
            }

            Playing {
                playing_since,
                offset,
                tag_conf,
                handle,
            } => {
                if handle.is_complete().await.unwrap_or(true) {
                    // playback finished already, event should trigger new playback.

                    if let Err(err) = handle.stop().await {
                        error!("Failed to stop playback: {}", err);
                        return Err(err);
                    }

                    drop(handle);
                    match Self::play_resource(interpreter.clone(), &tag_conf, None).await {
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
                                tag_conf,
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
                        prev_tag_conf: tag_conf.clone(),
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
        tx: Sender<Result<bool, anyhow::Error>>,
        state: &mut PlayerState,
        config: Arc<Config>,
        tag_mapper: &TagMapperHandle,
    ) -> Result<()> {
        let res = Self::handle_playback_command(interpreter, request, state, config, tag_mapper).await;
        tx.send(res).context("Sending result of handle_playback_command")?;
        Ok(())
    }

    async fn handle_playback_command(
        interpreter: Arc<Box<dyn Send + Sync + 'static + Interpreter>>,
        request: PlaybackRequest,
        state: &mut PlayerState,
        config: Arc<Config>,
        tag_mapper: &TagMapperHandle,
    ) -> Result<bool> {
        let mut is_playing = false;
        use PlaybackRequest::*;
        use PlayerState::*;

        match request {
            Start(tag) => {
                let tag_conf = tag_mapper.lookup(&tag.uid.to_string()).unwrap_or_default();

                match state.clone() {
                    Idle => {
                        let offset = Duration::from_secs(0);
                        match Self::play_resource(interpreter.clone(), &tag_conf, None).await {
                            Err(err) => {
                                error!("Failed to initiate new playback: {}", err);
                                return Err(err);
                            }
                            Ok(handle) => {
                                *state = Playing {
                                    playing_since: Instant::now(),
                                    offset,
                                    handle,
                                    tag_conf,
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

                        match Self::play_resource(interpreter.clone(), &tag_conf, None).await {
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
                                    tag_conf,
                                };
                                is_playing = true;
                            }
                        }
                    }

                    Playing {
                        tag_conf: current_tag_conf,
                        handle,
                        ..
                    } if config.trigger_only_mode && current_tag_conf != tag_conf => {
                        // Different RFID tag presented, replace playback.

                        if let Err(err) = handle.stop().await {
                            error!("Failed to stop playback: {}", err);
                            return Err(err);
                        }

                        drop(handle);
                        match Self::play_resource(interpreter.clone(), &tag_conf, None).await {
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
                                    tag_conf,
                                };
                                is_playing = true;
                            }
                        }
                    }

                    Playing { handle, .. } => {
                        // Same resource presented while playing already, trigger playback if already completed,
                        // otherwise do nothing.

                        if handle.is_complete().await.unwrap_or(true) {
                            if let Err(err) = handle.stop().await {
                                error!("Failed to stop playback: {}", err);
                                return Err(err);
                            }

                            drop(handle);
                            match Self::play_resource(interpreter.clone(), &tag_conf, None).await {
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
                                        tag_conf,
                                    };
                                }
                            }
                        }
                        is_playing = true;
                    }

                    Paused {
                        handle,
                        at,
                        prev_tag_conf,
                    } if tag_conf == prev_tag_conf => {
                        // Currently paused, last resource is presented again, continue playing.
                        if handle.is_complete().await.unwrap_or(true) {
                            if let Err(err) = handle.stop().await {
                                error!("Failed to stop playback: {}", err);
                                return Err(err);
                            }
                            drop(handle);
                            match Self::play_resource(interpreter.clone(), &tag_conf, None).await {
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
                                        tag_conf,
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
                                    prev_tag_conf,
                                };
                                return Err(err);
                            }
                            *state = Playing {
                                playing_since: Instant::now(),
                                offset: at,
                                handle,
                                tag_conf,
                            };
                        }
                        is_playing = true;
                    }

                    Paused {
                        handle,
                        at,
                        prev_tag_conf,
                    } => {
                        // new resource
                        info!("New resource, playing from beginning");
                        if let Err(err) = handle.stop().await {
                            error!("Failed to stop playback: {}", err);
                            *state = Paused {
                                handle,
                                at,
                                prev_tag_conf,
                            };
                            return Err(err);
                        }

                        drop(handle);
                        match Self::play_resource(interpreter.clone(), &tag_conf, None).await {
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
                                    tag_conf,
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
                        tag_conf,
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
                                    prev_tag_conf: tag_conf.clone(),
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
        tag_mapper: &TagMapperHandle,
    ) -> Result<()> {
        use PlayerCommand::*;

        match cmd {
            PlaybackCommand { request, tx } => {
                Self::handle_playback_command_tx(interpreter, request, tx, state, config, tag_mapper).await
            }

            PlayerCommand::PauseContinue { tx } => {
                Self::handle_pause_continue_command_tx(interpreter, tx, state, config, tag_mapper).await
            }
        }
    }

    async fn player_loop(mut player: Player, tag_mapper: TagMapperHandle) {
        let config = Arc::new(player.config.clone());
        let tag_mapper_clone = tag_mapper.clone();
        loop {
            let command = player.rx.recv().unwrap();
            let mut state = player.state.clone();
            let res = Self::handle_command(
                player.interpreter.clone(),
                command,
                &mut state,
                config.clone(),
                &tag_mapper_clone,
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
        tag_mapper: TagMapperHandle,
    ) -> Result<PlayerHandle> {
        let (tx, rx) = crossbeam_channel::bounded(1);
        let tag_mapper_clone = tag_mapper.clone();
        let player = Player {
            blinker,
            interpreter,
            state: PlayerState::Idle,
            rx,
            config,
            tag_mapper,
        };

        runtime.spawn(Self::player_loop(player, tag_mapper_clone));

        let player_handle = PlayerHandle { tx };

        Ok(player_handle)
    }
}

pub mod err {
    use std::convert::From;
    use std::fmt::{self, Display};

    #[derive(Debug)]
    pub enum Error {
        Http(anyhow::Error),
        SendError(String),
    }

    impl Display for Error {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            match self {
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

// #[cfg(test)]
// mod test {
//     use anyhow::Result;
//     use tokio::runtime::Runtime;

//     use super::*;
//     use crate::effects::{test::TestInterpreter, Effects};
//     use crate::player;

//     #[test]
//     fn player_plays_resource_on_playback_request() -> Result<()> {
//         let runtime = runtime::Builder::new_multi_thread()
//             .enable_all()
//             .build()
//             .unwrap();
//         let (interpreter, effects_rx) = TestInterpreter::new();
//         let interpreter =
//             Arc::new(Box::new(interpreter) as Box<dyn Interpreter + Send + Sync + 'static>);
//         let player_handle = Player::new(
//             None,
//             &runtime.handle(),
//             interpreter,
//             player::Config::default(),
//         )
//         .unwrap();
//         let playback_requests = vec![
//             PlaybackRequest::Start(PlaybackResource::SpotifyUri(
//                 "spotify:track:5j6ZZwA9BnxZi5Bk0Ng4jB".to_string(),
//             )),
//             PlaybackRequest::Stop,
//         ];
//         let effects_expected = vec![
//             Effects::PlaySpotify {
//                 spotify_uri: "spotify:track:5j6ZZwA9BnxZi5Bk0Ng4jB".to_string(),
//             },
//             Effects::StopSpotify,
//         ];
//         for req in playback_requests.iter() {
//             player_handle.playback(req.clone()).unwrap();
//         }
//         let produced_effects: Vec<_> = effects_rx
//             .iter()
//             .filter(|x| x.is_spotify_effect())
//             .take(2)
//             .collect();

//         assert_eq!(produced_effects, effects_expected);
//         Ok(())
//     }
// }
