pub mod file_player;
pub mod led;

use std::sync::{Mutex, Arc};

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

pub struct InterpreterState {
    pub currently_playing: bool
}

pub struct ProdInterpreter {
    file_player: FilePlayer,
    led_controller: Arc<Box<dyn LedController + 'static + Send + Sync>>,
    pause_state: std::time::Duration,
    pub interpreter_state: Arc<Mutex<InterpreterState>>,
}

pub trait Interpreter {
    fn wait_until_ready(&self) -> Result<()>;
    fn interprete(&self, eff: Effect) -> Result<()>;
}

impl Interpreter for ProdInterpreter {
    fn wait_until_ready(&self) -> Result<()> {
        Ok(())
    }

    fn interprete(&self, eff: Effect) -> Result<()> {
        match eff {
            Effect::GenericCommand(cmd) => self.generic_command(&cmd),
            Effect::LedOn => self.led_on(),
            Effect::LedOff => self.led_off(),
            Effect::Play(tag_conf) => self.play(tag_conf),
            Effect::Stop => self.stop(),
            Effect::PlayContinue() => self.
        }
    }
}

impl ProdInterpreter {
    pub fn new(config_loader: ConfigLoaderHandle) -> Result<Self> {
        let led_controller = Arc::new(Box::new(led::gpio_cdev::GpioCdev::new()?)
            as Box<dyn LedController + 'static + Send + Sync>);
        let file_player = FilePlayer::new(config_loader)?;
        let interpreter_state = Arc::new(Mutex::new(InterpreterState { currently_playing: false }));
        let interpreter_state_copy = interpreter_state.clone();
        let sink = file_player.sink.clone();
        tokio::task::spawn_blocking(move || {
            loop {
                {
                    let mut state = interpreter_state_copy.lock().unwrap();
                    state.currently_playing = !sink.empty();
                }
                std::thread::sleep(std::time::Duration::from_secs(2));
            }
        });
        Ok(ProdInterpreter {
            file_player,
            led_controller,
            pause_state: std::time::Duration::from_secs(0),
            interpreter_state,
        })
    }

    // Effect implementations.

    fn play(&mut self, tag_conf: TagConf) -> Result<()> {
        self.file_player.start_playback(&tag_conf.uris, self.pause_state)
    }
    
    fn stop(&self) -> Result<()> {
        self.file_player.stop()
    }

    fn led_on(&self) -> Result<()> {
        debug!("Switching LED on");
        self.led_controller.switch_on(Led::Playback)
    }

    fn led_off(&self) -> Result<()> {
        debug!("Switching LED off");
        self.led_controller.switch_off(Led::Playback)
    }

    pub fn generic_command(&self, cmd: &str) -> Result<()> {
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
