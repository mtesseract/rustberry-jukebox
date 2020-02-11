use rustberry::rfid::*;

fn main() {
    let mut mfrc522 = RfidController::new().expect("new rfidcontroller");
    loop {
        match mfrc522.open_tag() {
            Ok(None) => {}
            Ok(Some(tag)) => {
                println!("tag {:?}", tag.uid);
                let mut reader = tag.new_reader();
                let res = reader.read_string();
                drop(reader);
                match res {
                    Ok(s) => {
                        println!("{}", s);
                        loop {
                            let mut reader = tag.new_reader();
                            match reader.test_read_byte() {
                                Ok(s) => {
                                    std::thread::sleep(std::time::Duration::from_millis(80));
                                }
                                Err(err) => {
                                    println!("err: {}", err);
                                    break;
                                }
                            }
                        }
                    }
                    Err(err) => {
                        println!("err: {}", err);
                    }
                }
            }
            Err(err) => {
                println!("err: {}", err);
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(80));
    }
}
