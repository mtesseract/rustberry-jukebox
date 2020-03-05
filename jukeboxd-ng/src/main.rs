use failure::Fallible;
use serde::Deserialize;
use signal_hook::{iterator::Signals, SIGINT, SIGTERM};
use slog::{self, o, Drain};
use slog_async;
use slog_scope::{error, info, warn};
use slog_term;
use std::process::Command;

use rustberry::components::access_token_provider;
use rustberry::input_controller::{
    button,
    playback::{self, PlaybackRequest},
};
// use rustberry::led_controller;
// use rustberry::spotify_connect::{self, SpotifyConnector, SupervisorStatus};
// use rustberry::spotify_play::{self, PlayerCommand};
// use rustberry::spotify_util;

#[derive(Deserialize, Debug, Clone)]
struct Config {
    refresh_token: String,
    client_id: String,
    client_secret: String,
    device_name: String,
    post_init_command: Option<String>,
    shutdown_command: Option<String>,
    volume_up_command: Option<String>,
    volume_down_command: Option<String>,
}

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
    Ok(())
}

fn run_application() -> Fallible<()> {
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
