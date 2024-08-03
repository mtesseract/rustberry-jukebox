use std::sync::Arc;
use std::time::Duration;
// use tokio::runtime::{self, };
use std::path::Path;

use anyhow::{Context, Result};
use crossbeam_channel::{self, Receiver, Select};
use tracing::{error, info, warn};
use tracing_subscriber;

use rustberry::components::tag_mapper::TagMapper;
use rustberry::components::config::ConfigLoader;
use rustberry::model::config::Config;
use rustberry::effects::{Interpreter, ProdInterpreter};
use rustberry::input_controller::{
    button::{self, cdev_gpio::CdevGpio},
    rfid_playback::{self, rfid::PlaybackRequestTransmitterRfid},
    Input,
};
use rustberry::led::{self, Blinker};
use rustberry::player::{self, Player};

const DEFAULT_JUKEBOX_CONFIG_FILE: &str = "/etc/jukebox/config";

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();
    info!("Starting application");
    let config_loader = ConfigLoader::new(Path::new(DEFAULT_JUKEBOX_CONFIG_FILE))?;
    let config = config_loader.get()?;
    if let Err(err) = config_loader.spawn_async_loader() {
        error!("Failed to spawn aync config loader: {}", err);
    }

    info!("Creating TagMapper");
    let tag_mapper = TagMapper::new_initialized(&config.tag_mapper_configuration_file)
        .context("Creating tag_mapper")?;
    let tag_mapper_handle = tag_mapper.handle();
    tag_mapper_handle.debug_dump();

    // Create Effects Channel and Interpreter.
    let interpreter = ProdInterpreter::new(&config).context("Creating production interpreter")?;
    let interpreter: Arc<Box<dyn Interpreter + Sync + Send + 'static>> =
        Arc::new(Box::new(interpreter));

    let blinker = Blinker::new(interpreter.clone()).context("Creating blinker")?;
    blinker.run_async(led::Cmd::Loop(Box::new(led::Cmd::Many(vec![
        led::Cmd::On(Duration::from_millis(100)),
        led::Cmd::Off(Duration::from_millis(100)),
    ]))));

    interpreter
        .wait_until_ready()
        .context("Waiting for interpreter readiness")?;

    // Prepare input channels.
    info!("Creating Button Controller");
    let button_controller_handle: button::Handle<Input> =
        CdevGpio::new_from_env().context("Creating button controller")?;
    info!("Creating PlayBackRequestTransmitter");
    let playback_controller_handle: rfid_playback::Handle<Input> =
        PlaybackRequestTransmitterRfid::new().context("Creating playback controller")?;

    // Execute Application Logic, producing Effects.
    let application = App::new(
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
}

impl App {
    pub fn new(
        config: Config,
        interpreter: Arc<Box<dyn Interpreter + Sync + Send + 'static>>,
        blinker: Blinker,
        inputs: &[Receiver<Input>],
        tag_mapper: TagMapper,
    ) -> Result<Self> {
        let player_config = player::Config {
            trigger_only_mode: config.trigger_only_mode,
        };
        info!(
            "Running in {} mode",
            if player_config.trigger_only_mode {
                "trigger-only"
            } else {
                "traditional"
            }
        );
        let player = Player::new(
            Some(blinker.clone()),
            interpreter.clone(),
            player_config,
            tag_mapper.handle(),
        )
        .context("creating Player")?;
        let app = Self {
            config,
            inputs: inputs.to_vec(),
            player,
            interpreter,
            blinker,
        };

        Ok(app)
    }

    // Runs main application logic.
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

        // Main loop is an event handler .
        loop {
            // Wait until a receive operation becomes ready and handle the event.
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
                        Input::Button(cmd) => match cmd {
                            button::Command::Shutdown => {
                                let cmd = self
                                    .config
                                    .shutdown_command
                                    .clone()
                                    .unwrap_or_else(|| "sudo shutdown -h now".to_string());
                                if let Err(err) = self.interpreter.generic_command(&cmd) {
                                    error!("Failed to execute shutdown command '{}': {}", cmd, err);
                                }
                            }
                            button::Command::VolumeUp => {
                                let cmd =
                                    self.config.volume_up_command.clone().unwrap_or_else(|| {
                                        "pactl set-sink-volume 0 +10%".to_string()
                                    });
                                if let Err(err) = self.interpreter.generic_command(&cmd) {
                                    error!(
                                        "Failed to increase volume using command {}: {}",
                                        cmd, err
                                    );
                                }
                            }
                            button::Command::VolumeDown => {
                                let cmd =
                                    self.config.volume_up_command.clone().unwrap_or_else(|| {
                                        "pactl set-sink-volume 0 -10%".to_string()
                                    });
                                if let Err(err) = self.interpreter.generic_command(&cmd) {
                                    error!("Failed to decrease volume: {}", err);
                                }
                            }
                            button::Command::PauseContinue => {
                                if let Err(err) = self.player.pause_continue() {
                                    error!("Failed to execute pause_continue request: {}", err);
                                }
                            }
                        },
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
