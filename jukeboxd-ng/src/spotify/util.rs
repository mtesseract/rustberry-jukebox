use failure::{Fail, Fallible};
use hyper::header::AUTHORIZATION;
use reqwest::Client;
use serde::Deserialize;

use crate::access_token_provider::{AccessTokenProvider, AtpError};

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct Device {
    pub id: String,
    pub name: String,
    pub is_active: bool,
}

#[derive(Debug, Clone, Deserialize)]
struct DevicesResponse {
    pub devices: Vec<Device>,
}

#[derive(Debug, Fail)]
pub enum JukeboxError {
    #[fail(display = "Device not found: {}", device_name)]
    DeviceNotFound { device_name: String },
    #[fail(display = "Failed to retrieve access token: {}", err)]
    TokenRetrieval { err: AtpError },
    #[fail(display = "HTTP Failure: {}", err)]
    HttpError { err: reqwest::Error },
    #[fail(display = "JSON Deserialization Failure: {}", err)]
    DeserializationFailed { err: serde_json::Error },
}

impl From<AtpError> for JukeboxError {
    fn from(err: AtpError) -> Self {
        JukeboxError::TokenRetrieval { err }
    }
}

impl From<reqwest::Error> for JukeboxError {
    fn from(err: reqwest::Error) -> Self {
        JukeboxError::HttpError { err }
    }
}

impl From<serde_json::Error> for JukeboxError {
    fn from(err: serde_json::Error) -> Self {
        JukeboxError::DeserializationFailed { err }
    }
}

pub fn lookup_device_by_name(
    access_token_provider: &AccessTokenProvider,
    device_name: &str,
) -> Result<Device, JukeboxError> {
    let http_client = Client::new();
    let access_token = access_token_provider.get_bearer_token()?;
    let mut rsp = http_client
        .get("https://api.spotify.com/v1/me/player/devices")
        .header(AUTHORIZATION, &access_token)
        .send()?;
    let rsp: DevicesResponse = rsp.json()?;
    let opt_dev = rsp
        .devices
        .into_iter()
        .filter(|x| x.name == device_name)
        .next();
    match opt_dev {
        Some(dev) => Ok(dev),
        None => Err((JukeboxError::DeviceNotFound {
            device_name: device_name.clone().to_string(),
        })
        .into()),
    }
}
