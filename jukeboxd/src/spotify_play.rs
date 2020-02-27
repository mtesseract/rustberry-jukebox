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

use crossbeam_channel::{Receiver, RecvTimeoutError, Sender};

#[derive(Debug)]
pub enum Error {
    HTTP(reqwest::Error),
    NoToken,
    NoDevice,
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
        }
    }
}

impl From<reqwest::Error> for Error {
    fn from(err: reqwest::Error) -> Self {
        Error::HTTP(err)
    }
}

impl std::error::Error for Error {}

enum PlayerCommand {
    StartPlayback(spotify_uri: &str),
    StopPlayback,
    Terminate,
}

#[derive(Debug, Clone)]
pub struct PlayerHandle {
    handle: Arc<JoinHandle<()>>,
    commands: Sender<PlayerCommand>,
}

pub struct Player {
    device_id: Option<String>,
    access_token_provider: AccessTokenProvider,
    http_client: Client,
}

impl Drop for PlayerHandle {
    fn drop(&mut self) {
        println!("Destroying Player, stopping Music");
        let _ = self.stop_playback();
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
    fn player_thread(devicestatus_receiver: Receiver<SupervisorStatus>) {
        loop {

            info!("player tick");
            thread::sleep(std::time::Duration::from_secs(1));
        }
    }
}

impl Player {

    pub fn new(
        access_token_provider: AccessTokenProvider,
        spotify_connect_status: Receiver<SupervisorStatus>,
    ) -> Self {
        let http_client = Client::new();
        let handle = thread::spawn(|| Self::player_thread(spotify_connect_status));

        Player {
            access_token_provider,
            http_client,
            handle: Arc::new(handle),
            device_id: Arc::new(RwLock::new(None)),
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

    pub fn start_playback(&mut self, spotify_uri: &str) -> Result<(), Error> {
        let device_id = {
            match self.device_id.read().unwrap().clone() {
                Some(device_id) => device_id,
                None => return Err(Error::NoDevice),
            }
        };

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

    pub fn stop_playback(&mut self) -> Result<(), Error> {
        let device_id = {
            match self.device_id.read().unwrap().clone() {
                Some(device_id) => device_id,
                None => return Err(Error::NoDevice),
            }
        };

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
