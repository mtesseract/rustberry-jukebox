pub mod file_player;
pub mod led;

use std::sync::Arc;

use crate::config::Config;
use anyhow::Result;
use async_trait::async_trait;
use file_player::FilePlayer;
use led::{Led, LedController};
use tracing::{info, warn};
use std::process::Command;

use crate::player::{DynPlaybackHandle, PauseState};
use crate::components::tag_mapper::TagConf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effects {
    Play { uri: String },
    Stop,
    LedOn,
    LedOff,
    GenericCommand(String),
}

pub struct ProdInterpreter {
    file_player: FilePlayer,
    led_controller: Arc<Box<dyn LedController + 'static + Send + Sync>>,
    _config: Config,
}

#[async_trait]

pub trait Interpreter {
    fn wait_until_ready(&self) -> Result<()>;
    async fn play(
        &self,
        tag_conf: TagConf,
        pause_state: Option<PauseState>,
    ) -> Result<DynPlaybackHandle>;
    fn led_on(&self) -> Result<()>;
    fn led_off(&self) -> Result<()>;
    fn generic_command(&self, cmd: String) -> Result<()>;
}

#[async_trait]
impl Interpreter for ProdInterpreter {
    fn wait_until_ready(&self) -> Result<()> {
        Ok(())
    }

    async fn play( &self, tag_conf: TagConf, pause_state: Option<PauseState>) -> Result<DynPlaybackHandle> {
            self
                .file_player
                .start_playback(&tag_conf.uris, pause_state)
                .await
                .map(|x| Box::new(x) as DynPlaybackHandle)
    }

    fn led_on(&self) -> Result<()> {
        info!("Switching LED on");
        self.led_controller.switch_on(Led::Playback)
    }
    fn led_off(&self) -> Result<()> {
        info!("Switching LED off");
        self.led_controller.switch_off(Led::Playback)
    }
    fn generic_command(&self, cmd: String) -> Result<()> {
        info!("Executing command '{}'", &cmd);
        let res = Command::new("/bin/sh").arg("-c").arg(&cmd).status();
        match res {
            Ok(exit_status) => {
                if exit_status.success() {
                    info!("Command succeeded");
                    Ok(())
                } else {
                    warn!(
                        "Command terminated with non-zero exit code: {:?}",
                        exit_status
                    );
                    Err(anyhow::Error::msg(format!(
                        "Command terminated with exit status {}",
                        exit_status
                    )))
                }
            }
            Err(err) => {
                warn!("Failed to execute command: {}", err);
                Err(err.into())
            }
        }
    }
}

impl ProdInterpreter {
    pub fn new(config: &Config) -> Result<Self> {
        let config = config.clone();
        let led_controller = Arc::new(Box::new(led::gpio_cdev::GpioCdev::new()?)
            as Box<dyn LedController + 'static + Send + Sync>);
        let file_player = FilePlayer::new(&config.audio_base_directory)?;
        Ok(ProdInterpreter {
            file_player,
            led_controller,
            _config: config,
        })
    }
}

// pub mod test {
//     use super::*;
//     use async_trait::async_trait;
//     use crossbeam_channel::{self, Receiver, Sender};
//     use Effects::*;

//     pub struct TestInterpreter {
//         tx: Sender<Effects>,
//     }

//     impl TestInterpreter {
//         pub fn new() -> (TestInterpreter, Receiver<Effects>) {
//             let (tx, rx) = crossbeam_channel::unbounded();
//             let interpreter = TestInterpreter { tx };
//             (interpreter, rx)
//         }
//     }

//     struct DummyPlaybackHandle;

//     #[async_trait]
//     impl PlaybackHandle for DummyPlaybackHandle {
//         async fn stop(&self) -> Result<()> {
//             Ok(())
//         }
//         async fn is_complete(&self) -> Result<bool> {
//             Ok(true)
//         }
//         async fn cont(&self, _pause_state: PauseState) -> Result<()> {
//             Ok(())
//         }
//         async fn replay(&self) -> Result<()> {
//             Ok(())
//         }
//     }

//     #[async_trait]
//     impl Interpreter for TestInterpreter {
//         fn wait_until_ready(&self) -> Result<()> {
//             Ok(())
//         }

//         async fn play(
//             &self,
//             res: PlaybackResource,
//             _pause_state: Option<PauseState>,
//         ) -> Result<DynPlaybackHandle> {
//             use PlaybackResource::*;

//             self.tx.send(Play { uri: res.uid.0 })?;
//             Ok(Box::new(DummyPlaybackHandle) as DynPlaybackHandle)
//         }

//         fn led_on(&self) -> Result<()> {
//             self.tx.send(LedOn).unwrap();
//             Ok(())
//         }
//         fn led_off(&self) -> Result<()> {
//             self.tx.send(LedOff).unwrap();
//             Ok(())
//         }
//         fn generic_command(&self, cmd: String) -> Result<()> {
//             self.tx.send(GenericCommand(cmd)).unwrap();
//             Ok(())
//         }
//     }
// }
