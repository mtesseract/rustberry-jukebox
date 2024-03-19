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
    spotify_player: Option<SpotifyPlayer>,
    http_player: HttpPlayer,
    led_controller: Arc<Box<dyn LedController + 'static + Send + Sync>>,
    _config: Config,
}

#[async_trait]

pub trait Interpreter {
    fn wait_until_ready(&self) -> Fallible<()>;
    async fn play(
        &self,
        res: PlaybackResource,
        pause_state: Option<PauseState>,
    ) -> Fallible<DynPlaybackHandle>;
    // fn stop(&self, handle: DynPlaybackHandle) -> Fallible<()>;
    fn led_on(&self) -> Fallible<()>;
    fn led_off(&self) -> Fallible<()>;
    fn generic_command(&self, cmd: String) -> Fallible<()>;
}

#[async_trait]
impl Interpreter for ProdInterpreter {
    fn wait_until_ready(&self) -> Fallible<()> {
        if let Some(ref spotify_player) = self.spotify_player {
            spotify_player.wait_until_ready()?;
        }
        Ok(())
    }

    async fn play(
        &self,
        res: PlaybackResource,
        pause_state: Option<PauseState>,
    ) -> Fallible<DynPlaybackHandle> {
        use PlaybackResource::*;
        match res {
            SpotifyUri(uri) => {
                if let Some(ref spotify_player) = self.spotify_player {
                    spotify_player
                        .start_playback(&uri, pause_state)
                        .await
                        .map(|x| Box::new(x) as DynPlaybackHandle)
                } else {
                    Err(failure::err_msg("Spotify Player not available"))
                }
            }
            Http(url) => self
                .http_player
                .start_playback(&url, pause_state)
                .await
                .map(|x| Box::new(x) as DynPlaybackHandle),
            // .map_err(|err| err.into()),
        }
    }

    // fn stop(&self, handle: DynPlaybackHandle) -> Fallible<()> {
    // }

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
        let mut spotify_player: Option<SpotifyPlayer> = None;
        if config.enable_spotify {
            spotify_player = Some(SpotifyPlayer::newFromEnv()?);
        }
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

    struct DummyPlaybackHandle;

    #[async_trait]
    impl PlaybackHandle for DummyPlaybackHandle {
        async fn stop(&self) -> Fallible<()> {
            Ok(())
        }
        async fn is_complete(&self) -> Fallible<bool> {
            Ok(true)
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
                SpotifyUri(uri) => self.tx.send(PlaySpotify { spotify_uri: uri })?,
                Http(url) => self.tx.send(PlayHttp { url })?,
            }
            Ok(Box::new(DummyPlaybackHandle) as DynPlaybackHandle)
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
            self.tx.send(GenericCommand(cmd)).unwrap();
            Ok(())
        }
    }
}
