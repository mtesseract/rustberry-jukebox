use crossbeam_channel::{self, Receiver, RecvError, RecvTimeoutError, Select, Sender};
use failure::Fallible;
use serde::Deserialize;
use signal_hook::{iterator::Signals, SIGINT, SIGTERM};
use slog::{self, o, Drain};
use slog_async;
use slog_scope::{error, info, warn};
use slog_term;
use std::process::Command;

use rustberry::components::access_token_provider;
use rustberry::config::Config;
use rustberry::effects::Effects;
use rustberry::input_controller::{
    button,
    playback::{self, PlaybackRequest},
    Input,
};

// use rustberry::led_controller;
// use rustberry::spotify_connect::{self, SpotifyConnector, SupervisorStatus};
// use rustberry::spotify_play::{self, PlayerCommand};
// use rustberry::spotify_util;

// fn execute_shutdown(config: &Config) {
//     match config.shutdown_command {
//         Some(ref cmd) => {
//             Command::new(cmd)
//                 .status()
//                 .expect(&format!("failed to execute shutdown command '{}'", cmd));
//         }
//         None => {
//             Command::new("sudo")
//                 .arg("shutdown")
//                 .arg("-h")
//                 .arg("now")
//                 .status()
//                 .expect("failed to execute default shutdown command");
//         }
//     }
// }

// fn execute_volume_up(config: &Config) {
//     match config.volume_up_command {
//         Some(ref cmd) => {
//             Command::new(cmd)
//                 .status()
//                 .expect(&format!("failed to execute volume up command '{}'", cmd));
//         }
//         None => {
//             Command::new("amixer")
//                 .arg("-q")
//                 .arg("-M")
//                 .arg("set")
//                 .arg("PCM")
//                 .arg("5%+")
//                 .status()
//                 .expect("failed to execute default volume up command");
//         }
//     }
// }

// fn execute_volume_down(config: &Config) {
//     match config.volume_down_command {
//         Some(ref cmd) => {
//             Command::new(cmd)
//                 .status()
//                 .expect(&format!("failed to execute volume down command '{}'", cmd));
//         }
//         None => {
//             Command::new("amixer")
//                 .arg("-q")
//                 .arg("-M")
//                 .arg("set")
//                 .arg("PCM")
//                 .arg("5%-")
//                 .status()
//                 .expect("failed to execute default volume down command");
//         }
//     }
// }

fn main_with_log() -> Fallible<()> {
    let config = envy::from_env::<Config>()?;
    info!("Configuration"; o!("device_name" => &config.device_name));

    //// Prepare components.

    // Create Access Token Provider
    let mut access_token_provider = access_token_provider::AccessTokenProvider::new(
        &config.client_id,
        &config.client_secret,
        &config.refresh_token,
    );

    let player = unimplemented!();

    run_application(&config)
}

fn handle_inputs(inputs: &[Receiver<Input>]) {
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
                Input::Button(cmd) => {}
                Input::Playback(request) => match request {
                    Some(request) => {}
                    None => {}
                },
            },
        }
    }
}

fn run_application(config: &Config) -> Fallible<()> {
    let (tx, rx): (Sender<Effects>, Receiver<Effects>) = crossbeam_channel::bounded(2);

    // Prepare individual input channels.
    let button_controller_handle =
        button::cdev_gpio::CdevGpio::new_from_env(|cmd| Some(Input::Button(cmd)))?;
    let playback_controller_handle =
        playback::rfid::PlaybackRequestTransmitterRfid::new(|req| Some(req))?;

    loop {}

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
