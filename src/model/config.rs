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
    pub enable_rfid_controller: bool,
}

#[derive(Deserialize, Debug, Clone)]
pub struct PartialConfig {
    pub enable_spotify: Option<bool>,
    pub post_init_command: Option<String>,
    pub shutdown_command: Option<String>,
    pub volume_up_command: Option<String>,
    pub volume_down_command: Option<String>,
    pub trigger_only_mode: Option<bool>,
    pub tag_mapper_configuration_file: Option<String>,
    pub audio_base_directory: Option<String>,
    pub debug: Option<bool>,
    pub enable_rfid_controller: Option<bool>,
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
            enable_rfid_controller: true,
        }
    }
}

impl Config {
    // cfg overwrites values in self.
    pub fn merge_partial(&mut self, cfg: PartialConfig) {
        if let Some(enable_spotify) = cfg.enable_spotify {
            self.enable_spotify = enable_spotify;
        }

        if let Some(post_init_command) = cfg.post_init_command {
            self.post_init_command = Some(post_init_command);
        }
        if let Some(shutdown_command) = cfg.shutdown_command {
            self.shutdown_command = Some(shutdown_command);
        }
        if let Some(volume_up_command) = cfg.volume_up_command {
            self.volume_up_command = Some(volume_up_command);
        }
        if let Some(volume_down_command) = cfg.volume_down_command {
            self.volume_down_command = Some(volume_down_command);
        }
        if let Some(trigger_only_mode) = cfg.trigger_only_mode {
            self.trigger_only_mode = trigger_only_mode
        }
        if let Some(tag_mapper_configuration_file) = cfg.tag_mapper_configuration_file {
            self.tag_mapper_configuration_file = tag_mapper_configuration_file;
        }
        if let Some(audio_base_directory) = cfg.audio_base_directory {
            self.audio_base_directory = audio_base_directory;
        }
        if let Some(debug) = cfg.debug {
            self.debug = debug
        }
        if let Some(enable_rfid_controller) = cfg.enable_rfid_controller {
            self.enable_rfid_controller = enable_rfid_controller
        }
    }
}
