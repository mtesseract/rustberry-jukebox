use std::sync::Arc;
use std::time::Duration;

use failure::Fallible;
use slog_scope::{error, info, warn};

use crate::config::Config;
use crate::effects::{DynInterpreter, Interpreter};
use crate::input_controller::{button, Input, InputSource, InputSourceFactory};
use crate::player::{PlaybackRequest, Player};
use futures::future::AbortHandle;

use crate::led::{self, Blinker};

#[derive(Clone)]
pub struct App {
    config: Config,
    interpreter: Arc<Box<dyn Interpreter + Sync + Send + 'static>>,
    input_source_factory: Arc<Box<dyn InputSourceFactory + Sync + Send + 'static>>,
    blinker: Blinker,
}

impl App {
    pub fn new(
        config: Config,
        interpreter: Arc<Box<dyn Interpreter + Sync + Send + 'static>>,
        blinker: Blinker,
        input_source_factory: Arc<Box<dyn InputSourceFactory + Sync + Send + 'static>>,
    ) -> Fallible<Self> {
        let app = Self {
            config,
            interpreter,
            input_source_factory,
            blinker,
        };
        Ok(app)
    }

    pub async fn run(&self) -> Fallible<AbortHandle> {
        let input_source_factory = self.input_source_factory.clone();
        let blinker = self.blinker.clone();
        let interpreter = self.interpreter.clone();
        let config = self.config.clone();
        let (f, abortable_handle) = futures::future::abortable(async move {
            info!("Consuming Input Source...");
            let input_source = input_source_factory.consume().unwrap();
            info!("About to run Jukebox logic");
            Self::run_jukebox(config, input_source, blinker, interpreter).await;
            info!("Jukebox loop terminated");
        });
        tokio::spawn(f);
        Ok(abortable_handle)
    }

    pub async fn run_jukebox(
        config: Config,
        input_source: Box<dyn InputSource + Sync + Send + 'static>,
        blinker: Blinker,
        interpreter: DynInterpreter,
    ) -> Fallible<()> {
        info!("Running Jukebox App");
        let player = Player::new(interpreter.clone()).await?;
        blinker
            .run_async(led::Cmd::Repeat(
                1,
                Box::new(led::Cmd::Many(vec![
                    led::Cmd::On(Duration::from_secs(1)),
                    led::Cmd::Off(Duration::from_secs(0)),
                ])),
            ))
            .await;

        loop {
            warn!("app loop");
            let mut rx = input_source.receiver();
            let el = match rx.recv().await {
                Err(err) => {
                    // Closed or lagged.
                    error!(
                        "Error while consuming input source in Jukebox App: {:?}",
                        err
                    );
                    return Err(err.into());
                }
                Ok(input) => {
                    warn!("input = {:?}", input);
                    input
                }
            };

            blinker.stop();
            match el {
                Input::Button(cmd) => match cmd {
                    button::Command::Shutdown => {
                        if let Err(err) = interpreter
                            .generic_command(
                                config
                                    .shutdown_command
                                    .clone()
                                    .unwrap_or("sudo shutdown -h now".to_string()),
                            )
                            .await
                        {
                            error!("Failed to execute shutdown command: {}", err);
                        } else {
                            return Ok(()); // For tests we need this to terminate.
                        }
                    }
                    button::Command::VolumeUp => {
                        if let Err(err) = interpreter
                            .generic_command(
                                config
                                    .volume_up_command
                                    .clone()
                                    .unwrap_or("amixer -q -M set PCM 10%+".to_string()),
                            )
                            .await
                        {
                            error!("Failed to increase volume: {}", err);
                        }
                    }
                    button::Command::VolumeDown => {
                        if let Err(err) = interpreter
                            .generic_command(
                                config
                                    .volume_down_command
                                    .clone()
                                    .unwrap_or("amixer -q -M set PCM 10%-".to_string()),
                            )
                            .await
                        {
                            error!("Failed to decrease volume: {}", err);
                        }
                    }
                },
                Input::Playback(request) => {
                    if let Err(err) = player.playback(request.clone()).await {
                        error!("Failed to execute playback request {:?}: {}", request, err);
                    }
                    match request {
                        PlaybackRequest::Start(_) => {
                            interpreter.led_on().await;
                        }
                        PlaybackRequest::Stop => {
                            interpreter.led_off().await;
                        }
                    }
                }
            }
        }
    }
}
