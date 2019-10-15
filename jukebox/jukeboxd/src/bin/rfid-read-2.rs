use rustberry::rfid::*;

fn main() {
    let mut mfrc522 = RfidController::new().expect("new rfidcontroller");
    loop {
        match mfrc522.open_tag() {
            Ok(None) => {
                println!("no tag");
            }
            Ok(Some(tag)) => {
                println!("tag {:?}", tag.uid);
            }
            Err(err) => {
                println!("err {:?}", err);
            }
        }
        std::thread::sleep(std::time::Duration::from_secs(1));
    }
}
