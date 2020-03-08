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
    led_controller: Box<dyn LedController + 'static + Send>,
    _config: Config,
}

impl ProdInterpreter {
    pub fn new(config: &Config) -> Fallible<Self> {
        let config = config.clone();
        let spotify_player = SpotifyPlayer::new(&config);
        let http_player = HttpPlayer::new();
        let led_controller = Box::new(led::gpio_cdev::GpioCdev::new()?);
        Ok(ProdInterpreter {
            spotify_player,
            http_player,
            led_controller,
            _config: config,
        })
    }

    fn handle(&self, effect: &Effects) -> Fallible<()> {
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
                self.http_player
                    .start_playback(&url, unimplemented!(), unimplemented!())?;
                Ok(())
            }
            Effects::StopHttp => {
                self.http_player.stop_playback()?;
                Ok(())
            }
            Effects::LedOn => self.led_controller.switch_on(Led::Playback),
            Effects::LedOff => self.led_controller.switch_off(Led::Playback),
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

    pub fn run(&self, channel: Receiver<Effects>) -> Fallible<()> {
        // FIXME
        for effect in channel.iter() {
            if let Err(err) = self.handle(&effect) {
                error!("Failed to execute effect {:?}: {}", effect, err);
            }
        }
        Ok(())
    }
}
