use std::sync::{Arc, RwLock};
use std::thread;

use failure::Fallible;
// use gotham_derive::StateData;
use slog_scope::{info, warn};

use spotify_auth::request_fresh_token;

pub use err::*;

#[derive(Debug, Clone)]
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
            match request_fresh_token(&client_id, &client_secret, &refresh_token)
                .map(|x| x.access_token)
            {
                Ok(token) => {
                    info!("Retrieved fresh access token"; "access_token" => &token);
                    let mut access_token_write = access_token.write().unwrap();
                    *access_token_write = Some(token);
                }
                Err(err) => {
                    warn!("Failed to retrieve access token: {}", err);
                }
            }
        }
        thread::sleep(std::time::Duration::from_secs(600));
    }
    // error!("Token refresh thread terminated unexpectedly");
    // panic!()
}

impl AccessTokenProvider {
    pub fn wait_for_token(&self) -> Result<(), AtpError> {
        let n_attempts = 20;
        for _idx in 0..n_attempts {
            if self.access_token.read().unwrap().is_some() {
                return Ok(());
            }
            thread::sleep(std::time::Duration::from_millis(500));
        }
        Err(AtpError::NoTokenReceivedYet)
    }

    pub fn get_token(&self) -> Result<String, AtpError> {
        let access_token = self.access_token.read().unwrap();

        match &*access_token {
            Some(token) => Ok(token.clone()),
            None => Err(AtpError::NoTokenReceivedYet.into()),
        }
    }

    pub fn get_bearer_token(&self) -> Result<String, AtpError> {
        self.get_token().map(|token| format!("Bearer {}", &token))
    }

    pub fn new(
        client_id: &str,
        client_secret: &str,
        refresh_token: &str,
    ) -> Fallible<AccessTokenProvider> {
        let access_token = Arc::new(RwLock::new(None));

        {
            let access_token_clone = Arc::clone(&access_token);
            let client_id = client_id.clone().to_string();
            let client_secret = client_secret.clone().to_string();
            let refresh_token = refresh_token.clone().to_string();

            thread::Builder::new()
                .name("access-token-provider".to_string())
                .spawn(move || {
                    token_refresh_thread(
                        client_id,
                        client_secret,
                        refresh_token,
                        access_token_clone,
                    )
                })?;
        }

        Ok(AccessTokenProvider {
            client_id: client_id.clone().to_string(),
            client_secret: client_secret.clone().to_string(),
            refresh_token: refresh_token.clone().to_string(),
            access_token,
        })
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

        let http_client = reqwest::blocking::Client::new();
        let res = http_client
            .post(TOKEN_REFRESH_URL)
            .header(AUTHORIZATION, auth_token)
            .form(&params)
            .send()?
            .error_for_status()?;

        // FIXME: error logging.
        let rsp_body_json: serde_json::Value = res.json()?;
        dbg!(&rsp_body_json);
        Ok(serde_json::value::from_value(rsp_body_json)?)
    }
}

pub mod err {
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
}
