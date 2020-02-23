use std::sync::{Arc, RwLock};

pub trait SpotifyConnector {
    fn request_restart(&mut self);
}

mod external_command {

    use super::*;

    use failure::{Context, Fallible};
    use slog_scope::{error, info, warn};
    use std::env;
    use std::process::{Child, Command};
    use std::sync::mpsc::{channel, Receiver, RecvTimeoutError, Sender};
    use std::thread;
    use std::time::Duration;

    enum SupervisorCommands {
        Restart,
        Terminate,
    }

    enum SupervisorStatus {
        NewDeviceId(String),
        Failure(String),
    }

    struct ExternalCommand {
        status: Receiver<SupervisorStatus>,
        command: Sender<SupervisorCommands>,
    }

    struct SupervisedCommand {
        pub cmd: String,
        pub device_name: String,
        pub command_receiver: Receiver<SupervisorCommands>,
        pub status_sender: Sender<SupervisorStatus>,
    }

    impl Drop for ExternalCommand {
        fn drop(&mut self) {
            self.command.send(SupervisorCommands::Terminate).unwrap();
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

        fn supervisor(self) {
            loop {
                loop {
                    info!("tick");
                    match self
                        .command_receiver
                        .recv_timeout(Duration::from_millis(1000))
                    {
                        Ok(cmd) => {
                            info!("Need to handle command");
                        }
                        Err(RecvTimeoutError::Timeout) => {
                            continue;
                        }
                        Err(_) => {
                            eprintln!("error");
                        }
                    }
                }
            }
        }
    }

    impl ExternalCommand {
        pub fn new(device_name: &str) -> Fallible<Self> {
            let cmd = env::var("SPOTIFY_CONNECT_COMMAND").map_err(Context::new)?;
            let (status_sender, status_receiver) = channel();
            let (command_sender, command_receiver) = channel();

            let supervised_cmd = SupervisedCommand {
                cmd: cmd.to_string().clone(),
                device_name: device_name.to_string().clone(),
                command_receiver: command_receiver,
                status_sender,
            };

            Ok(ExternalCommand {
                status: status_receiver,
                command: command_sender,
            })
        }
    }

    impl SpotifyConnector for ExternalCommand {
        fn request_restart(&mut self) {}
    }
}
