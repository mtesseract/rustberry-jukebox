use super::*;

use failure::Fallible;
use slog_scope::{error, warn, info};
use spidev::{SpiModeFlags, Spidev, SpidevOptions};
use std::io::{self, Read, Write};
use std::sync::{Arc, Mutex};

use rfid_rs::{picc, Uid, MFRC522};

#[derive(Clone)]
pub struct RfidController {
    mfrc522: Arc<Mutex<MFRC522>>,
}

pub struct Tag {
    pub uid: Arc<Uid>,
    pub mfrc522: Arc<Mutex<MFRC522>>,
}

impl Drop for TagReader {
    fn drop(&mut self) {
        let mut mfrc522 = self.mfrc522.lock().unwrap();
        mfrc522.halt_a().expect("Could not halt");
        mfrc522.stop_crypto1().expect("Could not stop crypto1");
    }
}

impl Drop for TagWriter {
    fn drop(&mut self) {
        let mut mfrc522 = self.mfrc522.lock().unwrap();
        mfrc522.halt_a().expect("Could not halt");
        mfrc522.stop_crypto1().expect("Could not stop crypto1");
    }
}

pub struct TagReader {
    pub uid: Arc<Uid>,
    pub mfrc522: Arc<Mutex<MFRC522>>,
    pub current_block: u8,
    pub current_pos_in_block: u8,
}

pub struct TagWriter {
    pub uid: Arc<Uid>,
    pub mfrc522: Arc<Mutex<MFRC522>>,
    pub current_block: u8,
    pub buffered_data: [u8; N_BLOCK_SIZE as usize],
    pub current_pos_in_buffered_data: u8,
}

const DATA_BLOCKS: [u8; 9] = [8, 9, 10, 12, 13, 14, 16, 17, 18];
const N_BLOCKS: u8 = 9;
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

        Ok(RfidController {
            mfrc522: Arc::new(Mutex::new(mfrc522)),
        })
    }

    pub fn open_tag(&mut self) -> Fallible<Option<Tag>> {
        let mut mfrc522 = self.mfrc522.lock().unwrap();
        mfrc522.init().expect("Init failed!");
        match mfrc522.new_card_present() {
            Ok(()) => match mfrc522.read_card_serial() {
                Ok(uid) => Ok(Some(Tag {
                    uid: Arc::new(uid),
                    mfrc522: Arc::clone(&self.mfrc522),
                })),
                Err(err) => {
                    warn!("Failed to retrieve RFID tag serial: {:?}", err);
                    Ok(None)
                }
            },
            Err(rfid_rs::Error::Timeout) => Ok(None),
            Err(err) => {
                warn!("Failed to test if RFID tag is present: {:?}", err);
                Ok(None)
            }
        }
    }
}

impl Tag {
    pub fn new_reader(&self) -> TagReader {
        TagReader {
            mfrc522: Arc::clone(&self.mfrc522),
            current_block: 0,
            current_pos_in_block: 0,
            uid: Arc::clone(&self.uid),
        }
    }

    pub fn new_writer(&self) -> TagWriter {
        TagWriter {
            mfrc522: self.mfrc522.clone(),
            current_block: 0,
            buffered_data: [0; N_BLOCK_SIZE as usize],
            current_pos_in_buffered_data: 0,
            uid: Arc::clone(&self.uid),
        }
    }
}

