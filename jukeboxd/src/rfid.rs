use failure::Fallible;
use slog_scope::{error, info};
use spidev::{SpiModeFlags, Spidev, SpidevOptions};
use std::io::{Read, Write};
use std::sync::{Arc, Mutex};

use rfid_rs::{picc, Uid, MFRC522};

#[derive(Clone)]
pub struct RfidController {
    pub mfrc522: Arc<Mutex<MFRC522>>,
}

pub struct Tag {
    pub uid: Arc<Uid>,
    pub mfrc522: Arc<Mutex<MFRC522>>,
}

// impl Drop for TagReader {
//     fn drop(&mut self) {
//         let mut mfrc522 = self.mfrc522.lock().unwrap();
//         if let Err(err) = mfrc522.halt_a() {
//             error!("Could not halt MFRC522: {:?}", err);
//         }
//         if let Err(err) = mfrc522.stop_crypto1() {
//             error!("Could not stop crypto1 for MFRC522: {:?}", err);
//         }
//     }
// }

// impl Drop for TagWriter {
//     fn drop(&mut self) {
//         let mut mfrc522 = self.mfrc522.lock().unwrap();
//         if let Err(err) = mfrc522.halt_a() {
//             error!("Could not halt MFRC522: {:?}", err);
//         }
//         if let Err(err) = mfrc522.stop_crypto1() {
//             error!("Could not stop crypto1 for MFRC522: {:?}", err);
//         }
//     }
// }

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

        let mfrc522 = rfid_rs::MFRC522 { spi };
        info!("Create new MFRC522 Controller");
        Ok(RfidController {
            mfrc522: Arc::new(Mutex::new(mfrc522)),
        })
    }

    pub fn try_open_tag(&mut self) -> Result<Tag, rfid_rs::Error> {
        let mut mfrc522 = self.mfrc522.lock().unwrap();
        mfrc522.init().map_err(|err| {
            error!("Failed to initialize MFRC522");
            std::io::Error::new(std::io::ErrorKind::Other, err)
        })?;

        mfrc522.new_card_present()?;
        let uid = mfrc522.read_card_serial()?;
        Ok(Tag {
            uid: Arc::new(uid),
            mfrc522: Arc::clone(&self.mfrc522),
        })
    }

    pub fn open_tag(&mut self) -> Fallible<Option<Tag>> {
        match self.try_open_tag() {
            Ok(tag) => Ok(Some(tag)),
            Err(rfid_rs::Error::Timeout) => Ok(None),
            Err(err) => Err(err.into()),
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

pub const MIFARE_KEY_A: rfid_rs::MifareKey = [0xffu8; 6];

impl Write for TagWriter {
    fn write(&mut self, buf: &[u8]) -> Result<usize, std::io::Error> {
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

        for block in buf[n_to_skip..].chunks(N_BLOCK_SIZE as usize) {
            if block.len() == N_BLOCK_SIZE as usize {
                // Another complete block.
                let mut mfrc522 = mfrc522.lock().unwrap();

                mfrc522
                    .authenticate(
                        picc::Command::MfAuthKeyA,
                        DATA_BLOCKS[self.current_block as usize],
                        MIFARE_KEY_A,
                        &(*self.uid),
                    )
                    .map_err(|err| {
                        error!("Failed to authenticate RFID tag during writing: {:?}", err);
                        std::io::Error::new(std::io::ErrorKind::Other, err)
                    })?;

                mfrc522
                    .mifare_write(DATA_BLOCKS[self.current_block as usize], &block)
                    .map_err(|err| {
                        error!("Failed to write data block to RFID tag: {:?}", err);
                        std::io::Error::new(std::io::ErrorKind::Other, err)
                    })?;

                self.current_block += 1;
            } else {
                // Partial block.
                self.buffered_data[0..block.len()].copy_from_slice(&block);
                self.current_pos_in_buffered_data += block.len() as u8;
            }
        }

        Ok(buf.len())
    }

    fn flush(&mut self) -> Result<(), std::io::Error> {
        let mut mfrc522 = self.mfrc522.lock().unwrap();

        if self.current_pos_in_buffered_data > 0 {
            mfrc522
                .authenticate(
                    picc::Command::MfAuthKeyA,
                    DATA_BLOCKS[self.current_block as usize],
                    MIFARE_KEY_A,
                    &(*self.uid),
                )
                .map_err(|err| {
                    error!("Failed to authenticate RFID tag during flushing");
                    std::io::Error::new(std::io::ErrorKind::Other, err)
                })?;

            let mut buffer: [u8; N_BLOCK_SIZE as usize] = [0; N_BLOCK_SIZE as usize];
            buffer[..self.current_pos_in_buffered_data as usize]
                .copy_from_slice(&self.buffered_data[..self.current_pos_in_buffered_data as usize]);

            mfrc522
                .mifare_write(DATA_BLOCKS[self.current_block as usize], &buffer)
                .map_err(|err| {
                    error!("Failed to write data block to RFID tag during flushing");
                    std::io::Error::new(std::io::ErrorKind::Other, err)
                })?;
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
        let string = rmp::decode::read_str(self, &mut bytes)
            .map_err(|err| std::io::Error::new(std::io::ErrorKind::Other, err.to_string()))?;
        Ok(string.to_string().clone())
    }
    pub fn test_read_byte(&mut self) -> Result<(), std::io::Error> {
        let mut bytes: [u8; 1] = [0];
        let _ = self
            .read_exact(&mut bytes)
            .map_err(|err| std::io::Error::new(std::io::ErrorKind::Other, err.to_string()))?;
        Ok(())
        // let mut bytes: [u8; 1024] = [0; 1024];
        // let string = rmp::decode::read_str(self, &mut bytes)
        //     .map_err(|err| std::io::Error::new(std::io::ErrorKind::Other, err.to_string()))?;
        // Ok(())
    }
}

impl TagWriter {
    pub fn write_string(&mut self, s: &str) -> Result<(), std::io::Error> {
        rmp::encode::write_str(self, s)
            .map_err(|err| std::io::Error::new(std::io::ErrorKind::Other, err.to_string()))?;
        self.flush()
    }
}

impl Read for TagReader {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, std::io::Error> {
        let mut mfrc522 = self.mfrc522.lock().unwrap();

        if self.current_block == N_BLOCKS {
            return Ok(0);
        }

        // Authenticate current block.
        mfrc522
            .authenticate(
                picc::Command::MfAuthKeyA,
                DATA_BLOCKS[self.current_block as usize],
                MIFARE_KEY_A,
                &self.uid,
            )
            .map_err(|err| {
                error!("Failed to authenticate RFID tag during reading");
                std::io::Error::new(std::io::ErrorKind::Other, err)
            })?;

        // Read current block.
        let response = (*mfrc522)
            .mifare_read(DATA_BLOCKS[self.current_block as usize], N_BLOCK_SIZE + 2)
            .map_err(|err| {
                error!("Failed to read data block from RFID tag");
                std::io::Error::new(std::io::ErrorKind::Other, err)
            })?;

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
