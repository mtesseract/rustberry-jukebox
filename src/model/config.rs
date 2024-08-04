use serde::Deserialize;
use std::default::Default;

#[derive(Deserialize, Debug, Clone)]
pub struct Config {
    pub enable_spotify: bool,
    pub post_init_command: Option<String>,
    pub shutdown_command: Option<String>,
    pub volume_up_command: Option<String>,
    pub volume_down_command: Option<String>,
    pub trigger_only_mode: bool,
    pub tag_mapper_configuration_file: String,
    pub audio_base_directory: String,
    pub debug: bool,
}

#[derive(Deserialize, Debug, Clone)]
pub struct PartialConfig {
    pub debug: Option<bool>,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            enable_spotify: false,
            post_init_command: None,
            shutdown_command: None,
            volume_up_command: None,
            volume_down_command: None,
            trigger_only_mode: false,
            tag_mapper_configuration_file: "".to_string(),
            audio_base_directory: "".to_string(),
            debug: false,
        }
    }
}

impl Config {
    // cfg overwrites values in self.
    pub fn merge_partial(&mut self, cfg: PartialConfig) {
        if let Some(debug) = cfg.debug {
            self.debug = debug
        }
    }
}
