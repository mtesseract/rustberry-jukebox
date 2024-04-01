//! Raspberry Pi 4 demo.
//! This example makes use the `std` feature
//! and `anyhow` dependency to make error handling more ergonomic.
//!
//! # Connections
//!
//! - 3V3    = VCC
//! - GND    = GND
//! - GPIO9  = MISO
//! - GPIO10 = MOSI
//! - GPIO11 = SCLK (SCK)
//! - GPIO22 = NSS  (SDA)

use embedded_hal_1 as embedded_hal;
use linux_embedded_hal as hal;

use anyhow::{Context,Result};
use embedded_hal::delay::DelayNs;
use hal::spidev::{SpiModeFlags, SpidevOptions};
use hal::{Delay};
use hal::SpidevDevice;
use mfrc522::comm::{blocking::spi::SpiInterface, Interface};
use mfrc522::{Initialized, Mfrc522};

fn main() -> Result<()> {
    let mut delay = Delay;

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
    let vers = mfrc522.version()?;

    println!("VERSION: 0x{:x}", vers);

    assert!(vers == 0x91 || vers == 0x92);

    loop {
        const CARD_UID: [u8; 4] = [34, 246, 178, 171];
        const TAG_UID: [u8; 4] = [128, 170, 179, 76];

        if let Ok(atqa) = mfrc522.reqa() {
            if let Ok(uid) = mfrc522.select(&atqa) {
                println!("UID: {:?}", uid.as_bytes());

                if uid.as_bytes() == &CARD_UID {
                    println!("CARD");
                } else if uid.as_bytes() == &TAG_UID {
                    println!("TAG");
                }

                // handle_authenticate(&mut mfrc522, &uid, |m| {
                //     let data = m.mf_read(1)?;
                //     println!("read {:?}", data);
                //     Ok(())
                // })
                // .ok();
            }
        }

        delay.delay_ms(200u32);
    }
}

// fn handle_authenticate<E, COMM: Interface<Error = E>, F>(
//     mfrc522: &mut Mfrc522<COMM, Initialized>,
//     uid: &mfrc522::Uid,
//     action: F,
// ) -> Result<()>
// where
//     F: FnOnce(&mut Mfrc522<COMM, Initialized>) -> Result<()>,
//     E: std::fmt::Debug + std::marker::Sync + std::marker::Send + 'static,
// {
//     // Use *default* key, this should work on new/empty cards
//     let key = [0xFF; 6];
//     if mfrc522.mf_authenticate(uid, 1, &key).is_ok() {
//         action(mfrc522)?;
//     } else {
//         println!("Could not authenticate");
//     }

//     mfrc522.hlta()?;
//     mfrc522.stop_crypto1()?;
//     Ok(())
// }
