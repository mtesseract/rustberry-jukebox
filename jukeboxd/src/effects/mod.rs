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
use failure::Fallible;
use http_player::HttpPlayer;
use led::{Led, LedController};
use slog_scope::{info, warn};
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

pub trait Interpreter {
    fn wait_until_ready(&self) -> Fallible<()>;
    fn play_http(&self, url: &str) -> Fallible<()>;
    fn stop_http(&self) -> Fallible<()>;
    fn play_spotify(&self, spotify_uri: &str) -> Fallible<()>;
    fn stop_spotify(&self) -> Fallible<()>;
    fn led_on(&self) -> Fallible<()>;
    fn led_off(&self) -> Fallible<()>;
    fn generic_command(&self, cmd: String) -> Fallible<()>;
}

impl Interpreter for ProdInterpreter {
    fn wait_until_ready(&self) -> Fallible<()> {
        self.spotify_player.wait_until_ready()?;
        Ok(())
    }

    fn play_http(&self, url: &str) -> Fallible<()> {
        self.http_player.start_playback(url)?;
        Ok(())
    }

    fn stop_http(&self) -> Fallible<()> {
        self.http_player.stop_playback().map_err(|err| err.into())
    }
    fn play_spotify(&self, spotify_uri: &str) -> Fallible<()> {
        self.spotify_player
            .start_playback(&spotify_uri)
            .map_err(|err| err.into())
    }
    fn stop_spotify(&self) -> Fallible<()> {
        self.spotify_player
            .stop_playback()
            .map_err(|err| err.into())
    }
    fn led_on(&self) -> Fallible<()> {
        info!("Switching LED on");
        self.led_controller.switch_on(Led::Playback)
    }
    fn led_off(&self) -> Fallible<()> {
        info!("Switching LED off");
        self.led_controller.switch_off(Led::Playback)
    }
    fn generic_command(&self, cmd: String) -> Fallible<()> {
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

impl ProdInterpreter {
    pub fn new(config: &Config) -> Fallible<Self> {
        let config = config.clone();
        let led_controller = Arc::new(Box::new(led::gpio_cdev::GpioCdev::new()?)
            as Box<dyn LedController + 'static + Send + Sync>);
        let spotify_player = SpotifyPlayer::new(&config)?;
        let http_player = HttpPlayer::new()?;
        Ok(ProdInterpreter {
            spotify_player,
            http_player,
            led_controller,
            _config: config,
        })
    }
}

pub mod test {
    use super::*;
    use crossbeam_channel::{self, Receiver, Sender};
    use Effects::*;

    pub struct TestInterpreter {
        tx: Sender<Effects>,
    }

    impl TestInterpreter {
        pub fn new() -> (TestInterpreter, Receiver<Effects>) {
            let (tx, rx) = crossbeam_channel::unbounded();
            let interpreter = TestInterpreter { tx };
            (interpreter, rx)
        }
    }

    impl Interpreter for TestInterpreter {
        fn wait_until_ready(&self) -> Fallible<()> {
            Ok(())
        }

        fn play_http(&self, url: &str) -> Fallible<()> {
            self.tx
                .send(PlayHttp {
                    url: url.to_string().clone(),
                })
                .unwrap();
            Ok(())
        }
        fn stop_http(&self) -> Fallible<()> {
            self.tx.send(StopHttp).unwrap();
            Ok(())
        }
        fn play_spotify(&self, spotify_uri: &str) -> Fallible<()> {
            self.tx
                .send(PlaySpotify {
                    spotify_uri: spotify_uri.to_string().clone(),
                })
                .unwrap();
            Ok(())
        }
        fn stop_spotify(&self) -> Fallible<()> {
            self.tx.send(StopSpotify).unwrap();
            Ok(())
        }
        fn led_on(&self) -> Fallible<()> {
            self.tx.send(LedOn).unwrap();
            Ok(())
        }
        fn led_off(&self) -> Fallible<()> {
            self.tx.send(LedOff).unwrap();
            Ok(())
        }
        fn generic_command(&self, cmd: String) -> Fallible<()> {
            self.tx
                .send(GenericCommand(cmd.to_string().clone()))
                .unwrap();
            Ok(())
        }
    }
}
