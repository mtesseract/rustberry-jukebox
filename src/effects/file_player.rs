use anyhow::{anyhow, Context, Result};
use cpal::traits::HostTrait;
use rodio::{Device, DeviceTrait, Sink};
use std::convert::From;
use std::fs::File;
use std::io::BufReader;
use std::sync::Arc;
use std::thread::{Builder, JoinHandle};
use std::time::Duration;
use tracing::{debug, info, warn};

use async_trait::async_trait;
use crossbeam_channel::{self, Sender};
use std::path::{Path, PathBuf};
use tokio::runtime::Runtime;
use tokio::task::spawn_blocking;

use crate::player::{PauseState, PlaybackHandle};
use crate::components::config::ConfigLoaderHandle;

pub struct FilePlayer {
    _handle: Option<JoinHandle<()>>,
    base_dir: PathBuf,
    config: ConfigLoaderHandle,
}

pub struct FilePlaybackHandle {
    _tx: Sender<()>,
    sink: Arc<Sink>,
    file_path: PathBuf,
}

const FROM_BEGINNING: Duration = Duration::from_secs(0);

impl FilePlaybackHandle {
    pub async fn queue(&self) -> Result<()> {
        let filename = &self.file_path;
        let file = BufReader::new(File::open(filename).unwrap());
        let source =
            spawn_blocking(move || rodio::Decoder::new(BufReader::new(file)).unwrap()).await?;
        self.sink.append(source);
        Ok(())
    }
}

#[async_trait]
impl PlaybackHandle for FilePlaybackHandle {
    async fn is_complete(&self) -> Result<bool> {
        Ok(self.sink.empty())
    }

    async fn stop(&self) -> Result<()> {
        self.sink.pause();
        Ok(())
    }
    async fn cont(&self, _pause_state: PauseState) -> Result<()> {
        self.sink.play();
        Ok(())
    }

    async fn replay(&self) -> Result<()> {
        self.sink.stop();
        self.queue().await?;
        self.sink.play();
        Ok(())
    }
}

impl FilePlayer {
    fn display_device_info(device: &Device) -> Result<()> {
        let name = device.name().unwrap_or_else(|_| "Unknown".to_string());
        info!("- audio output device: {}", name);
        // if let Ok(configs) = device.supported_output_configs() {
        //     for config in configs {
        //         println!("  - {:?}", config);
        //     }
        // }
        Ok(())
    }

    fn display_devices_info() -> Result<()> {
        info!("Available output devices:");
        let host = cpal::default_host();
        let devices = host.output_devices()?;
        if let Some(device) = host.default_output_device() {
            if let Err(err) = Self::display_device_info(&device) {
                warn!(
                    "Failed to list device info for default device {:?}: {}",
                    device.name(),
                    err
                );
            }
        }
        for device in devices {
            if let Err(err) = Self::display_device_info(&device) {
                warn!(
                    "Failed to list device info for device {:?}: {}",
                    device.name(),
                    err
                );
            }
        }
        Ok(())
    }

    pub fn new(config_loader: ConfigLoaderHandle) -> Result<Self> {
        let config = config_loader.get();
        let base_dir = config.audio_base_directory;
        info!("Creating new FilePlayer...");
        if let Err(err) = Self::display_devices_info() {
            warn!("Failed to list audio devices: {}", err);
        }
        let base_dir = PathBuf::from(base_dir);
        let player = FilePlayer {
            _handle: None,
            base_dir,
            config: config_loader,
        };

        Ok(player)
    }

    fn complete_file_name(&self, mut fname: &Path) -> Result<PathBuf> {
        let mut complete_fname = self.base_dir.clone();
        if fname.is_absolute() {
            fname = fname.strip_prefix("/")?;
        }
        complete_fname.push(fname);
        Ok(complete_fname)
    }

    fn lookup_device_by_name(name: &str) -> Result<Device> {
        let devices = rodio::devices().with_context(|| "retrieving list of audio devices")?;
        for device in devices {
            let device_name = device.name().with_context(|| "retrieving audio device name")?;
            if device_name == name {
                return Ok(device);
            }
        }
        Err(anyhow!("audio device not found: {}", name))
    }

    pub async fn start_playback(
        &self,
        uris: &[String],
        pause_state: Option<PauseState>,
    ) -> Result<FilePlaybackHandle, anyhow::Error> {
        info!("Initiating playback for uris {:?}", uris);
        let config = self.config.get();

        if let Some(pause_state) = pause_state {
            warn!("Ignoring pause state: {:?}", pause_state);
        }

        let device = match config.audio_output_device {
            Some(name) => Self::lookup_device_by_name(&name).with_context(|| "attempting to initiate playback")?,
            None => rodio::default_output_device().with_context(|| "retrieving default audio output device")?,
        };
        debug!(
            "Initiating playback via device: {:?}",
            device.name().unwrap_or("(unknown)".to_string())
        );

        let file_name = match uris.first().cloned() {
            Some(uri) => uri,
            None => return Err(anyhow::Error::msg("TagConf is empty")),
        };
        let file_path = self
            .complete_file_name(Path::new(file_name.as_str()))
            .with_context(|| format!("completing file name {}", file_name))?;

        let (tx, rx) = crossbeam_channel::bounded(1);

        let _handle = Builder::new()
            .name("file-player".to_string())
            .spawn(move || {
                let rt = Runtime::new().unwrap();
                let f = async {
                    let _msg = rx.recv();
                };
                rt.block_on(f);
            })
            .unwrap();

        let sink = Sink::new(&device);
        let handle = FilePlaybackHandle {
            _tx: tx, // Cancellation mechanism.
            sink: Arc::new(sink),
            file_path: file_path,
        };
        handle
            .queue()
            .await
            .context("queue method of player handle")?;
        handle
            .cont(PauseState {
                pos: FROM_BEGINNING,
            })
            .await
            .context("cont method of player handle")?;
        Ok(handle)
    }
}
