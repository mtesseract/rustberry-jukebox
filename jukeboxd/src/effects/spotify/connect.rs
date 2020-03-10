use std::sync::{Arc, RwLock};

use crate::components::access_token_provider::AccessTokenProvider;

pub enum SupervisorCommands {
    Terminate,
}

pub trait SpotifyConnector {
    fn device_id(&self) -> Option<String>;
    fn request_restart(&self);
}

pub mod external_command {

    use super::*;

    use crate::effects::spotify::{self, util::JukeboxError};
    use failure::{Context, Fallible};
    use slog_scope::{error, info, warn};
    use std::env;
    use std::process::{Child, Command};
    use std::thread::{self, JoinHandle};
    use std::time::Duration;

    use crossbeam_channel::{self, Receiver, RecvTimeoutError, Sender};

    pub struct ExternalCommand {
        device_id: Arc<RwLock<Option<String>>>,
        // status: Receiver<T>,
        child: Arc<RwLock<Child>>,
        // command: Sender<SupervisorCommands>,
        _supervisor: JoinHandle<()>,
    }

    struct SupervisedCommand {
        pub cmd: String,
        pub device_name: String,
        pub device_id: Arc<RwLock<Option<String>>>,
        pub access_token_provider: AccessTokenProvider,
        child: Arc<RwLock<Child>>,
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

        fn spawn_device_id_watcher(&self) -> JoinHandle<()> {
            info!("Spawning device ID watcher for Spotify Connect command");
            let access_token_provider = Arc::new(self.access_token_provider.clone());
            let device_name = self.device_name.clone();
            let device_id = Arc::clone(&self.device_id);
            let child = Arc::clone(&self.child);
            thread::spawn(move || {
                Self::device_id_watcher(access_token_provider, device_name, device_id, child)
            })
        }

        fn device_id_watcher(
            access_token_provider: Arc<AccessTokenProvider>,
            device_name: String,
            device_id: Arc<RwLock<Option<String>>>,
            child: Arc<RwLock<Child>>,
        ) {
            loop {
                info!("device ID watcher tick");
                match spotify::util::lookup_device_by_name(&access_token_provider, &device_name) {
                    Ok(device) => {
                        *(device_id.write().unwrap()) = Some(device.id);
                    }
                    Err(JukeboxError::DeviceNotFound { .. }) => {
                        warn!("No Spotify device ID found for device name ...");
                        // kill child
                        if let Err(err) = child.write().unwrap().kill() {
                            error!("Failed to terminate Spotify Connector: {}", err);
                        } else {
                            info!("Terminated Spotify Connector");
                        }
                    }
                    Err(err) => {
                        error!("Failed to lookup Spotify Device ID: {}", err);
                        // fixme, what to do here for resilience?
                    }
                }
                thread::sleep(Duration::from_millis(2000));
            }
        }

        fn supervisor(mut self) {
            loop {
                info!("supervisor tick");

                // Child is expected to be running.
                // Check if it has terminated for some reason:
                let res = {
                    let mut writer = self.child.write().unwrap();
                    writer.try_wait()
                };
                dbg!(&res);
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
                    Ok(None) => {}
                    Err(err) => {
                        error!(
                            "Failed to check if Spotify Connector is still running: {}",
                            err
                        );
                        // fixme, what to do for resilience?
                    }
                }

                thread::sleep(Duration::from_millis(1000));
            }
        }

        pub fn new(
            cmd: String,
            device_name: &str,
            access_token_provider: &AccessTokenProvider,
        ) -> Result<(Self, Arc<RwLock<Child>>), std::io::Error> {
            let child = Command::new("sh").arg("-c").arg(&cmd).spawn()?;
            let rw_child = Arc::new(RwLock::new(child));
            let supervised_cmd = SupervisedCommand {
                cmd,
                device_name: device_name.to_string().clone(),
                access_token_provider: access_token_provider.clone(),
                child: Arc::clone(&rw_child),
                device_id: Arc::new(RwLock::new(None)),
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
            let device_id = Arc::new(RwLock::new(None));
            let (supervised_cmd, rw_child) = SupervisedCommand::new(
                cmd.to_string().clone(),
                &device_name,
                access_token_provider,
            )?;
            let _ = supervised_cmd.spawn_device_id_watcher();
            let supervisor = supervised_cmd.spawn_supervisor();

            Ok(ExternalCommand {
                device_id,
                child: rw_child,
                _supervisor: supervisor,
            })
        }
    }

    impl SpotifyConnector for ExternalCommand {
        fn request_restart(&self) {
            if let Err(err) = self.child.write().unwrap().kill() {
                error!("While trying to restart Spotify Connector ExternalCommand, terminating the running process failed: {}", err);
            } else {
                error!("While trying to restart Spotify Connector ExternalCommand, successfully killed running process");
            }
        }
        fn device_id(&self) -> Option<String> {
            let reader = self.device_id.read().unwrap();
            (*reader).clone()
        }
    }
}
