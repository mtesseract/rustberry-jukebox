use std::sync::{Arc, RwLock};

use crate::components::access_token_provider::AccessTokenProvider;
use crate::components::spotify::util;
use crossbeam_channel::Receiver;

pub enum SupervisorCommands {
    Terminate,
}

#[derive(Debug, Clone)]
pub enum SupervisorStatus {
    NewDeviceId(String),
    Failure(String),
}

pub trait SpotifyConnector {
    fn request_restart(&self);
}

pub mod external_command {

    use super::*;

    use crate::components::spotify::{self, util::JukeboxError};
    use failure::{Context, Fallible};
    use slog_scope::{error, info, warn};
    use std::env;
    use std::process::{Child, Command, ExitStatus};
    use std::thread::{self, JoinHandle};
    use std::time::Duration;

    use crossbeam_channel::{self, Receiver, RecvTimeoutError, Sender};

    pub struct ExternalCommand<T> {
        status: Receiver<T>,
        child: Arc<RwLock<Child>>,
        command: Sender<SupervisorCommands>,
        supervisor: JoinHandle<()>,
    }

    struct SupervisedCommand<T: Send> {
        pub cmd: String,
        pub device_name: String,
        pub command_receiver: Receiver<SupervisorCommands>,
        pub status_sender: Sender<T>,
        pub status_transformer: Box<Fn(SupervisorStatus) -> Option<T> + 'static + Send>,
        pub access_token_provider: AccessTokenProvider,
        child: Arc<RwLock<Child>>,
    }

    impl<T> Drop for ExternalCommand<T> {
        fn drop(&mut self) {
            let _ = self.command.send(SupervisorCommands::Terminate);
            let _ = self.child.write().unwrap().kill();
        }
    }

    impl<T: 'static + Send> SupervisedCommand<T> {
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
                        let found_device = match spotify::util::lookup_device_by_name(
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
                                let status = (self.status_transformer)(
                                    SupervisorStatus::NewDeviceId(found_device.id),
                                );
                                if let Some(status) = status {
                                    self.status_sender.send(status).unwrap();
                                }
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
        pub fn new<F>(
            cmd: String,
            device_name: &str,
            access_token_provider: &AccessTokenProvider,
            command_receiver: Receiver<SupervisorCommands>,
            status_sender: Sender<T>,
            status_transformer: F,
        ) -> Result<(Self, Arc<RwLock<Child>>), std::io::Error>
        where
            F: Fn(SupervisorStatus) -> Option<T> + 'static + Send,
        {
            let child = Command::new("sh").arg("-c").arg(&cmd).spawn()?;
            let rw_child = Arc::new(RwLock::new(child));
            let supervised_cmd = SupervisedCommand {
                cmd,
                device_name: device_name.to_string().clone(),
                command_receiver,
                status_sender,
                status_transformer: Box::new(status_transformer),
                access_token_provider: access_token_provider.clone(),
                child: Arc::clone(&rw_child),
            };
            Ok((supervised_cmd, rw_child))
        }
    }

    impl<T: Send + 'static> ExternalCommand<T> {
        pub fn status(&self) -> Receiver<T> {
            self.status.clone()
        }

        pub fn new_from_env<F>(
            access_token_provider: &AccessTokenProvider,
            device_name: String,
            status_transformer: F,
        ) -> Fallible<Self>
        where
            F: Fn(SupervisorStatus) -> Option<T> + 'static + Send,
        {
            let cmd = env::var("SPOTIFY_CONNECT_COMMAND").map_err(Context::new)?;
            Self::new(access_token_provider, cmd, device_name, status_transformer)
        }
        pub fn new<F>(
            access_token_provider: &AccessTokenProvider,
            cmd: String,
            device_name: String,
            status_transformer: F,
        ) -> Fallible<Self>
        where
            F: Fn(SupervisorStatus) -> Option<T> + 'static + Send,
        {
            let (status_sender, status_receiver) = crossbeam_channel::bounded(1);
            let (command_sender, command_receiver) = crossbeam_channel::bounded(1);

            // let (status_sender, status_receiver) = channel();
            // let (command_sender, command_receiver) = channel();

            let (supervised_cmd, rw_child) = SupervisedCommand::new(
                cmd.to_string().clone(),
                &device_name,
                access_token_provider,
                command_receiver,
                status_sender,
                status_transformer,
            )?;
            let supervisor = supervised_cmd.spawn_supervisor();

            Ok(ExternalCommand {
                status: status_receiver,
                child: rw_child,
                supervisor,
                command: command_sender,
            })
        }

        fn status_channel(&self) -> Receiver<T> {
            self.status.clone()
        }
    }

    impl<T> SpotifyConnector for ExternalCommand<T> {
        fn request_restart(&self) {
            if let Err(err) = self.child.write().unwrap().kill() {
                error!("While trying to restart Spotify Connector ExternalCommand, terminating the running process failed: {}", err);
            } else {
                error!("While trying to restart Spotify Connector ExternalCommand, successfully killed running process");
            }
        }
    }
}
