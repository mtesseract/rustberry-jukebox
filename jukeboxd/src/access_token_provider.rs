use std::sync::{Arc, RwLock};
use std::thread;

use failure::Fallible;
use gotham_derive::StateData;
use slog_scope::{info, warn};

use spotify_auth::request_fresh_token;

#[derive(Debug, Clone, StateData)]
pub struct AccessTokenProvider {
    client_id: String,
    client_secret: String,
    refresh_token: String,
    access_token: Arc<RwLock<Option<String>>>,
}

fn token_refresh_thread(
    client_id: String,
    client_secret: String,
    refresh_token: String,
    access_token: Arc<RwLock<Option<String>>>,
) {
    loop {
        {
            let token = request_fresh_token(&client_id, &client_secret, &refresh_token)
                .map(|x| x.access_token);
            if let Ok(ref token) = token {
                info!("Retrieved fresh access token"; "access_token" => token);
            } else {
                warn!("Failed to retrieve access token");
            }
            let mut access_token_write = access_token.write().unwrap();

            if let Ok(token) = token {
                *access_token_write = Some(token);
            }
        }
        thread::sleep(std::time::Duration::from_secs(600));
    }
    // error!("Token refresh thread terminated unexpectedly");
    // panic!()
}

#[derive(Clone, Copy, Debug)]
pub enum AtpError {
    NoTokenReceivedYet,
}

impl std::fmt::Display for AtpError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        use AtpError::*;

        match self {
            NoTokenReceivedYet => write!(f, "No initial token received yet"),
        }
    }
}

impl std::error::Error for AtpError {}

impl AccessTokenProvider {
    pub fn get_token(&mut self) -> Fallible<String> {
        let access_token = self.access_token.read().unwrap();

        match &*access_token {
            Some(token) => Ok(token.clone()),
            None => Err(AtpError::NoTokenReceivedYet.into()),
        }
    }

    pub fn get_bearer_token(&mut self) -> Fallible<String> {
        self.get_token().map(|token| format!("Bearer {}", &token))
    }

    pub fn new(client_id: &str, client_secret: &str, refresh_token: &str) -> AccessTokenProvider {
        let access_token = Arc::new(RwLock::new(None));

        {
            let access_token_clone = Arc::clone(&access_token);
            let client_id = client_id.clone().to_string();
            let client_secret = client_secret.clone().to_string();
            let refresh_token = refresh_token.clone().to_string();

            thread::spawn(move || {
                token_refresh_thread(client_id, client_secret, refresh_token, access_token_clone)
            });
        }

        AccessTokenProvider {
            client_id: client_id.clone().to_string(),
            client_secret: client_secret.clone().to_string(),
            refresh_token: refresh_token.clone().to_string(),
            access_token,
        }
    }
}

pub mod spotify_auth {
    use failure::Fallible;
    const TOKEN_REFRESH_URL: &str = "https://accounts.spotify.com/api/token";
    use base64;
    use reqwest::header::AUTHORIZATION;
    use serde::Deserialize;

    #[derive(Debug, Clone, Deserialize)]
    pub struct AuthResponse {
        pub access_token: String,
        pub token_type: String,
        pub scope: String,
        pub expires_in: i32,
        pub refresh_token: String,
    }

    #[derive(Debug, Clone, Deserialize)]
    pub struct RefreshTokenResponse {
        pub access_token: String,
        pub token_type: String,
        pub scope: String,
        pub expires_in: i32,
    }

    fn encode_client_id_and_secret(client_id: &str, client_secret: &str) -> String {
        let concat = format!("{}:{}", client_id, client_secret);
        let b64 = base64::encode(concat.as_bytes());
        b64
    }

    pub fn request_fresh_token(
        client_id: &str,
        client_secret: &str,
        refresh_token: &str,
    ) -> Fallible<RefreshTokenResponse> {
        let grant_type = "refresh_token";
        let client_id_and_secret = encode_client_id_and_secret(client_id, client_secret);
        let auth_token = format!("Basic {}", client_id_and_secret);
        let params = [("grant_type", grant_type), ("refresh_token", refresh_token)];

        let http_client = reqwest::Client::new();
        let mut res = http_client
            .post(TOKEN_REFRESH_URL)
            .header(AUTHORIZATION, auth_token)
            .form(&params)
            .send()?;
        let rsp_json: RefreshTokenResponse = res.json()?;
        Ok(rsp_json)
    }
}
