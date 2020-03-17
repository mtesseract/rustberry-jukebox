use std::sync::{Arc, RwLock};
use std::thread;
use std::time::Duration;

use slog_scope::{error, info};

use crate::components::access_token_provider::AccessTokenProvider;

use super::util;

pub enum SupervisorCommands {
    Terminate,
}

pub trait SpotifyConnector {
    fn wait_until_ready(&self) -> Result<(), util::JukeboxError> {
        let n_attempts = 30;
        for _idx in 0..n_attempts {
            if self.device_id().is_some() {
                info!("Initial Device ID retrieved");
                return Ok(());
            }
            thread::sleep(Duration::from_millis(500));
        }
        error!("Failed to wait for initial Device ID");
        Err(util::JukeboxError::DeviceNotFound {
            device_name: "FIXME".to_string(),
        })
    }
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

    impl Drop for SupervisedCommand {
        fn drop(&mut self) {
            self.child.write().unwrap().kill();
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
            thread::Builder::new()
                .name("spotify-supervisor".to_string())
                .spawn(move || Self::supervisor(self))
                .unwrap()
        }

        fn spawn_device_id_watcher(&self) -> JoinHandle<()> {
            info!("Spawning device ID watcher for Spotify Connect command");
            let access_token_provider = Arc::new(self.access_token_provider.clone());
            let device_name = self.device_name.clone();
            let device_id = Arc::clone(&self.device_id);
            let child = Arc::clone(&self.child);
            thread::Builder::new()
                .name("spotify-device-watcher".to_string())
                .spawn(move || {
                    thread::sleep(Duration::from_secs(2));
                    Self::device_id_watcher(access_token_provider, device_name, device_id, child)
                })
                .unwrap()
        }

        fn device_id_watcher(
            access_token_provider: Arc<AccessTokenProvider>,
            device_name: String,
            device_id: Arc<RwLock<Option<String>>>,
            child: Arc<RwLock<Child>>,
        ) {
            loop {
                // info!("device ID watcher tick");
                // info!("Looking for device named '{}'", device_name);
                match spotify::util::lookup_device_by_name(&access_token_provider, &device_name) {
                    Ok(device) => {
                        info!("Device ID found: {}", device.id);
                        *(device_id.write().unwrap()) = Some(device.id);
                    }
                    Err(JukeboxError::DeviceNotFound { .. }) => {
                        warn!(
                            "No Spotify device ID found for device name '{}'",
                            device_name
                        );
                        *(device_id.write().unwrap()) = None;
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
                thread::sleep(Duration::from_millis(10000));
            }
        }

        fn supervisor(mut self) {
            loop {
                // info!("supervisor tick");

                // Child is expected to be running.
                // Check if it has terminated for some reason:
                let res = {
                    let mut writer = self.child.write().unwrap();
                    writer.try_wait()
                };
                match res {
                    Ok(Some(status)) => {
                        // child terminated. needs to be restarted.
                        warn!(
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
                thread::sleep(Duration::from_millis(2000));
            }
        }

        pub fn new(
            cmd: String,
            device_name: &str,
            device_id: Arc<RwLock<Option<String>>>,
            access_token_provider: &AccessTokenProvider,
        ) -> Result<(Self, Arc<RwLock<Child>>), std::io::Error> {
            let child = Command::new("sh").arg("-c").arg(&cmd).spawn()?;
            let rw_child = Arc::new(RwLock::new(child));
            let supervised_cmd = SupervisedCommand {
                cmd,
                device_name: device_name.to_string().clone(),
                access_token_provider: access_token_provider.clone(),
                child: Arc::clone(&rw_child),
                device_id,
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
                Arc::clone(&device_id),
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
