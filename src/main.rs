use std::sync::Arc;
use std::time::Duration;
use tokio::runtime::{self, Runtime};

use anyhow::{Context, Result};
use crossbeam_channel::{self, Receiver, Select};
use slog::{self, o, Drain};
use slog_scope::{error, info, warn};

use rustberry::components::tag_mapper::TagMapper;
use rustberry::config::Config;
use rustberry::effects::{Interpreter, ProdInterpreter};
use rustberry::input_controller::{button, playback, Input};
use rustberry::led::{self, Blinker};
use rustberry::player::{self, PlaybackRequest, Player};

fn main() -> Result<()> {
    let decorator = slog_term::TermDecorator::new().build();
    let drain = slog_term::FullFormat::new(decorator).build().fuse();
    let drain = slog_async::Async::new(drain).build().fuse();
    let logger = slog::Logger::root(drain, o!());
    let _guard = slog_scope::set_global_logger(logger);

    slog_scope::scope(&slog_scope::logger().new(o!()), main_with_log)
}

fn main_with_log() -> Result<()> {
    let config = envy::from_env::<Config>()?;
    info!("Starting application");

    let runtime = runtime::Builder::new()
        .threaded_scheduler()
        .enable_all()
        .build()
        .unwrap();

    info!("Creating TagMapper");
    let tag_mapper = TagMapper::new_initialized(&config.tag_mapper_configuration_file)
        .context("Creating tag_mapper")?;
    let tag_mapper_handle = tag_mapper.handle();
    tag_mapper_handle.debug_dump();

    // Create Effects Channel and Interpreter.
    let interpreter = ProdInterpreter::new(&config).context("Creating production interpreter")?;
    let interpreter: Arc<Box<dyn Interpreter + Sync + Send + 'static>> =
        Arc::new(Box::new(interpreter));

    let blinker =
        Blinker::new(runtime.handle().clone(), interpreter.clone()).context("Creating blinker")?;
    blinker.run_async(led::Cmd::Loop(Box::new(led::Cmd::Many(vec![
        led::Cmd::On(Duration::from_millis(100)),
        led::Cmd::Off(Duration::from_millis(100)),
    ]))));

    interpreter
        .wait_until_ready()
        .context("Waiting for interpreter readiness")?;

    // Prepare individual input channels.
    info!("Creating Button Controller");
    let button_controller_handle: button::Handle<Input> =
        button::cdev_gpio::CdevGpio::new_from_env() .context("Creating button controller")?;
    info!("Creating PlayBackRequestTransmitter");
    let playback_controller_handle: playback::Handle<Input> =
        playback::rfid::PlaybackRequestTransmitterRfid::new()
            .context("Creating playback controller")?;

    // Execute Application Logic, producing Effects.
    let application = App::new(
        runtime,
        config,
        interpreter.clone(),
        blinker,
        &[
            button_controller_handle.channel(),
            playback_controller_handle.channel(),
        ],
        tag_mapper,
    )
    .context("Creating application object")?;
    info!("Running application");
    application
        .run()
        .context("Jukebox loop terminated, terminating application")?;
    unreachable!();
}

struct App {
    config: Config,
    player: player::PlayerHandle,
    interpreter: Arc<Box<dyn Interpreter + Sync + Send + 'static>>,
    inputs: Vec<Receiver<Input>>,
    blinker: Blinker,
    runtime: tokio::runtime::Runtime,
    tag_mapper: TagMapper,
}

impl App {
    pub fn new(
        runtime: Runtime,
        config: Config,
        interpreter: Arc<Box<dyn Interpreter + Sync + Send + 'static>>,
        blinker: Blinker,
        inputs: &[Receiver<Input>],
        tag_mapper: TagMapper,
    ) -> Result<Self> {
        let player_config = player::Config {
            trigger_only_mode: config.trigger_only_mode,
        };
        let player = Player::new(
            Some(blinker.clone()),
            runtime.handle(),
            interpreter.clone(),
            player_config,
        )?;
        let app = Self {
            config,
            inputs: inputs.to_vec(),
            player,
            interpreter,
            blinker,
            runtime,
            tag_mapper,
        };

        // info!("Running in {} mode", if app.config.trigger_only_mode { "trigger-only" } else { "traditional" });
        Ok(app)
    }

