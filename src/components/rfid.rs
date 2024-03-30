use anyhow::{Context, Result};
use slog_scope::{info};
use std::sync::{Arc, Mutex};
use std::fmt;
use serde::{Deserialize, Serialize};

use embedded_hal_1 as embedded_hal;
use linux_embedded_hal as hal;
use embedded_hal::spi::Error as SPIError;
use hal::spidev::{SpiModeFlags, SpidevOptions};
use hal::SpidevDevice;
use mfrc522::comm:: blocking::spi::{DummyDelay, SpiInterface};
use mfrc522::{self, Initialized, Mfrc522};

#[derive(Clone)]
pub struct RfidController {
    pub mfrc522: Arc<Mutex<Mfrc522<SpiInterface<SpidevDevice, DummyDelay>, Initialized>>>,
}
#[derive(Debug, Clone,Deserialize, Serialize, PartialEq, Eq)]
pub struct Uid(String);
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct Tag {
    pub uid: Uid,
}

impl fmt::Display for Uid {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl Uid {
    pub fn from_bytes(bs: &[u8]) -> Uid {
        return Uid(hex::encode(bs))
    }
}

impl RfidController {
    pub fn new() -> Result<Self> {
        let mut spi =
            SpidevDevice::open("/dev/spidev0.0").context("Opening SPI device /dev/spidev0.0")?;
        let options = SpidevOptions::new()
            .max_speed_hz(1_000_000)
            .mode(SpiModeFlags::SPI_MODE_0)
            .build();
        spi.configure(&options).context("Configuring SPI device")?;

        let itf = SpiInterface::new(spi);
        let mut mfrc522 = Mfrc522::new(itf)
            .init()
            .context("Initializing MFRC522 PICC")?;

        let vers = mfrc522
            .version()
            .context("Retrieving MFRC522 version information")?;

        info!("mfrc522 version: 0x{:x}", vers);
        info!("Created new MFRC522 Controller");
        Ok(RfidController {
            mfrc522: Arc::new(Mutex::new(mfrc522)),
        })
    }

    pub fn open_tag(&mut self) -> Result<Option<Tag>> {
        let mut mfrc522 = self.mfrc522.lock().unwrap();
        let atqa = match mfrc522.new_card_present() {
            Err(mfrc522::error::Error::Timeout) => return Ok(None),
            // mfrc522::error::Error only has a stub Display implementation.
            Err(err) => return Err(anyhow::Error::msg(format!("{:?}", err))),
            Ok(atqa) => atqa,
        };
        let uid = mfrc522.select(&atqa)?;
        let uid = Uid::from_bytes(uid.as_bytes());
        Ok(Some(Tag { uid }))
    }
}
