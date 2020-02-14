use std::fmt::{self, Display};

use crate::access_token_provider::AccessTokenProvider;

use hyper::header::AUTHORIZATION;
use reqwest::Client;
use serde::Serialize;
use slog_scope::error;
use std::convert::From;

#[derive(Debug)]
pub enum Error {
    HTTP(reqwest::Error),
}

pub trait PlaybackError {
    fn is_client_error(&self) -> bool;
}

impl PlaybackError for Error {
    fn is_client_error(&self) -> bool {
        let status = match self {
            Error::HTTP(err) => err.status(),
        };
        status.map(|s| s.is_client_error()).unwrap_or(false)
    }
}

impl Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::HTTP(err) => write!(f, "Spotify HTTP Error {}", err),
        }
    }
}

impl From<reqwest::Error> for Error {
    fn from(err: reqwest::Error) -> Self {
        Error::HTTP(err)
    }
}

impl std::error::Error for Error {}

#[derive(Debug, Clone)]
pub struct Player {
    device_id: String,
    access_token_provider: AccessTokenProvider,
    http_client: Client,
}

impl Drop for Player {
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

// #[derive(Debug, Clone, Serialize)]
// struct TransferPlayback {
//     play: bool,
//     device_ids: Vec<String>,
//     context_uri: String,
// }

impl Player {
    pub fn new(access_token_provider: AccessTokenProvider, device_id: &str) -> Self {
        let http_client = Client::new();
        Player {
            device_id: device_id.clone().to_string(),
            access_token_provider,
            http_client,
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
        let access_token = self.access_token_provider.get_bearer_token().unwrap();
        let msg = "Failed to start Spotify playback";
        let req = Self::derive_start_playback_payload_from_spotify_uri(spotify_uri);
        self.http_client
            .put("https://api.spotify.com/v1/me/player/play")
            .query(&[("device_id", &self.device_id)])
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
        let access_token = self.access_token_provider.get_bearer_token().unwrap();
        let msg = "Failed to stop Spotify playback";
        self.http_client
            .put("https://api.spotify.com/v1/me/player/pause")
            .query(&[("device_id", &self.device_id)])
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
