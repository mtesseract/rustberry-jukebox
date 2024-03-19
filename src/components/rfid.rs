use failure::{self, Fallible};
use slog_scope::{error, info};
use std::io::{Read, Write};
use std::sync::{Arc, Mutex};

use embedded_hal_1 as embedded_hal;
use linux_embedded_hal as hal;

use embedded_hal::delay::DelayNs;
use embedded_hal::spi::Error as SPIError;
use embedded_hal_bus::spi::{DeviceError, ExclusiveDevice};
use hal::spidev::{SpiModeFlags, SpidevOptions};
use hal::{Delay, SpidevBus, SysfsPin};
use mfrc522::comm::{blocking::spi::SpiInterface, Interface};
use mfrc522::{self, Initialized, Mfrc522, Uid};

type Mfrc522Comm = SpiInterface<
    ExclusiveDevice<SpidevBus, SysfsPin, Delay>,
    mfrc522::comm::blocking::spi::DummyDelay,
>;

type Mfrc522Error = mfrc522::error::Error<DeviceError<hal::SPIError, hal::SysfsPinError>>;

#[derive(Clone)]
pub struct RfidController {
    pub mfrc522: Arc<Mutex<Mfrc522<Mfrc522Comm, Initialized>>>,
}

pub struct Tag {
    pub uid: Uid,
}

impl RfidController {
    pub fn new() -> Fallible<Self> {
        let mut delay = Delay;
        let mut spi = SpidevBus::open("/dev/spidev0.0").unwrap();
        let options = SpidevOptions::new()
            .max_speed_hz(1_000_000)
            .mode(SpiModeFlags::SPI_MODE_0 | SpiModeFlags::SPI_NO_CS)
            .build();
        spi.configure(&options).unwrap();

        // software-controlled chip select pin
        let pin = SysfsPin::new(22)
            .into_output_pin(embedded_hal::digital::PinState::High)
            .unwrap();
        pin.export().unwrap();
        while !pin.is_exported() {}
        delay.delay_ms(1u32); // delay sometimes necessary because `is_exported()` returns too early?

        let spi = ExclusiveDevice::new(spi, pin, Delay);
        let itf = SpiInterface::new(spi);
        let mut mfrc522: Mfrc522<SpiInterface<ExclusiveDevice<SpidevBus, SysfsPin, Delay>, mfrc522::comm::blocking::spi::DummyDelay>, Initialized> = Mfrc522::new(itf).init()?;

        // // Use your HAL to create an SPI device that implements the embedded-hal `SpiDevice` trait.
        // // This device manages the SPI bus and CS pin.
        // let spi = spi::Spi;

        // let itf = SpiInterface::new(spi);
        // let mut mfrc522 = Mfrc522::new(itf).init().unwrap();

        let vers = mfrc522.version()?;

        info!("mfrc522 version: 0x{:x}", vers);
        info!("Created new MFRC522 Controller");
        Ok(RfidController {
            mfrc522: Arc::new(Mutex::new(mfrc522)),
        })
    }

    pub fn try_open_tag(&mut self) -> Result<Tag, Mfrc522Error> {
        let mut mfrc522 = self.mfrc522.lock().unwrap();
        let atqa = mfrc522.new_card_present()?;
        let uid = mfrc522.select(&atqa)?;
        Ok(Tag { uid })
    }

    pub fn open_tag(&mut self) -> Fallible<Option<Tag>> {
        match self.try_open_tag() {
            Ok(tag) => Ok(Some(tag)),
            Err(Mfrc522Error::Timeout) => Ok(None),
            Err(err) => Err(err.into()),
        }
    }
}
