use failure::Fallible;
use serde::Deserialize;
use signal_hook::{iterator::Signals, SIGINT, SIGTERM};
use slog::{self, o, Drain};
use slog_async;
use slog_scope::{error, info, warn};
use slog_term;
use std::process::Command;

use rustberry::access_token_provider;
// use rustberry::gpio_sysfs::{self, GpioController};
use rustberry::button_controller::{ButtonController, self};
use rustberry::playback_requests::{self, PlaybackRequest};
use rustberry::spotify_play;
use rustberry::spotify_util;

#[derive(Deserialize, Debug, Clone)]
struct Config {
    refresh_token: String,
    client_id: String,
    client_secret: String,
    device_name: String,
    post_init_command: Option<String>,
    shutdown_command: Option<String>,
}

fn execute_shutdown(config: &Config) {
    match config.shutdown_command {
        Some(ref cmd) => {
            Command::new(cmd)
                .status()
                .expect(&format!("failed to execute shutdown command '{}'", cmd));
        }
        None => {
            Command::new("sudo")
                .arg("shutdown")
                .arg("-h")
                .arg("now")
                .status()
                .expect("failed to execute default shutdown command");
        }
    }
}

fn run_application() -> Fallible<()> {
    info!("** Rustberry/Spotify Starting **");
    let config = envy::from_env::<Config>()?;
    info!("Configuration"; o!("device_name" => &config.device_name));

    // Create Access Token Provider
    let mut access_token_provider = access_token_provider::AccessTokenProvider::new(
        &config.client_id,
        &config.client_secret,
        &config.refresh_token,
    );

    let button_controller_backend = button_controller::backends::cdev_gpio::CdevGpio::new_from_env()?;
    let button_controller = ButtonController::new(button_controller_backend)?;
    info!("Created Button Controller");

    let config_copy = config.clone();
    std::thread::spawn(move || {
        for cmd in button_controller {
            info!("Received {:?} command from Button Controller", cmd);
            match cmd {
                button_controller::Command::Shutdown => {
                    info!("Shutting down");
                    execute_shutdown(&config_copy);
                }
            }
        }
    });

    std::thread::sleep(std::time::Duration::from_secs(2));

    let device = loop {
        match spotify_util::lookup_device_by_name(&mut access_token_provider, &config.device_name) {
            Err(err) => {
                warn!("Failed to lookup device, will retry: {}", err);
                std::thread::sleep(std::time::Duration::from_secs(5));
            }
            Ok(device) => {
                break device;
            }
        }
    };

    info!("Found device ID for device name"; o!("device_id" => &device.id));

    let mut player = spotify_play::Player::new(access_token_provider, &device.id);
    info!("Initialized Player");

    {
        let signals = Signals::new(&[SIGINT, SIGTERM])?;
        let mut player_clone = player.clone();
        std::thread::spawn(move || {
            let sig = signals.into_iter().next();
            info!("Received signal {:?}, exiting", sig);
            let _ = player_clone.stop_playback();
            std::process::exit(0);
        });
    }

    let transmitter_backend = playback_requests::rfid::PlaybackRequestTransmitterRfid::new()
        .expect("Failed to initialize backend");
    let transmitter = playback_requests::PlaybackRequestsTransmitter::new(transmitter_backend)
        .expect("Failed to create PlaybackRequestsTransmitter");
    let user_requests_producer: playback_requests::PlaybackRequests<PlaybackRequest> =
        playback_requests::PlaybackRequests::new(transmitter);

    // Execute post-init-command, if set in the environment.
    if let Some(ref post_init_command) = config.post_init_command {
        if let Err(err) = Command::new(post_init_command).output() {
            error!(
                "Failed to execute post init command '{}': {}",
                post_init_command, err
            );
        }
    }
    // Enter loop processing user requests (via RFID tag).
    user_requests_producer.for_each(|req| match req {
        Some(req) => {
            info!("Received playback request {:?}", &req);
            let res = match req {
                PlaybackRequest::SpotifyUri(ref uri) => player.start_playback(uri),
            };
            match res {
                Ok(_) => {
                    info!("Started playback: {:?}", &req);
                }
                Err(err) => {
                    error!("Failed to start playback: {}", err);
                }
            }
        }
        None => {
            info!("Stopping playback");
            match player.stop_playback() {
                Ok(_) => {
                    info!("Stopped playback");
                }
                Err(err) => {
                    error!("Failed to stop playback: {}", err);
                }
            }
        }
    });

    unreachable!()
}

fn main() -> Fallible<()> {
    let decorator = slog_term::TermDecorator::new().build();
    let drain = slog_term::FullFormat::new(decorator).build().fuse();
    let drain = slog_async::Async::new(drain).build().fuse();
    let logger = slog::Logger::root(drain, o!());
    let _guard = slog_scope::set_global_logger(logger);

    slog_scope::scope(&slog_scope::logger().new(o!()), || run_application())
}
