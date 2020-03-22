/*

input:
- user controls
- playback requests

effects:
- play via spotify, stop via spotify
- led on/off
- shutdown

*/

pub mod http_player;
pub mod led;
pub mod spotify;

use std::sync::Arc;

use crate::config::Config;
use crossbeam_channel::Receiver;
use failure::Fallible;
use http_player::HttpPlayer;
use led::{Led, LedController};
use slog_scope::{error, info, warn};
use spotify::player::SpotifyPlayer;
use std::process::Command;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effects {
    PlayHttp { url: String },
    StopHttp,
    PlaySpotify { spotify_uri: String },
    StopSpotify,
    LedOn,
    LedOff,
    GenericCommand(String),
}

pub struct ProdInterpreter {
    spotify_player: SpotifyPlayer,
    http_player: HttpPlayer,
    led_controller: Arc<Box<dyn LedController + 'static + Send + Sync>>,
    _config: Config,
}

impl ProdInterpreter {
    pub fn new(config: &Config) -> Fallible<Self> {
        let config = config.clone();
        let led_controller = Arc::new(Box::new(led::gpio_cdev::GpioCdev::new()?)
            as Box<dyn LedController + 'static + Send + Sync>);
        let spotify_player = SpotifyPlayer::new(&config, Arc::clone(&led_controller))?;
        let http_player = HttpPlayer::new(Some(Arc::clone(&led_controller)))?;
        Ok(ProdInterpreter {
            spotify_player,
            http_player,
            led_controller,
            _config: config,
        })
    }

    pub fn wait_until_ready(&self) -> Fallible<()> {
        self.spotify_player.wait_until_ready()?;
        Ok(())
    }

    fn handle(&mut self, effect: &Effects) -> Fallible<()> {
        match effect {
            Effects::PlaySpotify { spotify_uri } => {
                self.spotify_player.start_playback(&spotify_uri)?;
                Ok(())
            }
            Effects::StopSpotify => {
                self.spotify_player.stop_playback()?;
                Ok(())
            }
            Effects::PlayHttp { url } => {
                self.http_player.start_playback(&url)?;
                Ok(())
            }
            Effects::StopHttp => {
                self.http_player.stop_playback()?;
                Ok(())
            }
            Effects::LedOn => {
                info!("Switching LED on");
                self.led_controller.switch_on(Led::Playback)
            }
            Effects::LedOff => {
                info!("Switching LED off");
                self.led_controller.switch_off(Led::Playback)
            }
            Effects::GenericCommand(cmd) => {
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
                            Err(failure::err_msg(format!(
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
    }

    pub fn run(&mut self, channel: Receiver<Effects>) -> Fallible<()> {
        // FIXME
        for effect in channel.iter() {
            if let Err(err) = self.handle(&effect) {
                error!("Failed to execute effect {:?}: {}", effect, err);
            }
        }
        Ok(())
    }
}
