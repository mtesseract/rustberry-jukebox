use serde::Deserialize;

#[derive(Deserialize, Debug, Clone)]
pub struct Config {
    pub refresh_token: String,
    pub client_id: String,
    pub client_secret: String,
    pub device_name: String,
    pub post_init_command: Option<String>,
    pub shutdown_command: Option<String>,
    pub volume_up_command: Option<String>,
    pub volume_down_command: Option<String>,
}
