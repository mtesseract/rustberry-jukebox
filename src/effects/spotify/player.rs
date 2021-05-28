use std::convert::From;
use std::fmt::{self, Display};
use std::sync::Arc;

use async_trait::async_trait;
use failure::Fallible;
use http::header::{self, AUTHORIZATION};
use reqwest::Client;
use serde::Serialize;
use slog_scope::{error, info};

use crate::components::access_token_provider::{self, AccessTokenProvider};
use crate::config::Config;
use crate::player::{PauseState, PlaybackHandle};

use super::connect::{self, SpotifyConnector};
use super::util::is_currently_playing;

pub use err::*;

pub struct SpotifyPlayer {
    http_client: Arc<Client>,
    access_token_provider: Arc<AccessTokenProvider>,
    spotify_connector: Arc<Box<dyn SpotifyConnector + 'static + Sync + Send>>,
    device_name: Arc<String>,
}

#[derive(Debug, Clone, Serialize)]
struct StartPlayback {
    #[serde(skip_serializing_if = "Option::is_none")]
    context_uri: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    uris: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    position_ms: Option<u128>,
}

pub struct SpotifyPlaybackHandle {
    device_name: Arc<String>,
    http_client: Arc<Client>,
    access_token_provider: Arc<AccessTokenProvider>,
    uri: String,
    spotify_connector: Arc<Box<dyn SpotifyConnector + 'static + Sync + Send>>,
}

#[async_trait]
impl PlaybackHandle for SpotifyPlaybackHandle {
    async fn stop(&self) -> Fallible<()> {
        let msg = "Failed to stop Spotify playback";
        let access_token = self.access_token_provider.get_token()?;
        let device_id = match self.spotify_connector.device_id() {
            Some(device_id) => device_id,
            None => return Err(Error::NoSpotifyDevice.into()),
        };
        self.http_client
            .put("https://api.spotify.com/v1/me/player/pause")
            .query(&[("device_id", &device_id)])
            .body("")
            .header(header::CONTENT_LENGTH, 0)
            .header(AUTHORIZATION, format!("Bearer {}", access_token))
            .send()
            .await
            .map_err(|err| {
                error!("{}: Executing HTTP request failed: {}", msg, err);
                err
            })
            .map(|rsp| {
                if !rsp.status().is_success() {
                    error!("{}: HTTP Failure {}", msg, rsp.status());
                }
                rsp
            })?
            .error_for_status()
            .map(|_| ())
            .map_err(|err| Error::Http(err).into())
    }
    async fn is_complete(&self) -> Fallible<bool> {
        is_currently_playing(
            &*self.http_client,
            &*self.access_token_provider,
            &*self.device_name,
        )
        .await
        .map(|x| !x)
    }
    async fn cont(&self, pause_state: PauseState) -> Fallible<()> {
        let msg = "Failed to start Spotify playback";
        let access_token = self.access_token_provider.get_token()?;
        let device_id = match self.spotify_connector.device_id() {
            Some(device_id) => device_id,
            None => return Err(Error::NoSpotifyDevice.into()),
        };
        let req =
            Self::derive_start_playback_payload_from_spotify_uri(&self.uri, &Some(pause_state));

        self.http_client
            .put("https://api.spotify.com/v1/me/player/play")
            .query(&[("device_id", &device_id)])
            .header(AUTHORIZATION, format!("Bearer {}", access_token))
            .json(&req)
            .send()
            .await
            .map_err(|err| {
                error!("{}: Executing HTTP request failed: {}", msg, err);
                err
            })
            .map(|rsp| {
                if !rsp.status().is_success() {
                    error!("{}: HTTP Failure {}", msg, rsp.status());
                }
                rsp
            })?
            .error_for_status()
            .map(|_| ())
            .map_err(|err| Error::Http(err).into())
    }
    async fn replay(&self) -> Fallible<()> {
        let msg = "Failed to start Spotify playback";
        let access_token = self.access_token_provider.get_token()?;
        let device_id = match self.spotify_connector.device_id() {
            Some(device_id) => device_id,
            None => return Err(Error::NoSpotifyDevice.into()),
        };
        let req = Self::derive_start_playback_payload_from_spotify_uri(&self.uri, &None);

        self.http_client
            .put("https://api.spotify.com/v1/me/player/play")
            .query(&[("device_id", &device_id)])
            .header(AUTHORIZATION, format!("Bearer {}", access_token))
            .json(&req)
            .send()
            .await
            .map_err(|err| {
                error!("{}: Executing HTTP request failed: {}", msg, err);
                err
            })
            .map(|rsp| {
                if !rsp.status().is_success() {
                    error!("{}: HTTP Failure {}", msg, rsp.status());
                }
                rsp
            })?
            .error_for_status()
            .map(|_| ())
            .map_err(|err| Error::Http(err).into())
    }
}

