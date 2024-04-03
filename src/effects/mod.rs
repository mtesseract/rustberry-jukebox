pub mod file_player;
pub mod led;

use std::sync::Arc;

use crate::config::Config;
use anyhow::Result;
use async_trait::async_trait;
use file_player::FilePlayer;
use led::{Led, LedController};
use std::process::Command;
use tracing::{debug, info, warn};

use crate::components::tag_mapper::TagConf;
use crate::player::{DynPlaybackHandle, PauseState};

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
    fn generic_command(&self, cmd: &str) -> Result<()>;
}

#[async_trait]
impl Interpreter for ProdInterpreter {
    fn wait_until_ready(&self) -> Result<()> {
        Ok(())
    }

    async fn play(
        &self,
        tag_conf: TagConf,
        pause_state: Option<PauseState>,
    ) -> Result<DynPlaybackHandle> {
        self.file_player
            .start_playback(&tag_conf.uris, pause_state)
            .await
            .map(|x| Box::new(x) as DynPlaybackHandle)
    }

    fn led_on(&self) -> Result<()> {
        debug!("Switching LED on");
        self.led_controller.switch_on(Led::Playback)
    }
    fn led_off(&self) -> Result<()> {
        debug!("Switching LED off");
        self.led_controller.switch_off(Led::Playback)
    }
    fn generic_command(&self, cmd: &str) -> Result<()> {
        debug!("Executing command '{}'", &cmd);
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
