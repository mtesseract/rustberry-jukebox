use std::convert::From;
use std::fmt::{self, Display};

use hyper::header::AUTHORIZATION;
use reqwest::Client;
use serde::Serialize;
use slog_scope::{error, info};

use crate::components::access_token_provider::{self, AccessTokenProvider};
use crate::config::Config;

use super::connect::{self, SpotifyConnector};

pub use err::*;

pub struct SpotifyPlayer {
    http_client: Client,
    access_token_provider: AccessTokenProvider,
    spotify_connector: Box<dyn SpotifyConnector + 'static + Send>,
}

#[derive(Debug, Clone, Serialize)]
struct StartPlayback {
    #[serde(skip_serializing_if = "Option::is_none")]
    context_uri: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    uris: Option<Vec<String>>,
}

impl SpotifyPlayer {
    pub fn new(config: &Config) -> Self {
        let http_client = Client::new();
        // Create Access Token Provider
        let access_token_provider = access_token_provider::AccessTokenProvider::new(
            &config.client_id,
            &config.client_secret,
            &config.refresh_token,
        );
        let spotify_connector = Box::new(
            connect::external_command::ExternalCommand::new_from_env(
                &access_token_provider.clone(),
                config.device_name.clone(),
            )
            .unwrap(),
        );

        info!("Creating new SpotifyPlayer...");

        SpotifyPlayer {
            http_client,
            access_token_provider,
            spotify_connector,
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

    pub fn start_playback(&self, spotify_uri: &str) -> Result<(), Error> {
        let msg = "Failed to start Spotify playback";
        let access_token = self.access_token_provider.get_token().unwrap(); // fixme
        let device_id = match self.spotify_connector.device_id() {
            Some(device_id) => device_id,
            None => return Err(Error::NoSpotifyDevice),
        };
        dbg!(&device_id);
        dbg!(&access_token);
        let req = Self::derive_start_playback_payload_from_spotify_uri(spotify_uri);
        self.http_client
            .put("https://api.spotify.com/v1/me/player/play")
            .query(&[("device_id", &device_id)])
            .header(AUTHORIZATION, access_token)
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

    pub fn stop_playback(&self) -> Result<(), Error> {
        let msg = "Failed to stop Spotify playback";
        let access_token = self.access_token_provider.get_token().unwrap(); // fixme
        let device_id = match self.spotify_connector.device_id() {
            Some(device_id) => device_id,
            None => return Err(Error::NoSpotifyDevice),
        };
        self.http_client
            .put("https://api.spotify.com/v1/me/player/pause")
            .query(&[("device_id", &device_id)])
            .body("")
            .header(AUTHORIZATION, access_token)
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

pub mod err {
    use super::*;

    #[derive(Debug)]
    pub enum Error {
        HTTP(reqwest::Error),
        NoSpotifyDevice,
    }

    impl Display for Error {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            match self {
                Error::HTTP(err) => write!(f, "Spotify HTTP Error {}", err),
                Error::NoSpotifyDevice => write!(f, "No Spotify Connect Device found"),
            }
        }
    }

    impl From<reqwest::Error> for Error {
        fn from(err: reqwest::Error) -> Self {
            Error::HTTP(err)
        }
    }

    impl std::error::Error for Error {}
}
