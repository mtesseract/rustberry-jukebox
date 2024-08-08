use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

use anyhow::Result;
use crossbeam_channel::Sender;
use serde::{Deserialize, Serialize};
use tracing::{error, info};

use crate::components::config::ConfigLoaderHandle;
use crate::components::rfid::Tag;
use crate::components::tag_mapper::{TagConf, TagMapperHandle};
use crate::effects::{Effect, InterpreterState};

pub use err::*;

#[derive(Debug, Clone)]
enum PlayerState {
    Idle,
    Playing {
        tag_conf: TagConf,
        playing_since: std::time::Instant,
        offset: Duration,
    },
    Paused {
        at: std::time::Duration,
        prev_tag_conf: TagConf,
    },
}

impl PlayerState {
    pub fn comparable(&self) -> ComparablePlayerState {
        match self {
            PlayerState::Idle => ComparablePlayerState::Idle,
            PlayerState::Playing {
                tag_conf,
                playing_since,
                offset,
                ..
            } => ComparablePlayerState::Playing {
                tag_conf: tag_conf.clone(),
                playing_since: *playing_since,
                offset: *offset,
            },
            PlayerState::Paused {
                at, prev_tag_conf, ..
            } => ComparablePlayerState::Paused {
                at: *at,
                prev_tag_conf: prev_tag_conf.clone(),
            },
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
enum ComparablePlayerState {
    Idle,
    Playing {
        tag_conf: TagConf,
        playing_since: std::time::Instant,
        offset: Duration,
    },
    Paused {
        at: std::time::Duration,
        prev_tag_conf: TagConf,
    },
}

pub struct Player {
    effect_tx: Sender<Effect>,
    state: PlayerState,
    config: ConfigLoaderHandle,
    tag_mapper: TagMapperHandle,
    interpreter_state: Arc<RwLock<InterpreterState>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum PlaybackRequest {
    Start(Tag),
    Stop,
}

pub type PlaybackResource = Tag;

impl Player {
    fn play_resource(&self, tag_conf: &TagConf) -> Result<()> {
        let effect = Effect::Play(tag_conf.clone());
        if let Err(err) = self.effect_tx.send(effect.clone()) {
            error!("Failed to send effect {:?}: {}", effect, err);
        }
        Ok(())
    }

    // fn playing_led(
    //     &self,
    //     is_playing: bool,
    // ) -> Result<()> {

    //    let effect =if is_playing {
    //     Effect::LedOn
    //     } else {
    //         Effect::LedOff
    //     };
    //     if let Err(err) = self.effect_tx.send(effect.clone()) {
    //         error!("Failed to send effect {:?}: {}", effect, err);
    //     }
    //     Ok(())
    // }

    // External entry point.
    pub fn pause_continue_command(&mut self) -> Result<()> {
        let state = self.state.clone();
        let res = self.handle_pause_continue_command();
        if let Err(err) = res {
            error!(
                "Player State Transition Failure: {}, staying in State {:?}",
                err, &state
            );
            return Err(err.into());
        } else if self.state.comparable() != state.comparable() {
            info!("Player State Transition: {:?} -> {:?}", self.state, state);
        }
        self.state = state;
        Ok(())
    }

    fn handle_pause_continue_command(&mut self) -> Result<()> {
        use PlayerState::*;

        match self.state.clone() {
            Idle => {}

            Paused { at, prev_tag_conf } => {
                if let Err(err) = self.effect_tx.send(Effect::PlayContinue(at)) {
                    error!("Failed to continue playback: {}", err);
                    return Err(err.into());
                }

                self.state = Playing {
                    playing_since: Instant::now(),
                    offset: at,
                    tag_conf: prev_tag_conf,
                };
            }

            Playing {
                playing_since,
                offset,
                tag_conf,
            } => {
                let interpreter_state = {
                    let r = *self.interpreter_state.read().unwrap();
                    r
                };
                let is_complete = !interpreter_state.currently_playing;

                if is_complete {
                    // playback finished already, event should trigger new playback.

                    if let Err(err) = self.effect_tx.send(Effect::Stop) {
                        error!("Failed to stop playback: {}", err);
                        return Err(err.into());
                    }

                    match self.play_resource(&tag_conf) {
                        Err(err) => {
                            error!("Failed to initiate new playback: {}", err);
                            self.state = Idle;
                            return Err(err);
                        }
                        Ok(_) => {
                            self.state = Playing {
                                playing_since: Instant::now(),
                                offset: Duration::from_secs(0),
                                tag_conf,
                            };
                        }
                    }
                } else {
                    let played_pos = offset + playing_since.elapsed();

                    if let Err(err) = self.effect_tx.send(Effect::Stop) {
                        error!("Failed to execute playback stop: {}", err);
                        self.state = Idle;
                        return Err(err.into());
                    }

                    self.state = Paused {
                        prev_tag_conf: tag_conf.clone(),
                        at: played_pos,
                    };
                }
            }
        }

        Ok(())
    }

    // External entry point.
    pub fn playback(&mut self, request: PlaybackRequest) -> Result<()> {
        let state = self.state.clone();
        let res = self.handle_playback_command(request);
        if let Err(err) = res {
            error!(
                "Player State Transition Failure: {}, staying in State {:?}",
                err, &state
            );
            return Err(err.into());
        } else if self.state.comparable() != state.comparable() {
            info!("Player State Transition: {:?} -> {:?}", self.state, state);
            // Self::playing_led(player.interpreter.clone(), state.is_playing());
        }
        self.state = state;
        Ok(())
    }

    fn handle_playback_command(&mut self, request: PlaybackRequest) -> Result<()> {
        let mut is_playing = false;
        use PlayerState::*;

        let config = self.config.get();
        let interpreter_state = self.interpreter_state.read().unwrap();
        let is_complete = !interpreter_state.currently_playing;

        info!(
            "Player in state {:?} received playback command {:?}",
            self.state, request
        );

        match request {
            PlaybackRequest::Start(tag) => {
                let tag_conf = self
                    .tag_mapper
                    .lookup(&tag.uid.to_string())
                    .unwrap_or_default();

                match self.state.clone() {
                    Idle => {
                        let offset = Duration::from_secs(0);
                        match self.play_resource(&tag_conf) {
                            Err(err) => {
                                error!("Failed to initiate new playback: {}", err);
                                return Err(err);
                            }
                            Ok(_) => {
                                self.state = Playing {
                                    playing_since: Instant::now(),
                                    offset,
                                    tag_conf,
                                };
                            }
                        }
                    }

                    Playing { .. } if !config.trigger_only_mode => {
                        // This code path should atually not happen.
                        // It means that the player has received two consecutive Playback-Start-Requests,
                        // i.e. without a Playback-Stop-Request in between. The main application logic should
                        // guarantee that this does not happen.
                        // Nevertheless we handle the case here inside the player: We keep it simple and update
                        // the playback.
                        let offset = Duration::from_secs(0);

                        // Stop current playback.
                        if let Err(err) = self.effect_tx.send(Effect::Stop) {
                            error!("Failed to stop playback: {}", err);
                            return Err(err.into());
                        }

                        match self.play_resource(&tag_conf) {
                            Err(err) => {
                                error!("Failed to initiate new playback: {}", err);
                                self.state = Idle;
                                return Err(err);
                            }
                            Ok(_) => {
                                self.state = Playing {
                                    playing_since: Instant::now(),
                                    offset,
                                    tag_conf,
                                };
                                is_playing = true;
                            }
                        }
                    }

                    Playing {
                        tag_conf: current_tag_conf,
                        ..
                    } if config.trigger_only_mode && current_tag_conf != tag_conf => {
                        // Different RFID tag presented, replace playback.

                        if let Err(err) = self.effect_tx.send(Effect::Stop) {
                            error!("Failed to stop playback: {}", err);
                            return Err(err.into());
                        }

                        match self.play_resource(&tag_conf) {
                            Err(err) => {
                                error!("Failed to initiate new playback: {}", err);
                                self.state = Idle;
                                return Err(err);
                            }
                            Ok(_) => {
                                self.state = Playing {
                                    playing_since: Instant::now(),
                                    offset: Duration::from_secs(0),
                                    tag_conf,
                                };
                                // is_playing = true;
                            }
                        }
                    }

                    Playing { .. } => {
                        // Same resource presented while playing already, trigger playback if already completed,
                        // otherwise do nothing.

                        if is_complete {
                            if let Err(err) = self.effect_tx.send(Effect::Stop) {
                                error!("Failed to stop playback: {}", err);
                                return Err(err.into());
                            }

                            match self.play_resource(&tag_conf) {
                                Err(err) => {
                                    error!("Failed to initiate new playback: {}", err);
                                    self.state = Idle;
                                    return Err(err);
                                }
                                Ok(_) => {
                                    self.state = Playing {
                                        playing_since: Instant::now(),
                                        offset: Duration::from_secs(0),
                                        tag_conf,
                                    };
                                }
                            }
                        }
                        is_playing = true;
                    }

                    Paused { at, prev_tag_conf } if tag_conf == prev_tag_conf => {
                        // Currently paused, last resource is presented again, continue playing.
                        if is_complete {
                            if let Err(err) = self.effect_tx.send(Effect::Stop) {
                                error!("Failed to stop playback: {}", err);
                                return Err(err.into());
                            }
                            match self.play_resource(&tag_conf) {
                                Err(err) => {
                                    error!("Failed to initiate new playback: {}", err);
                                    self.state = Idle;
                                    return Err(err);
                                }
                                Ok(_) => {
                                    self.state = Playing {
                                        playing_since: Instant::now(),
                                        offset: Duration::from_secs(0),
                                        tag_conf,
                                    };
                                }
                            }
                        } else {
                            info!(
                                "Same resource, not completed, continuing with pause state {:?}",
                                &at
                            );
                            if let Err(err) = self.effect_tx.send(Effect::Stop) {
                                error!("Failed to continue playback: {}", err);
                                self.state = Paused { at, prev_tag_conf };
                                return Err(err.into());
                            }
                            self.state = Playing {
                                playing_since: Instant::now(),
                                offset: at,
                                tag_conf,
                            };
                        }
                        is_playing = true;
                    }

                    Paused { at, prev_tag_conf } => {
                        // new resource
                        info!("New resource, playing from beginning");
                        if let Err(err) = self.effect_tx.send(Effect::Stop) {
                            error!("Failed to stop playback: {}", err);
                            self.state = Paused { at, prev_tag_conf };
                            return Err(err.into());
                        }

                        match self.play_resource(&tag_conf) {
                            Err(err) => {
                                error!("Failed to initiate new playback: {}", err);
                                self.state = Idle;
                                return Err(err);
                            }
                            Ok(_) => {
                                self.state = Playing {
                                    playing_since: Instant::now(),
                                    offset: Duration::from_secs(0),
                                    tag_conf,
                                };
                            }
                        }
                    }
                }
            }

            PlaybackRequest::Stop => {
                // RFID tag removed.

                match self.state.clone() {
                    Idle => {}

                    Paused { .. } => {}

                    Playing {
                        playing_since,
                        offset,
                        tag_conf,
                    } => {
                        if config.trigger_only_mode {
                            is_playing = true;
                        } else {
                            let played_pos = offset + playing_since.elapsed();

                            if let Err(err) = self.effect_tx.send(Effect::Stop) {
                                error!("Failed to execute playback pause: {}", err);
                                self.state = Idle;
                                return Err(err.into());
                            }

                            if is_complete {
                                self.state = Idle;
                            } else {
                                self.state = Paused {
                                    prev_tag_conf: tag_conf.clone(),
                                    at: played_pos,
                                };
                            }
                        }
                    }
                }
            }
        }

        Ok(())
    }

    // Creates a new Player object and returns a handle to it.
    pub fn new(
        effect_tx: Sender<Effect>,
        config: ConfigLoaderHandle,
        tag_mapper: TagMapperHandle,
        interpreter_state: Arc<RwLock<InterpreterState>>,
    ) -> Result<Player> {
        let player = Player {
            effect_tx,
            state: PlayerState::Idle,
            config,
            tag_mapper,
            interpreter_state,
        };
        Ok(player)
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

    impl<T> From<crossbeam_channel::SendError<T>> for Error {
        fn from(err: crossbeam_channel::SendError<T>) -> Self {
            Error::SendError(err.to_string())
        }
    }
    impl std::error::Error for Error {}
}
