use spidev::{SpiModeFlags, Spidev, SpidevOptions};
use std::io;

extern crate rfid_rs;

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
    let spi = create_spi().unwrap();
    let mut mfrc522 = rfid_rs::MFRC522 { spi };

    mfrc522.init().expect("Init failed!");

    loop {
        let new_card = mfrc522.new_card_present().is_ok();

        if new_card {
            let uid = match mfrc522.read_card_serial() {
                Ok(u) => u,
                Err(e) => {
                    println!("Could not read card: {:?}", e);
                    continue;
                }
            };

            dbg!(&uid);

            // mfrc522.halt_a().expect("Could not halt");
            // mfrc522.stop_crypto1().expect("Could not stop crypto1");

            std::thread::sleep(std::time::Duration::from_millis(100));
        } else {
            println!("no tag");
        }
    }
}
