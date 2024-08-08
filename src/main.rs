use std::sync::Arc;
use std::time::Duration;
use std::path::Path;

use anyhow::{Context, Result};
use crossbeam_channel::{self, Receiver, Select};
use tracing::{error, info, warn};
use tracing_subscriber::{filter, fmt, prelude::*, reload};

use rustberry::components::config::ConfigLoader;
use rustberry::components::tag_mapper::TagMapper;
use rustberry::effects::{Effect, Interpreter, ProdInterpreter};
use rustberry::input_controller::{
    button::{self, cdev_gpio::CdevGpio},
    rfid_playback::{self, rfid::PlaybackRequestTransmitterRfid},
    Input,
};
use rustberry::led::{self, Blinker};
use rustberry::model::config::Config;
use rustberry::components::config::ConfigLoaderHandle;

use rustberry::player::{self, Player};

const DEFAULT_JUKEBOX_CONFIG_FILE: &str = "/etc/jukebox/conf.yaml";

#[tokio::main]
async fn main() -> Result<()> {
    let filter = filter::LevelFilter::INFO;
    let (filter, reload_handle) = reload::Layer::new(filter);
    tracing_subscriber::registry()
        .with(filter)
        .with(fmt::Layer::default().with_writer(std::io::stderr))
        .init();

    info!("Starting application");
    let config_loader = ConfigLoader::new(Path::new(DEFAULT_JUKEBOX_CONFIG_FILE), reload_handle)?;
    let config = config_loader.get();

    info!("Creating TagMapper");
    let tag_mapper = TagMapper::new_initialized(&config.tag_mapper_configuration_file)
        .context("Creating tag_mapper")?;
    let tag_mapper_handle = tag_mapper.handle();
    tag_mapper_handle.debug_dump();

    // Create Effects Channel and Interpreter.
    let interpreter = ProdInterpreter::new(config_loader).context("Creating production interpreter")?;
    // let interpreter: Arc<Box<dyn Interpreter + Sync + Send + 'static>> =
    //     Arc::new(Box::new(interpreter));

    // let blinker = Blinker::new(interpreter.clone()).context("Creating blinker")?;
    // blinker.run_async(led::Cmd::Loop(Box::new(led::Cmd::Many(vec![
    //     led::Cmd::On(Duration::from_millis(100)),
    //     led::Cmd::Off(Duration::from_millis(100)),
    // ]))));

    interpreter
        .wait_until_ready()
        .context("Waiting for interpreter readiness")?;
    let interpreter_state = interpreter.interpreter_state.clone();

    // Prepare input channels.
    let (inputs_tx, inputs_rx) = crossbeam_channel::bounded(10);

    info!("Creating Button Controller");
    let button_controller_handle: button::Handle<Input> =
        CdevGpio::new_from_env(inputs_tx.clone()).context("Creating button controller")?;

    if config.enable_rfid_controller {
        info!("Creating PlayBackRequestTransmitter");
        let playback_controller_handle: rfid_playback::Handle<Input> =
            PlaybackRequestTransmitterRfid::new(inputs_tx.clone()).context("Creating playback controller")?;
    } else {
        warn!("Skipping creation of PlayBackRequestTransmitter: RFID controller disabled.");
    }

    // Effect interpreter.
    let (effect_tx, effect_rx) = crossbeam_channel::bounded::<Effect>(50);
    tokio::spawn_blocking(|| {
        for effect in effect_rx {
            if let Err(err) = interpreter.interprete(effect) {
                error!("interpreting effect {} failed: {}", effect, err);
            }
        }
    });

    // Execute Application Logic.
    info!("Running application");
    let _res = run(config_loader, input_rx, output_tx).unwrap();
    unreachable!();
}

fn run(config: ConfigLoaderHandle, input: Receiver<Input>, output: Sender<Effect>) -> Result<()> {
    let plater = Player::new(effect_tx, config, tag_mapper, interpreter_state);
    for input_ev in input {
        let res = process_ev(config, player, input, output);
        match res {
            Err(err) => {
                error!("Failed to process input event {}: {}", input, err);
            }
            Ok(effects) => {
                for effect in effects {
                    if let Err(err) = output.send(effect) {
                        error!("Failed to send output effect {}: {}", effect, err);
                    }
                }
            }
        }
    }
    unreachable!()
}

fn process_ev(config_loader: ConfigLoaderHandle, player: Player, input: Input, output: Sender<Effect>) -> Result<Vec<Effect>> {
    let config = config_loader.get();

        match input {
            Input::Button(cmd) => match cmd {
                button::Command::VolumeUp => {
                    let cmd =
                        config.volume_up_command.clone().unwrap_or_else(|| {
                            "pactl set-sink-volume 0 +10%".to_string()
                        });
                    return Ok(vec![Effect::GenericCommand(cmd)]);
                }
                button::Command::VolumeDown => {
                    let cmd =
                        config.volume_up_command.clone().unwrap_or_else(|| {
                            "pactl set-sink-volume 0 -10%".to_string()
                        });
                    return Ok(vec![Effect::GenericCommand(cmd)]);
                }
                button::Command::PauseContinue => {
                    return Ok(player.pause_continue(output)?);
                }
            },
            Input::Playback(request) => {
                return Ok(player.playback(request.clone())?);
            }
        }
}