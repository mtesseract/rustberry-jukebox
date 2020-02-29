use std::fmt::{self, Display};
use std::thread::{self, JoinHandle};

use crate::access_token_provider::{self, AccessTokenProvider, AtpError};

use crate::spotify_connect::{SpotifyConnector, SupervisorCommands, SupervisorStatus};
use hyper::header::AUTHORIZATION;
use reqwest::Client;
use serde::Serialize;
use slog_scope::{error, info, warn};
use std::convert::From;
use std::sync::{Arc, RwLock};

use crossbeam_channel::{Receiver, RecvError, RecvTimeoutError, Select, Sender};
//use crossbeam_channel::{Receiver, RecvError, Select};

#[derive(Debug)]
pub enum Error {
    HTTP(reqwest::Error),
    NoToken,
    NoDevice,
    SendError(String),
}

impl From<access_token_provider::AtpError> for Error {
    fn from(err: access_token_provider::AtpError) -> Error {
        match err {
            AtpError::NoTokenReceivedYet => Error::NoToken,
        }
    }
}
impl Error {
    pub fn is_client_error(&self) -> bool {
        match self {
            Error::HTTP(err) => err.status().map(|s| s.is_client_error()).unwrap_or(false),
            Error::NoToken => true,
            Error::NoDevice => true,
            Error::SendError(_) => false,
        }
    }
    pub fn is_device_missing_error(&self) -> bool {
        match self {
            Error::NoDevice => true,
            _ => false,
        }
    }
}

impl Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::HTTP(err) => write!(f, "Spotify HTTP Error {}", err),
            Error::NoToken => write!(f, "Failed to obtain access token"),
            Error::NoDevice => write!(f, "No Spotify Connect Device found"),
            Error::SendError(err) => write!(f, "Failed to transmit command via channel: {}", err),
        }
    }
}

impl From<reqwest::Error> for Error {
    fn from(err: reqwest::Error) -> Self {
        Error::HTTP(err)
    }
}

impl<T> From<crossbeam_channel::SendError<T>> for Error {
    fn from(err: crossbeam_channel::SendError<T>) -> Self {
        Error::SendError(err.to_string())
    }
}
impl std::error::Error for Error {}

#[derive(Debug, Clone)]
pub enum PlayerCommand {
    StartPlayback { spotify_uri: String },
    StopPlayback,
    Terminate,
    NewDeviceId(String),
}

#[derive(Debug, Clone)]
pub struct PlayerHandle {
    handle: Arc<JoinHandle<()>>,
    commands: Sender<PlayerCommand>,
}

pub struct Player {
    access_token_provider: AccessTokenProvider,
    http_client: Client,
    commands: Receiver<PlayerCommand>,
    status: Receiver<PlayerCommand>,
}

impl Drop for PlayerHandle {
    fn drop(&mut self) {
        println!("Destroying Player, stopping Music");
        // let _ = self.stop_playback();
    }
}

#[derive(Debug, Clone, Serialize)]
struct StartPlayback {
    #[serde(skip_serializing_if = "Option::is_none")]
    context_uri: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    uris: Option<Vec<String>>,
}

impl PlayerHandle {
    pub fn stop_playback(&self) -> Result<(), Error> {
        self.commands.send(PlayerCommand::StopPlayback)?;
        Ok(())
    }

    pub fn start_playback(&self, spotify_uri: String) -> Result<(), Error> {
        self.commands
            .send(PlayerCommand::StartPlayback { spotify_uri })?;
        Ok(())
    }
}

impl Player {
    fn main(self) {
        use PlayerCommand::*;
        let mut device_id: Option<String> = None;

        let rs = vec![self.commands.clone(), self.status.clone()];
        // Build a list of operations.
        let mut sel = Select::new();
        for r in &rs {
            sel.recv(r);
        }

        loop {
            // Wait until a receive operation becomes ready and try executing it.
            let index = sel.ready();
            let res = rs[index].try_recv();

            match res {
                Err(err) => {
                    if err.is_empty() {
                        // If the operation turns out not to be ready, retry.
                        continue;
                    } else {
                    }
                }
                Ok(msg) => match msg {
                    StartPlayback { spotify_uri } => {
                        if let Some(ref device_id) = device_id {
                            match self.start_playback(device_id, &spotify_uri) {
                                Err(err) => {}
                                Ok(()) => {}
                            }
                        } else {
                            // no device id
                        }
                    }
                    StopPlayback => {
                        if let Some(ref device_id) = device_id {
                            match self.stop_playback(device_id) {
                                Err(err) => {}
                                Ok(()) => {}
                            }
                        } else {
                            // no device id
                        }
                    }
                    Terminate => {
                        info!("Player received Terminate command, terminating");
                        break;
                    }
                    NewDeviceId(new_device_id) => {
                        device_id = Some(new_device_id);
                    }
                },
            }
        }
    }

    pub fn new(
        access_token_provider: AccessTokenProvider,
        spotify_connect_status: Receiver<PlayerCommand>,
    ) -> PlayerHandle {
        let http_client = Client::new();
        let (commands_tx, commands_rx) = crossbeam_channel::bounded(1);

        let player = Player {
            access_token_provider,
            http_client,
            commands: commands_rx,
            status: spotify_connect_status,
        };

        let handle = thread::spawn(|| player.main());

        PlayerHandle {
            handle: Arc::new(handle),
            commands: commands_tx,
        }
    }

    fn derive_start_playback_payload_from_spotify_uri(spotify_uri: &str) -> StartPlayback {
        if &spotify_uri[0..14] == "spotify:album:" {
            StartPlayback {
                uris: None,
                context_uri: Some(spotify_uri.clone().to_string()),
            }
        } else {
            StartPlayback {
                uris: Some(vec![spotify_uri.clone().to_string()]),
                context_uri: None,
            }
        }
    }

    fn start_playback(&self, device_id: &str, spotify_uri: &str) -> Result<(), Error> {
        let access_token = self.access_token_provider.get_bearer_token()?;
        let msg = "Failed to start Spotify playback";
        let req = Self::derive_start_playback_payload_from_spotify_uri(spotify_uri);
        self.http_client
            .put("https://api.spotify.com/v1/me/player/play")
            .query(&[("device_id", &device_id)])
            .header(AUTHORIZATION, &access_token)
            .json(&req)
            .send()
            .map_err(|err| {
                error!("{}: Executing HTTP request failed: {}", msg, err);
                err
            })
            .map(|mut rsp| {
                if !rsp.status().is_success() {
                    error!("{}: HTTP Failure {}: {:?}", msg, rsp.status(), rsp.text());
                }
                rsp
            })?
            .error_for_status()
            .map(|_| ())
            .map_err(|err| Error::HTTP(err))
    }

    fn stop_playback(&self, device_id: &str) -> Result<(), Error> {
        let access_token = self.access_token_provider.get_bearer_token()?;
        let msg = "Failed to stop Spotify playback";
        self.http_client
            .put("https://api.spotify.com/v1/me/player/pause")
            .query(&[("device_id", &device_id)])
            .body("")
            .header(AUTHORIZATION, &access_token)
            .send()
            .map_err(|err| {
                error!("{}: Executing HTTP request failed: {}", msg, err);
                err
            })
            .map(|mut rsp| {
                if !rsp.status().is_success() {
                    error!("{}: HTTP Failure {}: {:?}", msg, rsp.status(), rsp.text());
                }
                rsp
            })?
            .error_for_status()
            .map(|_| ())
            .map_err(|err| Error::HTTP(err))
    }
}
