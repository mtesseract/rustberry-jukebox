use std::sync::{Arc, RwLock};

use crate::access_token_provider::AccessTokenProvider;
use crate::spotify_util;

pub trait SpotifyConnector {
    fn request_restart(&mut self);
}

mod external_command {

    use super::*;

    use crate::spotify_util::JukeboxError;
    use failure::{Context, Fallible};
    use slog_scope::{error, info, warn};
    use std::env;
    use std::process::{Child, Command, ExitStatus};
    use std::sync::mpsc::{channel, Receiver, RecvTimeoutError, Sender};
    use std::thread::{self, JoinHandle};
    use std::time::Duration;

    // enum SupervisorCommands {
    //     Restart,
    //     Terminate,
    // }

    enum SupervisorStatus {
        NewDeviceId(String),
        Failure(String),
    }

    struct ExternalCommand {
        status: Receiver<SupervisorStatus>,
        child: Arc<RwLock<Option<Child>>>,
        // command: Sender<SupervisorCommands>,
        supervisor: JoinHandle<()>,
    }

    struct SupervisedCommand {
        pub cmd: String,
        pub device_name: String,
        // pub command_receiver: Receiver<SupervisorCommands>,
        pub status_sender: Sender<SupervisorStatus>,
        pub access_token_provider: AccessTokenProvider,
        child: Arc<RwLock<Option<Child>>>,
    }

    impl Drop for ExternalCommand {
        fn drop(&mut self) {
            if let Some(ref mut child) = *(self.child.write().unwrap()) {
                child.kill();
            }
        }
    }

    impl SupervisedCommand {
        fn kill_child(&mut self) -> Result<(), std::io::Error> {
            let mut opt_child = self.child.write().unwrap();
            if let Some(ref mut child) = &mut *opt_child {
                child.kill();
                *opt_child = None;
            }
            Ok(())
        }

        fn spawn_cmd(&mut self) -> Result<(), std::io::Error> {
            let child = Command::new("sh").arg("-c").arg(&self.cmd).spawn()?;
            let mut opt_child = self.child.write().unwrap();
            *opt_child = Some(child);
            Ok(())
        }

        fn spawn_supervisor(self) -> JoinHandle<()> {
            info!("Spawning supervisor for Spotify Connect command");
            thread::spawn(move || Self::supervisor(self))
        }

        fn try_wait(&mut self) -> Result<Option<ExitStatus>, std::io::Error> {
            let mut opt_child = self.child.write().unwrap();
            match *opt_child {
                Some(ref mut child) => {
                    let res = child.try_wait();
                    if let Ok(Some(_)) = res {
                        *opt_child = None;
                    }
                    res
                }
                None => Ok(None),
            }
        }

        fn supervisor(mut self) {
            let mut device_id = None;

            loop {
                info!("supervisor tick");

                let child_running = {
                    let opt_child = self.child.read().unwrap();
                    opt_child.is_some()
                };

                if child_running {
                    // Child is expected to be running.
                    // Check if it has terminated for some reason:
                    match self.try_wait() {
                        Ok(Some(status)) => {
                            // child terminated. needs to be restarted.
                            error!(
                                "Spotify Connector terminated unexpectedly with status {}",
                                status
                            );
                            if let Err(err) = self.spawn_cmd() {
                                error!("Failed to start Spotify Connector: {}", err);
                            } else {
                                info!("Spawned new Spotify Connector");
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
                                if let Err(err) = self.spawn_cmd() {
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
                } else {
                    // No child known to be running.
                    if let Err(err) = self.spawn_cmd() {
                        error!("Failed to spawn Spotify Connector: {}", err);
                    }
                }

                thread::sleep(Duration::from_millis(1000));
            }
        }
    }

    impl ExternalCommand {
        pub fn new(
            access_token_provider: &AccessTokenProvider,
            device_name: &str,
        ) -> Fallible<Self> {
            let cmd = env::var("SPOTIFY_CONNECT_COMMAND").map_err(Context::new)?;
            let (status_sender, status_receiver) = channel();
            // let (command_sender, command_receiver) = channel();
            let child = Arc::new(RwLock::new(None));
            let supervised_cmd = SupervisedCommand {
                cmd: cmd.to_string().clone(),
                device_name: device_name.to_string().clone(),
                // command_receiver: command_receiver,
                status_sender,
                access_token_provider: access_token_provider.clone(),
                child: Arc::clone(&child),
            };
            let supervisor = supervised_cmd.spawn_supervisor();

            Ok(ExternalCommand {
                status: status_receiver,
                child,
                supervisor,
            })
        }
    }

    impl SpotifyConnector for ExternalCommand {
        fn request_restart(&mut self) {}
    }
}
