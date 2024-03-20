use anyhow::{Context,Result};
use slog_scope::{error, info};
use std::io::{Read, Write};
use std::sync::{Arc, Mutex};

use embedded_hal_1 as embedded_hal;
use linux_embedded_hal as hal;

use embedded_hal::delay::DelayNs;
use embedded_hal::spi::Error as SPIError;
use embedded_hal_bus::spi::{DeviceError, ExclusiveDevice};
use hal::spidev::{SpiModeFlags, SpidevOptions};
use hal::{Delay, SpidevBus, SpidevDevice};
use mfrc522::comm::{
    blocking::spi::{DummyDelay, SpiInterface},
    Interface,
};
use mfrc522::{self, Initialized, Mfrc522, Uid};

#[derive(Clone)]
pub struct RfidController {
    pub mfrc522: Arc<Mutex<Mfrc522<SpiInterface<SpidevDevice, DummyDelay>, Initialized>>>,
}

pub struct Tag {
    pub uid: Uid,
}

impl RfidController {
    pub fn new() -> Result<Self> {
        let mut spi =
            SpidevDevice::open("/dev/spidev0.0").context("Opening SPI device /dev/spidev0.0")?;
        let options = SpidevOptions::new()
            .max_speed_hz(1_000_000)
            .mode(SpiModeFlags::SPI_MODE_0 | SpiModeFlags::SPI_NO_CS)
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

    pub fn try_open_tag(&mut self) -> Result<Tag> {
        info!("try_open_tag()");
        let mut mfrc522 = self.mfrc522.lock().unwrap();
        info!("try_open_tag() 1");
        let atqa = mfrc522.new_card_present()?;
        info!("try_open_tag() 2");
        let uid = mfrc522.select(&atqa)?;
        info!("try_open_tag() 3");
        Ok(Tag { uid })
    }

    pub fn open_tag(&mut self) -> Result<Option<Tag>> {
        match self.try_open_tag() {
            Ok(tag) => Ok(Some(tag)),
            // Err(Mfrc522Error::Timeout) => Ok(None),
            // Err(err) => Err(err.into()),
            Err(err) => Err(err),
        }
    }
}
