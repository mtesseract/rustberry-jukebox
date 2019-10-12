
use super::*;

use failure::Fallible;
use spidev::{SpiModeFlags, Spidev, SpidevOptions};
use std::io;

use rfid_rs::{picc, MFRC522};

pub struct RfidController {
    mfrc522: MFRC522,
}

impl RfidController {
    pub fn new() -> Fallible<Self> {
        let mut spi = Spidev::open("/dev/spidev0.0")?;
        let options = SpidevOptions::new()
            .bits_per_word(8)
            .max_speed_hz(20_000)
            .mode(SpiModeFlags::SPI_MODE_0)
            .build();
        spi.configure(&options)?;

        let mut mfrc522 = rfid_rs::MFRC522 { spi };
        mfrc522.init().expect("Init failed!");

        Ok(RfidController { mfrc522 })
    }

    pub fn read_card(&mut self) -> Fallible<Option<String>> {

            let mut block = 4;
            let len = 18;

            let key: rfid_rs::MifareKey = [0xffu8; 6];

        let new_card = self.mfrc522.new_card_present().is_ok();
        if new_card {
            let uid = self.mfrc522.read_card_serial().expect("read_card_serial");
            println!("uid = {:?}", uid);

            self.mfrc522.authenticate(picc::Command::MfAuthKeyA, block, key, &uid).expect("authenticate");
            println!("Authenticated card");

            let response = self.mfrc522.mifare_read(block, len).expect("mifare_read");
            println!("Read block {}: {:?}", block, response.data);

            let s = std::str::from_utf8(&response.data).expect("from utf8");
            Ok(Some(s.to_string()))
        } else {
            println!("new_card_present() returned false");
            Ok(None)
        }
    }
}
