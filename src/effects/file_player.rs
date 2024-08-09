use anyhow::{anyhow, Context, Result};
use cpal::traits::HostTrait;
use rodio::{Device, DeviceTrait, OutputStream, OutputStreamHandle, Sink};
use std::convert::From;
use std::fs::File;
use std::io::BufReader;
use std::sync::Arc;
use tracing::{debug, info, warn};

use std::path::{Path, PathBuf};

use crate::components::config::ConfigLoaderHandle;

pub struct FilePlayer {
    base_dir: PathBuf,
    pub sink: Arc<Sink>,
    file_path: Option<PathBuf>,
    output_stream: OutputStream,
    output_stream_handle: OutputStreamHandle,
}

// const FROM_BEGINNING: Duration = Duration::from_secs(0);

impl FilePlayer {
    pub fn queue(&self) -> Result<()> {
        debug!("FilePlayer: queue");
        let path = if let Some(ref file_path) = self.file_path {
            file_path.clone()
        } else {
            warn!("cannot queue without file name");
            return Ok(());
        };
        let file = BufReader::new(File::open(path).unwrap());
        let source = rodio::Decoder::new(BufReader::new(file))?;
        self.sink.stop();
        self.sink.append(source);
        Ok(())
    }

    pub fn stop(&self) -> Result<()> {
        debug!("FilePlayer: stop");
        self.sink.pause();
        Ok(())
    }

    pub fn cont(&self) -> Result<()> {
        debug!("FilePlayer: cont");
        self.sink.play();
        Ok(())
    }

    fn display_device_info(device: &Device) -> Result<()> {
        let name = device.name().unwrap_or_else(|_| "Unknown".to_string());
        info!("- audio output device: {}", name);
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
        info!("Creating new FilePlayer...");
        let config = config_loader.get();
        let base_dir = config.audio_base_directory;
        if let Err(err) = Self::display_devices_info() {
            warn!("Failed to list audio devices: {}", err);
        }
        let base_dir = PathBuf::from(base_dir);

        let (stream, stream_handle) = match config.audio_output_device {
            Some(name) => {
                let device = Self::lookup_device_by_name(&name)?;
                debug!(
                    "Initiating playback via device: {:?}",
                    device.name().unwrap_or("(unknown)".to_string())
                );
                OutputStream::try_from_device(&device)?
            }
            None => {
                OutputStream::try_default().with_context(|| "retrieving default audio output device")?
            }
        };

        let sink = Sink::try_new(&stream_handle)?;
        let player = FilePlayer {
            base_dir,
            sink: Arc::new(sink),
            file_path: None,
            output_stream: stream,
            output_stream_handle: stream_handle,
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
        let host = cpal::default_host();
        let devices = host.output_devices().with_context(|| "retrieving list of audio devices")?;
        for device in devices {
            let device_name = device
                .name()
                .with_context(|| "retrieving audio device name")?;
            if device_name == name {
                return Ok(device);
            }
        }
        Err(anyhow!("audio device not found: {}", name))
    }

    pub fn start_playback(
        &mut self,
        uris: &[String],
        pause_state: Option<std::time::Duration>,
    ) -> Result<()> {
        info!("FilePlayer: initiating playback for uris {:?}", uris);

        if let Some(pause_state) = pause_state {
            warn!("Ignoring pause state: {:?}", pause_state);
        }

        let file_name = match uris.first().cloned() {
            Some(uri) => uri,
            None => return Err(anyhow::Error::msg("TagConf is empty")),
        };
        let file_path = self
            .complete_file_name(Path::new(file_name.as_str()))
            .with_context(|| format!("completing file name {}", file_name))?;

        self.file_path = Some(file_path);

        self.queue().context("queue method of player handle")?;
        self.cont().context("cont method of player handle")?;
        Ok(())
    }
}
