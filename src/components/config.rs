use crate::model;
use anyhow::{Context, Result};
use std::default::Default;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

#[derive(Clone)]
pub struct ConfigLoader {
    cfg_file: PathBuf,
    cfg: Arc<RwLock<model::config::Config>>,
}

impl ConfigLoader {
    // fn loader(self) {

    // }

    pub fn spawn_async_loader(&self) -> Result<()> {
        // let cfg_loader = self.clone();
        // info!("Spawning configuration loader")
        Ok(())
    }

    pub fn new(cfg_file: &Path) -> Result<Self> {
        let cfg_file = cfg_file.to_path_buf();
        let cfg = envy::from_env::<model::config::Config>()?;
        let cfg = Arc::new(RwLock::new(cfg));
        let cfg_loader = ConfigLoader { cfg_file, cfg };
        Ok(cfg_loader)
    }


    pub fn get(&self) -> Result<model::config::Config> {
        let cfg: model::config::Config = {
            let read_guard = self.cfg.read().unwrap();
            read_guard.clone()
        };
        Ok(cfg)
    }
}
