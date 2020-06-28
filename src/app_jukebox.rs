use std::sync::Arc;
use std::time::Duration;

use failure::Fallible;
use slog_scope::{error, info, warn};

use tokio::sync::broadcast::Receiver; // FIXME: use mpsc?

use crate::config::Config;
use crate::effects::{DynInterpreter, DynInterpreterFactory, InterpreterFactory, Interpreter};
use crate::input_controller::{button, Input, InputSource, InputSourceFactory, DynInputSourceFactory};
use crate::player::{PlaybackRequest, Player};

use crate::led::{self, Blinker};

pub struct App {
    config: Config,
    interpreter: DynInterpreter,
    input_source: Box<dyn InputSource + Sync + Send + 'static>,
    rx: Receiver<Input>,
}

impl App {
    pub async fn new(
        config: Config,
        interpreter_factory: &DynInterpreterFactory,
        input_source_factory: &DynInputSourceFactory,
    ) -> Fallible<Self> {
        let interpreter = interpreter_factory.run().await?;
        let (input_source, rx) = input_source_factory.consume()?;

        let app = Self {
            config,
            interpreter,
            input_source,
            rx,
        };
        Ok(app)
    }

    pub async fn run(self) -> Fallible<()> {
        let interpreter = Arc::new(self.interpreter);

        let blinker = Blinker::new(interpreter.clone())?;
        blinker
            .run_async(led::Cmd::Loop(Box::new(led::Cmd::Many(vec![
                led::Cmd::On(Duration::from_millis(100)),
                led::Cmd::Off(Duration::from_millis(100)),
            ]))))
            .await;

        info!("Waiting for interpreter readiness...");

        interpreter.wait_until_ready().await.map_err(|err| {
            error!("Failed to wait for interpreter readiness: {}", err);
            err
        })?;

        info!("Interpreter for Jukebox ready");

        info!("About to run Jukebox logic");
        if let Err(err) = Self::run_jukebox(self.config, self.rx, self.input_source, blinker, interpreter).await {
            error!("Jukebox loop terminated with error: {}", err);
        } else {
            error!("Jukebox loop terminated unexpectedly");
        }
        Ok(())
    }

    pub async fn run_jukebox(
        config: Config,
        rx: Receiver<Input>,
        input_source: Box<dyn InputSource + Sync + Send + 'static>,
        blinker: Blinker,
        interpreter: Arc<DynInterpreter>,
    ) -> Fallible<()> {
        info!("Running Jukebox App");
        let mut rx = rx;
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
            let el = match rx.recv().await {
                Err(tokio::sync::broadcast::RecvError::Lagged(_)) => {
                    warn!("Lagged while transmitting button events");
                    continue
                },
                Err(err) => {
                    // Closed.
                    error!(
                        "Error while consuming input source in Jukebox App: {:?}",
                        err
                    );
                    return Err(err.into());
                }
                Ok(input) => input,
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
                           let _ = interpreter.led_on().await;
                        }
                        PlaybackRequest::Stop => {
                            let _ = interpreter.led_off().await;
                        }
                    }
                }
            }
        }
    }
}
