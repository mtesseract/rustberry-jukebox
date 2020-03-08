use std::thread;

use crossbeam_channel::{self, Receiver, Select, Sender};
use failure::Fallible;
use slog::{self, o, Drain};
use slog_async;
use slog_scope::{error, info, warn};
use slog_term;

use rustberry::config::Config;
use rustberry::effects::Effects;
use rustberry::effects::ProdInterpreter;
use rustberry::input_controller::{
    button,
    playback::{self},
    Input,
};
use rustberry::player::{self, Player};

fn main() -> Fallible<()> {
    let decorator = slog_term::TermDecorator::new().build();
    let drain = slog_term::FullFormat::new(decorator).build().fuse();
    let drain = slog_async::Async::new(drain).build().fuse();
    let logger = slog::Logger::root(drain, o!());
    let _guard = slog_scope::set_global_logger(logger);

    slog_scope::scope(&slog_scope::logger().new(o!()), || main_with_log())
}

fn main_with_log() -> Fallible<()> {
    let config = envy::from_env::<Config>()?;
    info!("Configuration"; o!("device_name" => &config.device_name));

    // Create Effects Channel and Interpreter.
    let (tx, rx): (Sender<Effects>, Receiver<Effects>) = crossbeam_channel::bounded(2);
    let interpreter = ProdInterpreter::new(&config).unwrap();

    // Run Interpreter.
    thread::spawn(move || interpreter.run(rx));

    // Create Player component.
    let player_handle = Player::new(tx.clone());

    // Prepare individual input channels.
    let button_controller_handle =
        button::cdev_gpio::CdevGpio::new_from_env(|cmd| Some(Input::Button(cmd)))?;
    let playback_controller_handle =
        playback::rfid::PlaybackRequestTransmitterRfid::new(|req| Some(Input::Playback(req)))?;

    // Execute Application Logic, producing Effects.
    run_application(
        &config,
        player_handle,
        &vec![
            button_controller_handle.channel(),
            playback_controller_handle.channel(),
        ],
        tx,
    );
    warn!("Jukebox loop terminated, terminating application");
    unreachable!()
}

fn run_application(
    config: &Config,
    player_handle: player::Handle,
    inputs: &[Receiver<Input>],
    effects: Sender<Effects>,
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
                    // FIXME
                    error!("Failed to receive input event: {}", err);
                    continue;
                }
            }
            Ok(input) => match input {
                Input::Button(cmd) => match cmd {
                    button::Command::Shutdown => effects
                        .send(Effects::GenericCommand(
                            config
                                .shutdown_command
                                .clone()
                                .unwrap_or("shutdown -h now".to_string()),
                        ))
                        .unwrap(),
                    button::Command::VolumeUp => {
                        effects
                            .send(Effects::GenericCommand(
                                config
                                    .volume_up_command
                                    .clone()
                                    .unwrap_or("amixer -q -M set PCM 10%+".to_string()),
                            ))
                            .unwrap();
                    }
                    button::Command::VolumeDown => {
                        effects
                            .send(Effects::GenericCommand(
                                config
                                    .volume_up_command
                                    .clone()
                                    .unwrap_or("amixer -q -M set PCM 10%-".to_string()),
                            ))
                            .unwrap();
                    }
                },
                Input::Playback(request) => {
                    player_handle.playback(request).unwrap();
                }
            },
        }
    }
}
