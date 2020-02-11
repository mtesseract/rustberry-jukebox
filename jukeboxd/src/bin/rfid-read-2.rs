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
                loop {
                    let mut reader = tag.new_reader();
                    match reader.read_string() {
                        Ok(s) => {
                            println!("{}", s);
                            std::thread::sleep(std::time::Duration::from_millis(1000));
                        }
                        Err(err) => {
                            println!("err: {:?}", err);
                            break;
                        }
                    }
                }
            }
            Err(err) => {
                println!("err {:?}", err);
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(1000));
    }
}
