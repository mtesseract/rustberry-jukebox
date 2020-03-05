use failure::Fallible;
use serde::Deserialize;
use signal_hook::{iterator::Signals, SIGINT, SIGTERM};
use slog::{self, o, Drain};
use slog_async;
use slog_scope::{error, info, warn};
use slog_term;
use std::process::Command;

use rustberry::access_token_provider;
use rustberry::button_controller::{self, ButtonController};
use rustberry::led_controller;
use rustberry::playback_requests::{self, PlaybackRequest};
use rustberry::spotify_connect::{self, SpotifyConnector, SupervisorStatus};
use rustberry::spotify_play::{self, PlayerCommand};
use rustberry::spotify_util;

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

fn execute_volume_up(config: &Config) {
    match config.volume_up_command {
        Some(ref cmd) => {
            Command::new(cmd)
                .status()
                .expect(&format!("failed to execute volume up command '{}'", cmd));
        }
        None => {
            Command::new("amixer")
                .arg("-q")
                .arg("-M")
                .arg("set")
                .arg("PCM")
                .arg("5%+")
                .status()
                .expect("failed to execute default volume up command");
        }
    }
}

fn execute_volume_down(config: &Config) {
    match config.volume_down_command {
        Some(ref cmd) => {
            Command::new(cmd)
                .status()
                .expect(&format!("failed to execute volume down command '{}'", cmd));
        }
        None => {
            Command::new("amixer")
                .arg("-q")
                .arg("-M")
                .arg("set")
                .arg("PCM")
                .arg("5%-")
                .status()
                .expect("failed to execute default volume down command");
        }
    }
}

fn main_with_log() -> Fallible<()> {
    info!("** Rustberry/Spotify Starting **");
    let config = envy::from_env::<Config>()?;
    info!("Configuration"; o!("device_name" => &config.device_name));

    // Move to interpreter:
    //
    // // Create Access Token Provider
    // let mut access_token_provider = access_token_provider::AccessTokenProvider::new(
    //     &config.client_id,
    //     &config.client_secret,
    //     &config.refresh_token,
    // );

    /* Create input channels.
     */

    // 1. Create UserControlTransmitter.
    let button_controller_backend =
        button_controller::backends::cdev_gpio::CdevGpio::new_from_env()?;
    let user_control_transmitter = UserControlTransmitter::new(button_controller_backend);
    info!("Created UserControlTransmitter");

    // 2. Create PlayRequests (Transmitter).
    //
    let user_requests_transmitter: playback_requests::PlaybackRequests<PlaybackRequest> = {
        let transmitter_backend = playback_requests::rfid::PlaybackRequestTransmitterRfid::new()
            .expect("Failed to initialize backend");
        let transmitter = playback_requests::PlaybackRequestsTransmitter::new(transmitter_backend)
            .expect("Failed to create PlaybackRequestsTransmitter");
        playback_requests::PlaybackRequests::new(transmitter)
    };

    // let config_copy = config.clone();
    // std::thread::spawn(move || {
    //     for cmd in button_controller {
    //         info!("Received {:?} command from Button Controller", cmd);
    //         match cmd {
    //             button_controller::Command::Shutdown => {
    //                 info!("Shutting down");
    //                 execute_shutdown(&config_copy);
    //             }
    //             button_controller::Command::VolumeUp => {
    //                 info!("Volume up");
    //                 execute_volume_up(&config_copy);
    //             }
    //             button_controller::Command::VolumeDown => {
    //                 info!("Volume down");
    //                 execute_volume_down(&config_copy);
    //             }
    //         }
    //     }
    // });

    /*
     Create effect channel.
    */

    let (effects_tx, effects_rx) = crossbeam_channel::bounded(1);

    // run main application thread
    thread::spawn(|| run_application(effects_tx));
    // run interpreter on effects_rx;

    // Move to effects interpreter:
    //
    // // 1. Create LED Controller.
    // let mut led_controller = {
    //     let led_controller_backend = led_controller::backends::gpio_cdev::GpioCdev::new()?;
    //     led_controller::LedController::new(led_controller_backend)?
    // };

    // // std::thread::sleep(std::time::Duration::from_secs(2));

    // let spotify_connector = spotify_connect::external_command::ExternalCommand::new_from_env(
    //     &access_token_provider,
    //     config.device_name.clone(),
    //     |status| match status {
    //         SupervisorStatus::NewDeviceId(device_id) => Some(PlayerCommand::NewDeviceId(device_id)),
    //         other => {
    //             warn!("Ignoring SupervisorStatus {:?} in Player", other);
    //             None
    //         }
    //     },
    // )?;

    // let device = loop {
    //     match spotify_util::lookup_device_by_name(&mut access_token_provider, &config.device_name) {
    //         Err(err) => {
    //             warn!("Failed to lookup device, will retry: {}", err);
    //             std::thread::sleep(std::time::Duration::from_secs(5));
    //         }
    //         Ok(device) => {
    //             break device;
    //         }
    //     }
    // };

    // info!("Found device ID for device name"; o!("device_id" => &device.id));

    // let player = spotify_play::Player::new(access_token_provider, spotify_connector.status());
    // info!("Initialized Player");

    // {
    //     let signals = Signals::new(&[SIGINT, SIGTERM])?;
    //     let player_clone = player.clone();
    //     std::thread::spawn(move || {
    //         let sig = signals.into_iter().next();
    //         info!("Received signal {:?}, exiting", sig);
    //         let _ = player_clone.stop_playback();
    //         std::process::exit(0);
    //     });
    // }
}

fn run_application() -> Fallible<()> {
    // Execute post-init-command, if set in the environment.
    if let Some(ref post_init_command) = config.post_init_command {
        effects_tx
            .send(Effects::GenericCommand(post_init_command.clone()))
            .unwrap();
        // if let Err(err) = Command::new(post_init_command).output() {
        //     error!(
        //         "Failed to execute post init command '{}': {}",
        //         post_init_command, err
        //     );
        // }
    }

    // Enter loop processing user requests (via RFID tag).
    for req in user_requests_producer {
        match req {
            Some(req) => {
                info!("Received playback request {:?}", &req);
                led_controller.switch_on(led_controller::Led::Playback);
                let res = match req {
                    PlaybackRequest::SpotifyUri(ref uri) => player.start_playback(uri.to_string()),
                };
                match res {
                    Ok(_) => {
                        info!("Started playback: {:?}", &req);
                    }
                    Err(err) => {
                        led_controller.switch_off(led_controller::Led::Playback);
                        error!("Failed to start playback: {}", err);
                        if err.is_device_missing_error() {
                            warn!("No device ID found, restarting Spotify Connector");
                            spotify_connector.request_restart();
                        // FIXME: how to automatically retry the playbacj?
                        } else {
                            if err.is_client_error() {
                                warn!("Playback error is regarded as client error, application will terminate");
                                break;
                            }
                        }
                    }
                }
            }
            None => {
                info!("Stopping playback");
                match player.stop_playback() {
                    Ok(_) => {
                        info!("Stopped playback");
                        led_controller.switch_off(led_controller::Led::Playback);
                    }
                    Err(err) => {
                        error!("Failed to stop playback: {}", err);
                        if err.is_device_missing_error() {
                            warn!("No device ID found, restarting Spotify Connector");
                            spotify_connector.request_restart();
                        // FIXME: how to automatically retry the playback?
                        } else {
                            if err.is_client_error() {
                                warn!("Playback error is regarded as client error, application will terminate");
                                break;
                            }
                        }
                    }
                }
            }
        }
    }

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