impl Write for TagWriter {
    fn write(&mut self, buf: &[u8]) -> Result<usize, std::io::Error> {
        let key: rfid_rs::MifareKey = [0xffu8; 6];
        let n_to_skip = if self.current_pos_in_buffered_data > 0 {
            // Need to fill currently buffered data first.
            let n_space_left_in_buffered_data =
                (self.current_pos_in_buffered_data as usize..N_BLOCK_SIZE as usize).len();
            let to_copy_into_buffered_data: u8 =
                std::cmp::min(buf.len(), n_space_left_in_buffered_data) as u8;
            self.buffered_data[self.current_pos_in_buffered_data as usize
                ..(self.current_pos_in_buffered_data as usize
                    + to_copy_into_buffered_data as usize)]
                .copy_from_slice(&buf[..to_copy_into_buffered_data as usize]);
            self.current_pos_in_buffered_data += to_copy_into_buffered_data;

            if self.current_pos_in_buffered_data == N_BLOCK_SIZE {
                // Completed a block. flush it and continue.
                self.flush()?;
                to_copy_into_buffered_data as usize
            } else {
                return Ok(buf.len());
            }
        } else {
            0
        };

        let mfrc522 = self.mfrc522.clone();

        buf[n_to_skip..]
            .chunks(N_BLOCK_SIZE as usize)
            .for_each(move |block| {
                if block.len() == N_BLOCK_SIZE as usize {
                    // Another complete block.
                    let mut mfrc522 = mfrc522.lock().unwrap();

                    mfrc522
                        .authenticate(
                            picc::Command::MfAuthKeyA,
                            DATA_BLOCKS[self.current_block as usize],
                            key,
                            &(*self.uid),
                        )
                        .expect("authenticate for writing");

                    mfrc522
                        .mifare_write(DATA_BLOCKS[self.current_block as usize], &block)
                        .expect("mifare_write");

                    self.current_block += 1;
                } else {
                    // Partial block.
                    self.buffered_data[0..block.len()].copy_from_slice(&block);
                    self.current_pos_in_buffered_data += block.len() as u8;
                }
            });
        Ok(buf.len())
    }

    fn flush(&mut self) -> Result<(), std::io::Error> {
        let mut mfrc522 = self.mfrc522.lock().unwrap();
        let key: rfid_rs::MifareKey = [0xffu8; 6];

        if self.current_pos_in_buffered_data > 0 {
            mfrc522
                .authenticate(
                    picc::Command::MfAuthKeyA,
                    DATA_BLOCKS[self.current_block as usize],
                    key,
                    &(*self.uid),
                )
                .expect("authenticate for flushing");

            let mut buffer: [u8; N_BLOCK_SIZE as usize] = [0; N_BLOCK_SIZE as usize];
            buffer[..self.current_pos_in_buffered_data as usize]
                .copy_from_slice(&self.buffered_data[..self.current_pos_in_buffered_data as usize]);

            mfrc522
                .mifare_write(DATA_BLOCKS[self.current_block as usize], &buffer)
                .expect("mifare_write");

            self.current_pos_in_buffered_data = 0;
            self.current_block += 1;
            self.buffered_data
                .copy_from_slice(&[0; N_BLOCK_SIZE as usize]);
        }
        Ok(())
    }
}

impl TagReader {
    pub fn read_string(&mut self) -> Result<String, std::io::Error> {
        let mut bytes: [u8; 1024] = [0; 1024];
        let string = rmp::decode::read_str(self, &mut bytes).unwrap();
        Ok(string.to_string().clone())
    }
}

impl TagWriter {
    pub fn write_string(&mut self, s: &str) -> Result<(), std::io::Error> {
        rmp::encode::write_str(self, s).unwrap();
        self.flush();
        Ok(())
    }
}

impl Read for TagReader {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, std::io::Error> {
        let mut mfrc522 = self.mfrc522.lock().unwrap();
        let key: rfid_rs::MifareKey = [0xffu8; 6];

        if self.current_block == N_BLOCKS {
            return Ok(0);
        }

        // Authenticate current block.
        (*mfrc522)
            .authenticate(
                picc::Command::MfAuthKeyA,
                DATA_BLOCKS[self.current_block as usize],
                key,
                &self.uid,
            )
            .expect("authenticate");

        // Read current block.
        let response = (*mfrc522)
            .mifare_read(DATA_BLOCKS[self.current_block as usize], N_BLOCK_SIZE + 2)
            .expect("mifare_read");

        let bytes_to_copy = std::cmp::min(
            buf.len(),
            (N_BLOCK_SIZE - self.current_pos_in_block) as usize,
        ) as u8;

        let src: &[u8] = &response.data[self.current_pos_in_block as usize
            ..(self.current_pos_in_block + bytes_to_copy) as usize];
        buf[..bytes_to_copy as usize].copy_from_slice(src);

        self.current_pos_in_block = (self.current_pos_in_block + bytes_to_copy) % N_BLOCK_SIZE;
        if self.current_pos_in_block == 0 {
            self.current_block += 1;
        }

        Ok(bytes_to_copy as usize)
    }
}
