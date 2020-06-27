use std::sync::{Arc, RwLock};
use std::thread;

use slog_scope::{error, info};
use async_trait::async_trait;

use crate::components::access_token_provider::AccessTokenProvider;

use super::util;

pub enum SupervisorCommands {
    Terminate,
}

#[async_trait]
pub trait SpotifyConnector {
    async fn wait_until_ready(&self) -> Result<(), util::JukeboxError>;
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
    use tokio::task;
    use futures::future::{abortable,AbortHandle};
    use std::time::Duration;

    pub struct ExternalCommand {
        device_id: Arc<RwLock<Option<String>>>,
        // status: Receiver<T>,
        child: Arc<RwLock<Child>>,
        // command: Sender<SupervisorCommands>,
        // _supervisor: JoinHandle<()>,
        abort_handle_device_id_watcher: AbortHandle,
        abort_handle_supervisor: AbortHandle,
    }

    impl Drop for ExternalCommand {
        fn drop(&mut self) {
            info!("Dropping ExternalCommand (Device ID Watcher and Supervisor)");
            self.abort_handle_device_id_watcher.abort();
            self.abort_handle_supervisor.abort();
        }
    }

    #[derive(Clone)]
    struct SupervisedCommand {
        pub cmd: String,
        pub device_name: String,
        pub username: String,
        pub password: String,
        pub cache_directory: String,
        pub device_id: Arc<RwLock<Option<String>>>,
        pub librespot_cmd: String,
        pub access_token_provider: AccessTokenProvider,
        child: Arc<RwLock<Child>>,
    }

    impl Drop for SupervisedCommand {
        fn drop(&mut self) {
            if let Err(err) = self.child.write().unwrap().kill() {
                error!(
                    "Failed to terminate supervised librespot while dropping SupervisedCommand: {}",
                    err
                );
            }
        }
    }

    impl SupervisedCommand {
        // fn kill_child(&mut self) -> Result<(), std::io::Error> {
        //     self.child.write().unwrap().kill()
        // }
        //

        fn spawn(
            username: &str,
            password: &str,
            device_name: &str,
            librespot_cmd: &str,
            cache_directory: &str,
        ) -> Result<Child, std::io::Error> {
            Command::new(librespot_cmd)
                .arg("--name")
                .arg(device_name)
                .arg("--username")
                .arg(username)
                .arg("--password")
                .arg(password)
                .arg("--bitrate")
                .arg("160")
                .arg("--cache")
                .arg(cache_directory)
                .arg("--enable-volume-normalisation")
                .arg("--linear-volume")
                .arg("--initial-volume=100")
                .spawn()
        }

        fn respawn(&mut self) -> Result<(), std::io::Error> {
            let child = Self::spawn(
                &self.username,
                &self.password,
                &self.device_name,
                &self.librespot_cmd,
                &self.cache_directory,
            )?;
            *(self.child.write().unwrap()) = child;
            Ok(())
        }

        async fn run_supervisor(supervised_cmd: SupervisedCommand) -> () {
            Self::supervisor(supervised_cmd)
        }

        async fn run_device_id_watcher(supervised_cmd: SupervisedCommand) -> () {
            let access_token_provider = Arc::new(supervised_cmd.access_token_provider.clone());
            let device_name = supervised_cmd.device_name.clone();
            let device_id = Arc::clone(&supervised_cmd.device_id);
            let child = Arc::clone(&supervised_cmd.child);
            tokio::time::delay_for(std::time::Duration::from_secs(2)).await;
            Self::device_id_watcher(access_token_provider, device_name, device_id, child)
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

        fn supervisor(mut self) -> () {
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
                            let pid = self.child.read().unwrap().id();
                            info!("Respawned new Spotify Connector (PID {})", pid);
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
            librespot_cmd: String,
            username: String,
            password: String,
            cache_directory: String,
            device_id: Arc<RwLock<Option<String>>>,
            access_token_provider: &AccessTokenProvider,
        ) -> Result<(Self, Arc<RwLock<Child>>), std::io::Error> {
            let child = Self::spawn(
                &username,
                &password,
                &device_name,
                &librespot_cmd,
                &cache_directory,
            )?;
            let rw_child = Arc::new(RwLock::new(child));
            let supervised_cmd = SupervisedCommand {
                cmd,
                device_name: device_name.to_string().clone(),
                access_token_provider: access_token_provider.clone(),
                child: Arc::clone(&rw_child),
                device_id,
                librespot_cmd,
                username,
                password,
                cache_directory,
            };
            Ok((supervised_cmd, rw_child))
        }
    }

    impl ExternalCommand {
        pub async fn new_from_env(
            access_token_provider: &AccessTokenProvider,
            device_name: String,
        ) -> Fallible<Self> {
            let cmd = env::var("SPOTIFY_CONNECT_COMMAND").map_err(Context::new)?;
            let username = env::var("SPOTIFY_CONNECT_USERNAME").map_err(Context::new)?;
            let password = env::var("SPOTIFY_CONNECT_PASSWORD").map_err(Context::new)?;
            let librespot_cmd = env::var("SPOTIFY_CONNECT_LIBRESPOT").map_err(Context::new)?;
            let cache_directory =
                env::var("SPOTIFY_CONNECT_CACHE_DIRECTORY").map_err(Context::new)?;
            Self::new(
                access_token_provider,
                cmd,
                device_name,
                username,
                password,
                cache_directory,
                librespot_cmd,
            ).await
        }
        pub async fn new(
            access_token_provider: &AccessTokenProvider,
            cmd: String,
            device_name: String,
            username: String,
            password: String,
            cache_directory: String,
            librespot_cmd: String,
        ) -> Fallible<Self> {
            let device_id = Arc::new(RwLock::new(None));
            let (supervised_cmd, rw_child) = SupervisedCommand::new(
                cmd.to_string().clone(),
                &device_name,
                librespot_cmd,
                username,
                password,
                cache_directory,
                Arc::clone(&device_id),
                access_token_provider,
            )?;
            let abort_handle_device_id_watcher = {
                info!("Spawning Device ID Watcher for Spotify Connect command");
                let supervised_cmd = supervised_cmd.clone();
                let (f, abort_handle) = abortable(SupervisedCommand::run_device_id_watcher(supervised_cmd));
                task::spawn(f);
                abort_handle
            };
            let abort_handle_supervisor = {
                info!("Spawning Supervisor for Spotify Connect command");
                let (f, abort_handle) = abortable(SupervisedCommand::run_supervisor(supervised_cmd));
                task::spawn(f);
                abort_handle
            };

            Ok(ExternalCommand {
                device_id,
                child: rw_child,
                abort_handle_device_id_watcher,
                abort_handle_supervisor,
            })
        }
    }

    #[async_trait]
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
        async fn wait_until_ready(&self) -> Result<(), util::JukeboxError> {
            let n_attempts = 30;
            for _idx in 0..n_attempts {
                if self.device_id().is_some() {
                    info!("Initial Device ID retrieved");
                    return Ok(());
                }
                tokio::time::delay_for(Duration::from_millis(500)).await;
            }
            error!("Failed to wait for initial Device ID");
            Err(util::JukeboxError::DeviceNotFound {
                device_name: "FIXME".to_string(),
            })
        }
        }
}
