pub mod file_player;
pub mod led;

use std::sync::{Arc, RwLock};

use crate::components::config::ConfigLoaderHandle;
use anyhow::Result;
use file_player::FilePlayer;
use led::{Led, LedController};
use std::process::Command;
use tracing::{debug, info, warn};

use crate::components::tag_mapper::TagConf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    Play(TagConf),
    PlayContinue(std::time::Duration),
    Stop,
    LedOn,
    LedOff,
    GenericCommand(String),
}

#[derive(Debug, Clone, Copy)]
pub struct InterpreterState {
    pub currently_playing: bool,
}

impl InterpreterState {
    pub fn new() -> Self {
        InterpreterState {
            currently_playing: false,
        }
    }
}

pub struct ProdInterpreter {
    file_player: FilePlayer,
    led_controller: Arc<Box<dyn LedController + 'static + Send + Sync>>,
    pause_state: std::time::Duration,
    pub interpreter_state: Arc<RwLock<InterpreterState>>,
}

pub trait Interpreter {
    fn wait_until_ready(&self) -> Result<()>;
    fn interprete(&mut self, eff: Effect) -> Result<()>;
}

impl Interpreter for ProdInterpreter {
    fn wait_until_ready(&self) -> Result<()> {
        Ok(())
    }

    fn interprete(&mut self, eff: Effect) -> Result<()> {
        match eff {
            Effect::GenericCommand(cmd) => self.generic_command(&cmd),
            Effect::LedOn => self.led_on(),
            Effect::LedOff => self.led_off(),
            Effect::Play(tag_conf) => self.play(tag_conf),
            Effect::Stop => self.stop(),
            Effect::PlayContinue(_) => self.play_continue(),
        }
    }
}

impl ProdInterpreter {
    pub fn new(config_loader: ConfigLoaderHandle, interpreter_state: Arc<RwLock<InterpreterState>>) -> Result<Self> {
        info!("Creating production interpreter");
        let led_controller = Arc::new(Box::new(led::gpio_cdev::GpioCdev::new()?)
            as Box<dyn LedController + 'static + Send + Sync>);
        let file_player = FilePlayer::new(config_loader)?;
        let interpreter_state_copy = interpreter_state.clone();
        let sink = file_player.sink.clone();
        tokio::task::spawn_blocking(move || loop {
            {
                let mut state = interpreter_state_copy.write().unwrap();
                state.currently_playing = !sink.empty();
            }
            std::thread::sleep(std::time::Duration::from_secs(2));
        });
        Ok(ProdInterpreter {
            file_player,
            led_controller,
            pause_state: std::time::Duration::from_secs(0),
            interpreter_state,
        })
    }

    //
    // Effect implementations.
    //

    fn play_continue(&mut self) -> Result<()> {
        debug!("Interpreter: play/continue");
        self.file_player.cont()
    }

    fn play(&mut self, tag_conf: TagConf) -> Result<()> {
        debug!("Interpreter: play");
        self.file_player
            .start_playback(&tag_conf.uris, Some(self.pause_state))
    }

    fn stop(&self) -> Result<()> {
        debug!("Interpreter: stop");
        self.file_player.stop()
    }

    fn led_on(&self) -> Result<()> {
        debug!("Interpreter: LED on");
        self.led_controller.switch_on(Led::Playback)
    }

    fn led_off(&self) -> Result<()> {
        debug!("Interpreter: LED off");
        self.led_controller.switch_off(Led::Playback)
    }

    pub fn generic_command(&self, cmd: &str) -> Result<()> {
        debug!("Interpreter: Executing command '{}'", &cmd);
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
