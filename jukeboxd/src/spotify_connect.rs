use std::sync::{Arc, RwLock};

use crate::access_token_provider::AccessTokenProvider;
use crate::spotify_util;

pub trait SpotifyConnector {
    fn request_restart(&mut self);
}

pub mod external_command {

    use super::*;

    use crate::spotify_util::JukeboxError;
    use failure::{Context, Fallible};
    use slog_scope::{error, info, warn};
    use std::env;
    use std::process::{Child, Command, ExitStatus};
    use std::sync::mpsc::{channel, Receiver, RecvTimeoutError, Sender};
    use std::thread::{self, JoinHandle};
    use std::time::Duration;

    enum SupervisorCommands {
        Terminate,
    }

    enum SupervisorStatus {
        NewDeviceId(String),
        Failure(String),
    }

    pub struct ExternalCommand {
        status: Receiver<SupervisorStatus>,
        child: Arc<RwLock<Child>>,
        command: Sender<SupervisorCommands>,
        supervisor: JoinHandle<()>,
    }

    struct SupervisedCommand {
        pub cmd: String,
        pub device_name: String,
        pub command_receiver: Receiver<SupervisorCommands>,
        pub status_sender: Sender<SupervisorStatus>,
        pub access_token_provider: AccessTokenProvider,
        child: Arc<RwLock<Child>>,
    }

    impl Drop for ExternalCommand {
        fn drop(&mut self) {
            let _ = self.command.send(SupervisorCommands::Terminate);
            let _ = self.child.write().unwrap().kill();
        }
    }

    impl SupervisedCommand {
        fn kill_child(&mut self) -> Result<(), std::io::Error> {
            self.child.write().unwrap().kill()
        }

        fn respawn(&mut self) -> Result<(), std::io::Error> {
            let child = Command::new("sh").arg("-c").arg(&self.cmd).spawn()?;
            *(self.child.write().unwrap()) = child;
            Ok(())
        }

        fn spawn_supervisor(self) -> JoinHandle<()> {
            info!("Spawning supervisor for Spotify Connect command");
            thread::spawn(move || Self::supervisor(self))
        }

        fn supervisor(mut self) {
            let mut device_id = None;

            loop {
                info!("supervisor tick");

                // Child is expected to be running.
                // Check if it has terminated for some reason:
                let res = {
                    let mut writer = self.child.write().unwrap();
                    writer.try_wait()
                };
                match res {
                    Ok(Some(status)) => {
                        // child terminated. needs to be restarted.
                        error!(
                            "Spotify Connector terminated unexpectedly with status {}",
                            status
                        );
                        if let Err(err) = self.respawn() {
                            error!("Failed to respawn Spotify Connector: {}", err);
                        } else {
                            info!("Respawned new Spotify Connector");
                        }
                    }
                    Ok(None) => {
                        // seems it is still running.
                        // check if device id can still be resolved.
                        let found_device = match spotify_util::lookup_device_by_name(
                            &self.access_token_provider,
                            &self.device_name,
                        ) {
                            Ok(device) => Some(device),
                            Err(JukeboxError::DeviceNotFound { .. }) => {
                                warn!("No Spotify device ID found for device name ...");
                                None
                            }
                            Err(err) => {
                                error!("Failed to lookup Spotify Device ID: {}", err);
                                // fixme, what to do here for resilience?
                                None
                            }
                        };

                        if let Some(found_device) = found_device {
                            let opt_found_device_id = Some(found_device.id.clone());
                            if opt_found_device_id != device_id {
                                // Device ID changed, send status update and note new device ID.
                                self.status_sender
                                    .send(SupervisorStatus::NewDeviceId(found_device.id))
                                    .unwrap();
                                device_id = opt_found_device_id;
                            } else {
                                // Device ID unchanged, nothing to do.
                            }
                        } else {
                            // No device found for name. Kill subprocess.
                            warn!("Failed to lookup device ID");
                            if let Err(err) = self.kill_child() {
                                error!("Failed to terminate Spotify Connector: {}", err);
                            } else {
                                info!("Terminated Spotify Connector");
                            }
                            if let Err(err) = self.respawn() {
                                error!("Failed to start new Spotify Connector: {}", err);
                            } else {
                                info!("Started new Spotify Connector");
                            }
                        }
                    }
                    Err(err) => {
                        error!(
                            "Failed to check if Spotify Connector is still running: {}",
                            err
                        );
                        // fixme, what to do for resilience?
                    }
                }

                match self
                    .command_receiver
                    .recv_timeout(Duration::from_millis(1000))
                {
                    Ok(SupervisorCommands::Terminate) => {
                        info!("Terminating Spotify Connect Supervisor");
                        break;
                    }
                    Err(RecvTimeoutError::Timeout) => {
                        continue;
                    }
                    Err(RecvTimeoutError::Disconnected) => {
                        error!("Supervisor's command channel disconnected, exiting supervisor");
                        break;
                    }
                }
            }
        }
        pub fn new(
            cmd: String,
            device_name: &str,
            access_token_provider: &AccessTokenProvider,
            command_receiver: Receiver<SupervisorCommands>,
            status_sender: Sender<SupervisorStatus>,
        ) -> Result<(Self, Arc<RwLock<Child>>), std::io::Error> {
            let child = Command::new("sh").arg("-c").arg(&cmd).spawn()?;
            let rw_child = Arc::new(RwLock::new(child));
            let supervised_cmd = SupervisedCommand {
                cmd,
                device_name: device_name.to_string().clone(),
                command_receiver,
                status_sender,
                access_token_provider: access_token_provider.clone(),
                child: Arc::clone(&rw_child),
            };
            Ok((supervised_cmd, rw_child))
        }
    }

    impl ExternalCommand {
        pub fn new_from_env(
            access_token_provider: &AccessTokenProvider,
            device_name: String,
        ) -> Fallible<Self> {
            let cmd = env::var("SPOTIFY_CONNECT_COMMAND").map_err(Context::new)?;
            Self::new(access_token_provider, cmd, device_name)
        }
        pub fn new(
            access_token_provider: &AccessTokenProvider,
            cmd: String,
            device_name: String,
        ) -> Fallible<Self> {
            let (status_sender, status_receiver) = channel();
            let (command_sender, command_receiver) = channel();

            let (supervised_cmd, rw_child) = SupervisedCommand::new(
                cmd.to_string().clone(),
                &device_name,
                access_token_provider,
                command_receiver,
                status_sender,
            )?;
            let supervisor = supervised_cmd.spawn_supervisor();

            Ok(ExternalCommand {
                status: status_receiver,
                child: rw_child,
                supervisor,
                command: command_sender,
            })
        }
    }

    impl SpotifyConnector for ExternalCommand {
        fn request_restart(&mut self) {}
    }
}
