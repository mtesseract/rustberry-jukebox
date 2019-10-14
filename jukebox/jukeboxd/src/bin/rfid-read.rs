use spidev::{Spidev, SpidevOptions, SpiModeFlags};
use std::io;

extern crate rfid_rs;
use rfid_rs::picc;

fn create_spi() -> io::Result<Spidev> {
    let mut spi = Spidev::open("/dev/spidev0.0")?;
    let options = SpidevOptions::new()
        .bits_per_word(8)
        .max_speed_hz(20_000)
        .mode(SpiModeFlags::SPI_MODE_0)
        .build();
    spi.configure(&options)?;
    Ok(spi)
}

fn main() {

    loop {
        let spi = create_spi().unwrap();
        let mut mfrc522 = rfid_rs::MFRC522 { spi };
        mfrc522.init().expect("Init failed!");

        let new_card = mfrc522.new_card_present().is_ok();

        if new_card {
            let key: rfid_rs::MifareKey = [0xffu8; 6];

            let uid = match mfrc522.read_card_serial() {
                Ok(u) => u,
                Err(e) => {
                    println!("Could not read card: {:?}", e);
                    continue
                },
            };

            dbg!(&uid);


            // let mut block = 4;
            // let len = 18;

            // match mfrc522.authenticate(picc::Command::MfAuthKeyA, block, key, &uid) {
            //     Ok(_) => println!("Authenticated card"),
            //     Err(e) => {
            //         println!("Could not authenticate card {:?}", e);
            //         continue
            //     }
            // }
            // match mfrc522.mifare_read(block, len) {
            //     Ok(response) => println!("Read block {}: {:?}", block, response.data),
            //     Err(e) => {
            //         println!("Failed reading block {}: {:?}", block, e);
            //         continue
            //     }
            // }

            // block = 1;

            // match mfrc522.authenticate(picc::Command::MfAuthKeyA, block, key, &uid) {
            //     Ok(_) => println!("Authenticated card"),
            //     Err(e) => {
            //         println!("Could not authenticate card {:?}", e);
            //         continue
            //     }
            // }
            // match mfrc522.mifare_read(block, len) {
            //     Ok(response) => println!("Read block {}: {:?}", block, response.data),
            //     Err(e) => {
            //         println!("Failed reading block {}: {:?}", block, e);
            //         continue
            //     }
            // }

            mfrc522.halt_a().expect("Could not halt");
            mfrc522.stop_crypto1().expect("Could not stop crypto1");

            std::thread::sleep(std::time::Duration::from_secs(1));
        }
    }
}
