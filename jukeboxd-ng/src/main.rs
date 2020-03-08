use crossbeam_channel::{self, Receiver, RecvError, RecvTimeoutError, Select, Sender};
use failure::Fallible;
use serde::Deserialize;
use signal_hook::{iterator::Signals, SIGINT, SIGTERM};
use slog::{self, o, Drain};
use slog_async;
use slog_scope::{error, info, warn};
use slog_term;
use std::process::Command;
use std::thread;

use rustberry::components::access_token_provider;
use rustberry::config::Config;
use rustberry::effects::spotify::connect::{self, SpotifyConnector, SupervisorCommands};
use rustberry::effects::Effects;
use rustberry::effects::ProdInterpreter;
use rustberry::input_controller::{
    button,
    playback::{self},
    Input,
};
use rustberry::player::{self, PlaybackRequest, Player, PlayerCommand, PlayerHandle};

fn execute_shutdown(config: &Config, effects: &Sender<Effects>) {
    let cmd = match config.shutdown_command {
        Some(ref cmd) => cmd.clone(),
        None => "sudo shutdown -h now".to_string(),
    };
    effects.send(Effects::GenericCommand(cmd)).unwrap();
}

fn execute_volume_up(config: &Config, effects: &Sender<Effects>) {
    let cmd = config
        .volume_up_command
        .clone()
        .unwrap_or("amixer -q -M set PCM 10%+".to_string());
    effects.send(Effects::GenericCommand(cmd)).unwrap();
}

fn execute_volume_down(config: &Config, effects: &Sender<Effects>) {
    let cmd = config
        .volume_up_command
        .clone()
        .unwrap_or("amixer -q -M set PCM 10%-".to_string());
    effects.send(Effects::GenericCommand(cmd)).unwrap();
}

fn main_with_log() -> Fallible<()> {
    let config = envy::from_env::<Config>()?;
    info!("Configuration"; o!("device_name" => &config.device_name));

    //// Prepare components.

    let (tx, rx): (Sender<Effects>, Receiver<Effects>) = crossbeam_channel::bounded(2);
    let player_handle = Player::new(tx.clone());

    let interpreter = ProdInterpreter::new(&config).unwrap();
    thread::spawn(move || interpreter.run(rx));
    run_application(player_handle, tx, &config)
}

fn handle_inputs(
    config: &Config,
    player_handle: PlayerHandle,
    effects: &Sender<Effects>,
    inputs: &[Receiver<Input>],
) {
    let mut sel = Select::new();
    for r in inputs {
        sel.recv(r);
    }

    loop {
        // Wait until a receive operation becomes ready and try executing it.
        let index = sel.ready();
        let res = inputs[index].try_recv();

        match res {
            Err(err) => {
                if err.is_empty() {
                    // If the operation turns out not to be ready, retry.
                    continue;
                } else {
                    error!("Failed to receive input event: {}", err);
                    continue;
                }
            }
            Ok(input) => match input {
                Input::Button(cmd) => match cmd {
                    button::Command::Shutdown => execute_shutdown(config, effects),
                    button::Command::VolumeUp => execute_volume_up(config, effects),
                    button::Command::VolumeDown => execute_volume_down(config, effects),
                },
                Input::Playback(request) => {
                    player_handle.playback(request).unwrap();
                }
            },
        }
    }
}

fn run_application(
    player_handle: PlayerHandle,
    effects: Sender<Effects>,
    config: &Config,
) -> Fallible<()> {
    // Prepare individual input channels.
    let button_controller_handle =
        button::cdev_gpio::CdevGpio::new_from_env(|cmd| Some(Input::Button(cmd)))?;
    let playback_controller_handle =
        playback::rfid::PlaybackRequestTransmitterRfid::new(|req| Some(Input::Playback(req)))?;

    handle_inputs(
        config,
        player_handle,
        &effects,
        &vec![
            button_controller_handle.channel(),
            playback_controller_handle.channel(),
        ],
    );

    warn!("Jukebox loop terminated, terminating application");
    Ok(())
}

fn main() -> Fallible<()> {
    let decorator = slog_term::TermDecorator::new().build();
    let drain = slog_term::FullFormat::new(decorator).build().fuse();
    let drain = slog_async::Async::new(drain).build().fuse();
    let logger = slog::Logger::root(drain, o!());
    let _guard = slog_scope::set_global_logger(logger);

    slog_scope::scope(&slog_scope::logger().new(o!()), || main_with_log())
}