impl SpotifyPlaybackHandle {
    fn derive_start_playback_payload_from_spotify_uri(
        spotify_uri: &str,
        pause_state: &Option<PauseState>,
    ) -> StartPlayback {
        let position_ms = pause_state.as_ref().map(|x| x.pos.as_millis());
        if &spotify_uri[0..14] == "spotify:album:" || &spotify_uri[0..17] == "spotify:playlist:" {
            StartPlayback {
                uris: None,
                context_uri: Some(spotify_uri.to_string()),
                position_ms,
            }
        } else {
            StartPlayback {
                uris: Some(vec![spotify_uri.to_string()]),
                context_uri: None,
                position_ms,
            }
        }
    }
}

impl SpotifyPlayer {
    pub fn new(config: &Config) -> Fallible<Self> {
        let http_client = Arc::new(Client::new());
        // Create Access Token Provider
        let access_token_provider = Arc::new(access_token_provider::AccessTokenProvider::new(
            &config.client_id,
            &config.client_secret,
            &config.refresh_token,
        )?);
        let spotify_connector = Arc::new(Box::new(
            connect::external_command::ExternalCommand::new_from_env(
                &access_token_provider,
                config.device_name.clone(),
            )
            .unwrap(),
        )
            as Box<dyn SpotifyConnector + 'static + Sync + Send>);

        info!("Creating new SpotifyPlayer...");

        Ok(SpotifyPlayer {
            http_client,
            access_token_provider,
            spotify_connector,
            device_name: Arc::new(config.device_name.clone()),
        })
    }

    pub fn wait_until_ready(&self) -> Result<(), Error> {
        self.spotify_connector
            .wait_until_ready()
            .map_err(|_err| Error::NoSpotifyDevice)?;
        self.access_token_provider
            .wait_for_token()
            .map_err(|_err| Error::NoToken)?;
        Ok(())
    }

    pub async fn start_playback(
        &self,
        spotify_uri: &str,
        _pause_state: Option<PauseState>,
    ) -> Result<SpotifyPlaybackHandle, failure::Error> {
        let handle = SpotifyPlaybackHandle {
            http_client: self.http_client.clone(),
            access_token_provider: self.access_token_provider.clone(),
            uri: spotify_uri.to_string(),
            spotify_connector: self.spotify_connector.clone(),
            device_name: self.device_name.clone(),
        };

        let _ = handle.replay().await?;

        Ok(handle)
    }
}

pub mod err {
    use super::*;

    #[derive(Debug)]
    pub enum Error {
        Http(reqwest::Error),
        NoSpotifyDevice,
        NoToken,
    }

    impl Display for Error {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            match self {
                Error::Http(err) => write!(f, "Spotify HTTP Error {}", err),
                Error::NoSpotifyDevice => write!(f, "No Spotify Connect Device found"),
                Error::NoToken => write!(
                    f,
                    "Failed to obtain access token from Access Token Provider"
                ),
            }
        }
    }

    impl From<reqwest::Error> for Error {
        fn from(err: reqwest::Error) -> Self {
            Error::Http(err)
        }
    }

    impl From<access_token_provider::AtpError> for Error {
        fn from(err: access_token_provider::err::AtpError) -> Self {
            match err {
                access_token_provider::AtpError::NoTokenReceivedYet => Error::NoToken,
            }
        }
    }

    impl std::error::Error for Error {}
}
