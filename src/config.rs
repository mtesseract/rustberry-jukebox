use serde::Deserialize;

#[derive(Deserialize, Debug, Clone)]
pub struct Config {
    // pub refresh_token: String,
    // pub client_id: Option<String>,
    // pub client_secret: Option<String>,
    // pub device_name: Option<String>,
    pub enable_spotify: bool,
    pub post_init_command: Option<String>,
    pub shutdown_command: Option<String>,
    pub volume_up_command: Option<String>,
    pub volume_down_command: Option<String>,
    pub trigger_only_mode: bool,
}
