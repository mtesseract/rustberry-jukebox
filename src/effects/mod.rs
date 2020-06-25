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
use async_trait::async_trait;
use failure::Fallible;
use http_player::HttpPlayer;
use led::{Led, LedController};
use slog_scope::{info, warn};
use spotify::player::SpotifyPlayer;
use std::process::Command;

use crate::player::{DynPlaybackHandle, PauseState, PlaybackHandle, PlaybackResource};

pub type DynInterpreter = Box<dyn Interpreter + Sync + Send + 'static>;
pub type DynInterpreterFactory = Box<dyn InterpreterFactory + Sync + Send + 'static>;

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

pub struct ProdInterpreterFactory {
    _config: Config,
}

impl ProdInterpreterFactory {
    pub fn new(config: &Config) -> Self { ProdInterpreterFactory { _config: config.clone() }}
}

impl InterpreterFactory for ProdInterpreterFactory {
    fn run(&self) -> Fallible<DynInterpreter> {
        let interpreter = ProdInterpreter::new(&self._config)?;
        Ok(Box::new(interpreter))
    }
}
#[async_trait]
pub trait Interpreter {
    fn wait_until_ready(&self) -> Fallible<()>;
    async fn play(
        &self,
        res: PlaybackResource,
        pause_state: Option<PauseState>,
    ) -> Fallible<DynPlaybackHandle>;
    async fn led_on(&self) -> Fallible<()>;
    async fn led_off(&self) -> Fallible<()>;
    async fn generic_command(&self, cmd: String) -> Fallible<()>;
}


pub trait InterpreterFactory {
    fn run(&self) -> Fallible<Box<dyn Interpreter + Sync + Send + 'static>>;
}

#[async_trait]
impl Interpreter for ProdInterpreter {
    fn wait_until_ready(&self) -> Fallible<()> {
        self.spotify_player.wait_until_ready()?;
        Ok(())
    }

    async fn play(
        &self,
        res: PlaybackResource,
        pause_state: Option<PauseState>,
    ) -> Fallible<DynPlaybackHandle> {
        use PlaybackResource::*;
        match res {
            SpotifyUri(uri) => self
                .spotify_player
                .start_playback(&uri, pause_state)
                .await
                .map(|x| Box::new(x) as DynPlaybackHandle)
                .map_err(|err| err.into()),
            Http(url) => self
                .http_player
                .start_playback(&url, pause_state)
                .await
                .map(|x| Box::new(x) as DynPlaybackHandle)
                .map_err(|err| err.into()),
        }
    }

    async fn led_on(&self) -> Fallible<()> {
        info!("Switching LED on");
        self.led_controller.switch_on(Led::Playback)
    }

    async fn led_off(&self) -> Fallible<()> {
        info!("Switching LED off");
        self.led_controller.switch_off(Led::Playback)
    }

    async fn generic_command(&self, cmd: String) -> Fallible<()> {
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
    use async_trait::async_trait;
    use tokio::sync::mpsc::{channel, Receiver, Sender};
    use Effects::*;

    pub struct TestInterpreter {
        tx: Sender<Effects>,
    }

    pub struct TestInterpreterFactory {
        tx: Sender<Effects>,
    }

    impl TestInterpreter {
        pub fn new() -> (TestInterpreter, Receiver<Effects>) {
            let (tx, rx) = channel(100);
            let interpreter = TestInterpreter { tx };
            (interpreter, rx)
        }
    }

    impl TestInterpreterFactory {
        pub fn new() -> (TestInterpreterFactory, Receiver<Effects>) {
            let (tx, rx) = channel(100);
            let interpreter_factory = TestInterpreterFactory { tx };
            (interpreter_factory, rx)
        }
    }

    impl InterpreterFactory for TestInterpreterFactory {
        fn run(&self) -> Fallible<Box<dyn Interpreter + Sync + Send + 'static>> {
            Ok(Box::new(TestInterpreter { tx: self.tx.clone() }))
        }
    }

    struct TestSpotifyPlaybackHandle {
        tx: Sender<Effects>,
    }
    struct TestHttpPlaybackHandle {
        tx: Sender<Effects>,
    }

    #[async_trait]
    impl PlaybackHandle for TestSpotifyPlaybackHandle {
        async fn stop(&self) -> Fallible<()> {
            self.tx.clone().send(Effects::StopSpotify).await.unwrap();
            Ok(())
        }
        async fn is_complete(&self) -> Fallible<bool> {
            Ok(true)
        }
        async fn pause(&self) -> Fallible<()> {
            self.tx.clone().send(Effects::StopSpotify).await.unwrap();
            Ok(())
        }
        async fn cont(&self, _pause_state: PauseState) -> Fallible<()> {
            Ok(())
        }
        async fn replay(&self) -> Fallible<()> {
            Ok(())
        }
    }

    #[async_trait]
    impl PlaybackHandle for TestHttpPlaybackHandle {
        async fn stop(&self) -> Fallible<()> {
            self.tx.clone().send(Effects::StopHttp).await.unwrap();
            Ok(())
        }
        async fn is_complete(&self) -> Fallible<bool> {
            Ok(true)
        }
        async fn pause(&self) -> Fallible<()> {
            Ok(())
        }
        async fn cont(&self, _pause_state: PauseState) -> Fallible<()> {
            Ok(())
        }
        async fn replay(&self) -> Fallible<()> {
            Ok(())
        }
    }

    #[async_trait]
    impl Interpreter for TestInterpreter {
        fn wait_until_ready(&self) -> Fallible<()> {
            Ok(())
        }

        async fn play(
            &self,
            res: PlaybackResource,
            _pause_state: Option<PauseState>,
        ) -> Fallible<DynPlaybackHandle> {
            use PlaybackResource::*;

            match res {
                SpotifyUri(uri) => {
                    self.tx
                        .clone()
                        .send(PlaySpotify {
                            spotify_uri: uri.to_string().clone(),
                        })
                        .await?;
                    Ok(Box::new(TestSpotifyPlaybackHandle {
                        tx: self.tx.clone(),
                    }) as DynPlaybackHandle)
                }
                Http(url) => {
                    self.tx
                        .clone()
                        .send(PlayHttp {
                            url: url.to_string().clone(),
                        })
                        .await?;
                    Ok(Box::new(TestHttpPlaybackHandle {
                        tx: self.tx.clone(),
                    }) as DynPlaybackHandle)
                }
            }
        }

        async fn led_on(&self) -> Fallible<()> {
            self.tx.clone().send(LedOn).await.unwrap();
            Ok(())
        }
        async fn led_off(&self) -> Fallible<()> {
            self.tx.clone().send(LedOff).await.unwrap();
            Ok(())
        }
        async fn generic_command(&self, cmd: String) -> Fallible<()> {
            self.tx
                .clone()
                .send(GenericCommand(cmd.to_string().clone()))
                .await
                .unwrap();
            Ok(())
        }
    }
}
