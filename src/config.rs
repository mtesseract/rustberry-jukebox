use serde::Deserialize;

#[derive(Deserialize, Debug, Clone)]
pub struct Config {
    #[serde(default = "enable_spotify_default")]
    pub enable_spotify: bool,
    pub post_init_command: Option<String>,
    pub shutdown_command: Option<String>,
    pub volume_up_command: Option<String>,
    pub volume_down_command: Option<String>,
    #[serde(default = "trigger_only_mode_default")]
    pub trigger_only_mode: bool,
    pub tag_mapper_configuration_file: String,
    pub audio_base_directory: String,
}

fn trigger_only_mode_default() -> bool {
    true
}

fn enable_spotify_default() -> bool {
    false
}
