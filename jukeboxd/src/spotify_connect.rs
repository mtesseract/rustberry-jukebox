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
    use std::thread;
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
        fn spawn_cmd(&self) -> Result<Child, std::io::Error> {
            Command::new("sh").arg("-c").arg(&self.cmd).spawn()
        }

        fn spawn_supervisor(self) -> Fallible<()> {
            info!("Spawning supervisor for Spotify Connect command");
            let handle = thread::spawn(move || Self::supervisor(self));
            Ok(())
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

                /*

                sources:
                - device id can be resolved
                // - command received
                - process terminated/active

                */

                let child_running = {
                    let opt_child = self.child.read().unwrap();
                    opt_child.is_some()
                };

                if child_running {
                    // Child is expected to be running.
                    // Check if it has terminated for some reason:
                    match self.try_wait() {
                        Ok(Some(status)) => {
                            // child terminated.
                            error!(
                                "Spotify Connector terminated unexpectedly with status {}",
                                status
                            );
                        }
                        Ok(None) => {
                            // seems it is still running. check if device id can still be resolved.
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
                                if Some(found_device.id) != device_id {
                                    self.status_sender
                                        .send(SupervisorStatus::NewDeviceId(found_device.id))
                                        .unwrap();
                                    device_id = Some(found_device.id);
                                }
                            } else {
                                // No device found for name.
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
                    match self.spawn_cmd() {
                        Ok(child) => {
                            // New child running, remember it.
                            let mut opt_child = self.child.write().unwrap();
                            *opt_child = Some(child);
                        }
                        Err(err) => {
                            error!("Failed to spawn Spotify Connector: {}", err);
                        }
                    }
                }

                // match self
                //     .command_receiver
                //     .recv_timeout(Duration::from_millis(1000))
                // {
                //     Ok(cmd) => {
                //         info!("Need to handle command");
                //     }
                //     Err(RecvTimeoutError::Timeout) => {
                //         continue;
                //     }
                //     Err(_) => {
                //         eprintln!("error");
                //     }
                // }
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

            Ok(ExternalCommand {
                status: status_receiver,
                child,
            })
        }
    }

    impl SpotifyConnector for ExternalCommand {
        fn request_restart(&mut self) {}
    }
}
