use std::path::Path;
use std::sync::{Arc, RwLock};

use anyhow::{Context, Result};
use crossbeam_channel::{self, Receiver, Sender};
use rustberry::effects::InterpreterState;
use tracing::{error, info, warn};
use tracing_subscriber::{filter, fmt, prelude::*, reload};

use rustberry::components::config::ConfigLoader;
use rustberry::components::config::ConfigLoaderHandle;
use rustberry::components::tag_mapper::{TagMapper, TagMapperHandle};
use rustberry::effects::{Effect, Interpreter, ProdInterpreter};
use rustberry::input_controller::{
    button::{self, cdev_gpio::CdevGpio},
    rfid_playback::rfid::PlaybackRequestTransmitterRfid,
    Input,
};
// use rustberry::led;
//::{self, Blinker};
// use rustberry::model::config::Config;

use rustberry::player::Player;

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
    tag_mapper.debug_dump();

    // Create Effects Channel and Interpreter.
    let mut interpreter =
        ProdInterpreter::new(config_loader.clone()).context("Creating production interpreter")?;
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
    let _button_controller_handle =
        CdevGpio::new_from_env(inputs_tx.clone()).context("Creating button controller")?;

    if config.enable_rfid_controller {
        info!("Creating PlayBackRequestTransmitter");
        let _playback_controller_handle = PlaybackRequestTransmitterRfid::new(inputs_tx.clone())
            .context("Creating playback controller")?;
    } else {
        warn!("Skipping creation of PlayBackRequestTransmitter: RFID controller disabled.");
    }

    // Effect interpreter.
    let (effect_tx, effect_rx) = crossbeam_channel::bounded::<Effect>(50);
    tokio::task::spawn_blocking(move || {
        for effect in effect_rx {
            if let Err(err) = interpreter.interprete(effect.clone()) {
                error!("interpreting effect {:?} failed: {}", effect, err);
            }
        }
    });

    // Execute Application Logic.
    info!("Running application");
    let _res = run(
        config_loader,
        inputs_rx,
        effect_tx,
        tag_mapper,
        interpreter_state,
    )
    .unwrap();
    unreachable!();
}

fn run(
    config: ConfigLoaderHandle,
    input: Receiver<Input>,
    effect_tx: Sender<Effect>,
    tag_mapper: TagMapperHandle,
    interpreter_state: Arc<RwLock<InterpreterState>>,
) -> Result<()> {
    let mut player = Player::new(effect_tx.clone(), config.clone(), tag_mapper, interpreter_state)?;
    for input_ev in input {
        let res = process_ev(config.clone(), &mut player, input_ev.clone(), effect_tx.clone());
        match res {
            Err(err) => {
                error!("Failed to process input event {:?}: {}", input_ev, err);
            }
            Ok(effects) => {
                for effect in effects {
                    if let Err(err) = effect_tx.send(effect.clone()) {
                        error!("Failed to send output effect {:?}: {}", effect, err);
                    }
                }
            }
        }
    }
    unreachable!()
}

fn process_ev(
    config_loader: ConfigLoaderHandle,
    player: &mut Player,
    input: Input,
    _output: Sender<Effect>,
) -> Result<Vec<Effect>> {
    let config = config_loader.get();

    match input {
        Input::Button(cmd) => match cmd {
            button::Command::VolumeUp => {
                let cmd = config
                    .volume_up_command
                    .clone()
                    .unwrap_or_else(|| "pactl set-sink-volume 0 +10%".to_string());
                return Ok(vec![Effect::GenericCommand(cmd)]);
            }
            button::Command::VolumeDown => {
                let cmd = config
                    .volume_up_command
                    .clone()
                    .unwrap_or_else(|| "pactl set-sink-volume 0 -10%".to_string());
                return Ok(vec![Effect::GenericCommand(cmd)]);
            }
            button::Command::PauseContinue => {
                player.pause_continue_command()?;
                return Ok(vec![]);
            }
        },
        Input::Playback(request) => {
            player.playback(request.clone())?;
            return Ok(vec![]);
        }
    }
}
