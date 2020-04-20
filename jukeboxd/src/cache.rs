use std::collections::HashMap;
use std::fs::File;
use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json;

use failure::Fallible;
use fxhash::hash;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Cache {
    spotify_refresh_tokens: HashMap<usize, String>,
}

const DEFAULT_CACHE_DIRECTORY: &str = "/var/cache/jukeboxd";

impl Cache {
    pub fn set_spotify_refresh_token(&mut self, username: &str, token: &str) {
        let username_hash = hash(username);
        let _ = self
            .spotify_refresh_tokens
            .insert(username_hash, token.to_string().clone());
    }

    pub fn get_spotify_refresh_token(&self, username: &str) -> Option<String> {
        let username_hash = hash(username);
        self.spotify_refresh_tokens.get(&username_hash).cloned()
    }

    pub fn load_from_directory(directory: &Path) -> Fallible<Self> {
        let mut pb = directory.to_path_buf();
        pb.push("cache");
        let mut cr = File::open(pb)?;
        let cache = serde_json::from_reader(cr)?;
        Ok(cache)
    }

    pub fn load() -> Fallible<Self> {
        Self::load_from_directory(Path::new(DEFAULT_CACHE_DIRECTORY))
    }
}
