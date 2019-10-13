use super::*;

use failure::Fallible;
use spidev::{SpiModeFlags, Spidev, SpidevOptions};
use std::io::{self, Read, Write};
use std::sync::{Arc, Mutex};

use rfid_rs::{Uid, picc, MFRC522};

#[derive(Clone)]
pub struct RfidController {
    mfrc522: Arc<Mutex<MFRC522>>,
}

pub struct Tag {
    pub uid: Uid,
    pub mfrc522: Arc<Mutex<MFRC522>>,
    pub current_block: u8,
    pub current_pos_in_block: u8,
}

const N_BLOCKS: u8 = 4;
const N_BLOCK_SIZE: u8 = 16;

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

        Ok(RfidController {
            mfrc522: Arc::new(Mutex::new(mfrc522)),
        })
    }

    pub fn open_tag(&mut self) -> Fallible<Option<Tag>> {
        let mut mfrc522 = self.mfrc522.lock().unwrap();
        let new_card = (*mfrc522).new_card_present().is_ok();
        if new_card {
            let uid = (*mfrc522).read_card_serial().expect("read_card_serial");
            println!("uid = {:?}", uid);

            Ok(Some(Tag {
                uid,
                mfrc522: Arc::clone(&self.mfrc522),
                current_block: 0,
                current_pos_in_block: 0,
            }))
        } else {
            println!("new_card_present() returned false");
            Ok(None)
        }
    }
}

impl Read for Tag {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, std::io::Error> {
        let mut mfrc522 = self.mfrc522.lock().unwrap();
        let key: rfid_rs::MifareKey = [0xffu8; 6];
        // let bytes_to_read = min_bytes_to_read; // FIXME
        // let block: [u8; N_BLOCK_SIZE] = [0; N_BLOCK_SIZE];

        if self.current_block == N_BLOCKS {
            return Ok(0);
        }

        // Authenticate current block.
        (*mfrc522)
            .authenticate(
                picc::Command::MfAuthKeyA,
                self.current_block,
                key,
                &self.uid,
            )
            .expect("authenticate");

        println!("Authenticated card");

        // Read current block.
        let response = (*mfrc522)
            .mifare_read(self.current_block, N_BLOCK_SIZE + 2)
            .expect("mifare_read");

        // println!("Read block {}: {:?}", block, response.data);

        let bytes_to_copy = std::cmp::min(buf.len(), (N_BLOCK_SIZE - self.current_pos_in_block) as usize) as u8;
        dbg!(buf.len());
        dbg!(bytes_to_copy);
        dbg!(self.current_pos_in_block);

        let src: &[u8] = &response.data[self.current_pos_in_block as usize.. (self.current_pos_in_block + bytes_to_copy) as usize];
        buf[..bytes_to_copy as usize].copy_from_slice(src);

        self.current_block += 1;
        self.current_pos_in_block = (self.current_pos_in_block + bytes_to_copy) % N_BLOCK_SIZE;
        Ok(bytes_to_copy as usize)
    }
}

//     pub fn read_card(&mut self) -> Fallible<Option<String>> {

//             let mut block = 8;
//             let len = 18;

//         let new_card = self.mfrc522.new_card_present().is_ok();
//         if new_card {
//             let uid = self.mfrc522.read_card_serial().expect("read_card_serial");
//             println!("uid = {:?}", uid);

//             self.mfrc522.authenticate(picc::Command::MfAuthKeyA, block, key, &uid).expect("authenticate");
//             println!("Authenticated card");

//             let response = self.mfrc522.mifare_read(block, len).expect("mifare_read");
//             println!("Read block {}: {:?}", block, response.data);

//             let s = std::str::from_utf8(&response.data).expect("from utf8");
//             Ok(Some(s.to_string()))
//         } else {
//             println!("new_card_present() returned false");
//             Ok(None)
//         }
//     }
// }