    pub fn run(self) -> Result<()> {
        self.blinker.run_async(led::Cmd::Repeat(
            1,
            Box::new(led::Cmd::Many(vec![
                led::Cmd::On(Duration::from_secs(1)),
                led::Cmd::Off(Duration::from_secs(0)),
            ])),
        ));
        let mut sel = Select::new();
        for r in &self.inputs {
            sel.recv(r);
        }

        loop {
            // Wait until a receive operation becomes ready and try executing it.
            let index = sel.ready();
            let res = self.inputs[index].try_recv();

            match res {
                Err(err) => {
                    if err.is_empty() {
                        // If the operation turns out not to be ready, retry.
                        continue;
                    } else {
                        error!(
                            "Failed to receive input event on channel {}: {}",
                            index, err
                        );
                        // remove the channel.
                        warn!("Not watching input channel {} any longer", index);
                        sel.remove(index);
                    }
                }
                Ok(input) => {
                    self.blinker.stop();
                    match input {
                        Input::Button(cmd) => {
                            match cmd {
                                button::Command::Shutdown => {
                                    if let Err(err) = self.interpreter.generic_command(
                                        self.config
                                            .shutdown_command
                                            .clone()
                                            .unwrap_or_else(|| "sudo shutdown -h now".to_string()),
                                    ) {
                                        error!("Failed to execute shutdown command: {}", err);
                                    }
                                }
                                button::Command::VolumeUp => {
                                    if let Err(err) = self.interpreter.generic_command(
                                        self.config.volume_up_command.clone().unwrap_or_else(
                                            || "amixer -q -M set PCM 10%+".to_string(),
                                        ),
                                    ) {
                                        error!("Failed to increase volume: {}", err);
                                    }
                                }
                                button::Command::VolumeDown => {
                                    if let Err(err) = self.interpreter.generic_command(
                                        self.config.volume_down_command.clone().unwrap_or_else(
                                            || "amixer -q -M set PCM 10%-".to_string(),
                                        ),
                                    ) {
                                        error!("Failed to decrease volume: {}", err);
                                    }
                                }
                                button::Command::PauseContinue => {
                                    if let Err(err) = self.player.pause_continue() {
                                        error!("Failed to execute pause_continue request: {}", err);
                                    }
                                }
                            }
                        }
                        Input::Playback(request) => {
                            if let Err(err) = self.player.playback(request.clone()) {
                                error!("Failed to execute playback request {:?}: {}", request, err);
                            }
                        }
                    }
                }
            };
        }
    }
}

// #[cfg(test)]
// mod test {
//     use rustberry::config::Config;
//     use rustberry::effects::{test::TestInterpreter, Effects};
//     use rustberry::input_controller::{button, Input};
//     use rustberry::components::tag_mapper::TagMapper;
//     use super::*;

//     #[test]
//     fn jukebox_can_be_shut_down() {
//         let (interpreter, effects_rx) = TestInterpreter::new();
//         let interpreter =
//             Arc::new(Box::new(interpreter) as Box<dyn Interpreter + Send + Sync + 'static>);
//         let (_effects_tx, effects_rx) = crossbeam_channel::bounded(10);
//         let config: Config = Config {
//             enable_spotify: false,
//             post_init_command: None,
//             shutdown_command: None,
//             volume_up_command: None,
//             volume_down_command: None,
//             trigger_only_mode: false,
//             tag_mapper_configuration_file: "/some/file.yaml".to_string(),
//         };
//         let blinker = Blinker::new(interpreter.clone()).unwrap();
//         let inputs = vec![Input::Button(button::Command::Shutdown)];
//         let effects_expected = vec![Effects::GenericCommand("sudo shutdown -h now".to_string())];
//         let (input_tx, input_rx) = crossbeam_channel::unbounded();
//         let tag_mapper = TagMapper::new(&config.tag_mapper_configuration_file);
//         let app = App::new(config, interpreter, blinker, &vec![input_rx], tag_mapper).unwrap();
//         for input in inputs {
//             input_tx.send(input).unwrap();
//         }
//         drop(input_tx);
//         app.run();
//         let produced_effects: Vec<Effects> = effects_rx.iter().collect();

//         assert_eq!(produced_effects, effects_expected);
//     }
// }
