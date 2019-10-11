        use super::*;

        use spidev::{SpiModeFlags, Spidev, SpidevOptions};
        use std::io;
        use failure::Fallible;

        use rfid_rs::{picc, MFRC522};

        pub struct RfidController {
            mfrc522: MFRC522,
        }

        impl RfidController {
            pub fn new() -> Fallible<Self> {
                let mut spi = Spidev::open("/dev/spidev1.0")?;
                let options = SpidevOptions::new()
                    .bits_per_word(8)
                    .max_speed_hz(20_000)
                    .mode(SpiModeFlags::SPI_MODE_0)
                    .build();
                spi.configure(&options)?;

                let mut mfrc522 = rfid_rs::MFRC522 { spi };
                mfrc522.init().expect("Init failed!");

                Ok(RfidController {
                    mfrc522,
                })
            }

            pub fn read_card(&mut self) -> Fallible<Option<String>> {
                    let new_card = self.mfrc522.new_card_present().is_ok();
                    if new_card {
                        match self.mfrc522.read_card_serial() {
                            Ok(u) => {
                                println!("New card: {:?}", u);
                                Ok(Some(format!("{:?}", u)))
                            }
                            Err(e) => {
                                println!("Could not read card: {:?}", e);
                                Ok(None)
                            }
                        }
                    } else {
                        println!("new_card_present() returned false");
                        Ok(None)
                    }
            }
        }
