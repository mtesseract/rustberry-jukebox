use crate::model;
use anyhow::{Context, Result};
use std::default::Default;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

use tokio::time::{sleep, Duration};
use tracing::level_filters::LevelFilter;
use tracing::{error, info};
use tracing_subscriber::{filter, reload, Registry};

use model::config::{Config, PartialConfig};

#[derive(Clone)]
pub struct ConfigLoader {
    cfg_file: PathBuf,
    cfg: Arc<RwLock<model::config::Config>>,
    reload_handle: reload::Handle<LevelFilter, Registry>,
}

pub struct ConfigLoaderHandle {
    cfg: Arc<RwLock<model::config::Config>>,
}

impl ConfigLoaderHandle {
    pub fn get(&self) -> model::config::Config {
        let read_guard = self.cfg.read().unwrap();
        read_guard.clone()
    }
}

impl ConfigLoader {
    pub fn get(&self) -> model::config::Config {
        let read_guard = self.cfg.read().unwrap();
        read_guard.clone()
    }

    pub fn set(&self, cfg: Config) {
        let mut write_guard = self.cfg.write().unwrap();
        *write_guard = cfg;
    }

    async fn load_cfg(file: &Path) -> Result<PartialConfig> {
        let content = tokio::fs::read_to_string(file)
            .await
            .with_context(|| format!("Reading configuration file at {}", file.display()))?;
        let cfg: PartialConfig = serde_yaml::from_str(&content)
            .with_context(|| format!("YAML unmarshalling configuration at {}", file.display()))?;
        Ok(cfg)
    }

    fn log_level_hook(&self, prev: &Config, current: &Config) {
        if prev.debug != current.debug {
            let mut fltr = filter::LevelFilter::INFO;
            if current.debug {
                fltr = filter::LevelFilter::TRACE;
            }
            info!("Updating tracing log level to: {}", fltr);
            if let Err(err) = self.reload_handle.modify(|filter| *filter = fltr) {
                error!("Failed to update tracing log level: {}", err);
            }
        }
    }

    async fn loader_loop(self) {
        let cfg_file = self.cfg_file.as_path();
        info!("Config loader loop started");
        loop {
            match Self::load_cfg(&cfg_file).await {
                Ok(cfg_part) => {
                    let cfg_prev = self.get();
                    let mut cfg = cfg_prev.clone();
                    cfg.merge_partial(cfg_part);
                    self.log_level_hook(&cfg_prev, &cfg);
                    self.set(cfg);
                }
                Err(err) => {
                    if let Some(io_err) = err.downcast_ref::<io::Error>() {
                        if io_err.kind() == io::ErrorKind::NotFound {
                            continue;
                        }
                    }
                    error!("Failed to load runtime config: {}", err);
                }
            }
            sleep(Duration::from_secs(5)).await;
        }
    }

    pub fn spawn_async_loader(self) -> Result<()> {
        info!("Spawning configuration loader");
        tokio::spawn(async {
            self.loader_loop().await;
        });
        Ok(())
    }

    fn handle(&self) -> ConfigLoaderHandle {
        let cfg = self.cfg.clone();
        ConfigLoaderHandle { cfg }
    }

    pub fn new(
        cfg_file: &Path,
        reload_handle: reload::Handle<LevelFilter, Registry>,
    ) -> Result<ConfigLoaderHandle> {
        let cfg_file = cfg_file.to_path_buf();
        let mut cfg = model::config::Config::default();
        let env_cfg = envy::from_env::<model::config::PartialConfig>()?;
        cfg.merge_partial(env_cfg);
        let cfg = Arc::new(RwLock::new(cfg));
        let cfg_loader = ConfigLoader {
            cfg_file,
            cfg,
            reload_handle,
        };
        let handle = cfg_loader.handle();
        if let Err(err) = cfg_loader.spawn_async_loader() {
            error!("Failed to spawn aync config loader: {}", err);
        }
        Ok(handle)
    }
}
