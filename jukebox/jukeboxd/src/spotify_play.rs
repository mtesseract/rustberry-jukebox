use crate::access_token_provider::AccessTokenProvider;
use failure::Fallible;
use hyper::header::AUTHORIZATION;
use reqwest::Client;
use serde::Serialize;

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
    context_uri: Option<String>,
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
        if &spotify_uri[0..10] == "spotify:album:" {
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

    pub fn start_playback(&mut self, spotify_uri: &str) -> Fallible<()> {
        let access_token = self.access_token_provider.get_bearer_token().unwrap();

        let req = Self::derive_start_playback_payload_from_spotify_uri(spotify_uri);
        let rsp = self
            .http_client
            .put("https://api.spotify.com/v1/me/player/play")
            .query(&[("device_id", &self.device_id)])
            .header(AUTHORIZATION, &access_token)
            .json(&req)
            .send()?;
        // assert!(rsp.status().is_success());

        Ok(())
    }

    pub fn stop_playback(&mut self) -> Fallible<()> {
        let access_token = self.access_token_provider.get_bearer_token().unwrap();
        let rsp = self
            .http_client
            .put("https://api.spotify.com/v1/me/player/pause")
            .query(&[("device_id", &self.device_id)])
            .body("")
            .header(AUTHORIZATION, &access_token)
            .send()?;
        // assert!(rsp.status().is_success());
        Ok(())
    }
}
