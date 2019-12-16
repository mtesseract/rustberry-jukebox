use failure::Fallible;
use slog::{self, o, Drain};
// use slog_async;
use rfid_rs::{picc, Uid, MFRC522};
use slog_scope::{error, info, warn};
use slog_term;
use spidev::{SpiModeFlags, Spidev, SpidevOptions};
use std::sync::{Arc, Mutex};

use rustberry::playback_requests::*;
use rustberry::rfid::*;

fn handle_tag(tag: Tag) -> Fallible<()> {
    let mut tag_reader = tag.new_reader();
    let request_string = tag_reader.read_string()?;
    let request_deserialized: PlaybackRequest = serde_json::from_str(&request_string)?;
    info!("Request: {:?}", request_deserialized);
    Ok(())
}

fn run() -> Fallible<()> {
    let mut rc = RfidController::new()?;
    let mut last_uid: Option<String> = None;
    loop {
        std::thread::sleep(std::time::Duration::from_millis(100));
        match rc.open_tag() {
            Err(err) => {
                // Do not change playback state in this case.
                warn!("Failed to open RFID tag: {}", err);
            }
            Ok(None) => {
                if last_uid.is_some() {
                    info!("RFID Tag gone");
                    last_uid = None;
                }
            }
            Ok(Some(tag)) => {
                let current_uid = format!("{:?}", tag.uid);
                if last_uid == Some(current_uid.clone()) {
                    continue;
                }
                // new tag!
                match handle_tag(tag) {
                    Ok(_) => {
                        last_uid = Some(current_uid);
                    }
                    Err(err) => {
                        error!("Failed to handle tag: {}", err);
                    }
                }
            }
        }
    }
}

fn try_open_tag(
    amfrc522: &mut RfidController,
    check_new_card_present: bool,
) -> Result<Tag, rfid_rs::Error> {
    let mut mfrc522 = amfrc522.mfrc522.lock().unwrap();
    if check_new_card_present {
        mfrc522.new_card_present()?;
    }
    let uid = mfrc522.read_card_serial()?;
    Ok(Tag {
        uid: Arc::new(uid),
        mfrc522: Arc::clone(&amfrc522.mfrc522),
    })
}

fn open_tag(mfrc522: &mut RfidController) -> Fallible<Option<Tag>> {
    match try_open_tag(mfrc522, false) {
        Ok(tag) => Ok(Some(tag)),
        Err(rfid_rs::Error::Timeout) => Ok(None),
        Err(err) => Err(err.into()),
    }
}

fn open_tag_when_none(mfrc522: &mut RfidController) -> Fallible<Option<Tag>> {
    match try_open_tag(mfrc522, true) {
        Ok(tag) => Ok(Some(tag)),
        Err(rfid_rs::Error::Timeout) => Ok(None),
        Err(err) => Err(err.into()),
    }
}

fn run2() -> Fallible<()> {
    let mut rc = RfidController::new()?;
    // let mut spi = Spidev::open("/dev/spidev0.0")?;
    // let options = SpidevOptions::new()
    //     .bits_per_word(8)
    //     .max_speed_hz(20_000)
    //     .mode(SpiModeFlags::SPI_MODE_0)
    //     .build();
    // spi.configure(&options)?;
    // let mut mfrc522 = rfid_rs::MFRC522 { spi };
    {
        let mut mfrc522 = rc.mfrc522.lock().unwrap();
        mfrc522.init().map_err(|err| {
            error!("Failed to initialize MFRC522");
            std::io::Error::new(std::io::ErrorKind::Other, err)
        })?;
    }
    let mut last_uid: Option<String> = None;

    loop {
        std::thread::sleep(std::time::Duration::from_millis(100));
        let res = if last_uid.is_none() {
            open_tag_when_none(&mut rc)
        } else {
            open_tag(&mut rc)
        };
        match res {
            Err(err) => {
                // Do not change playback state in this case.
                warn!("Failed to open RFID tag: {}", err);
            }
            Ok(None) => {
                if last_uid.is_some() {
                    info!("RFID Tag gone");
                    last_uid = None;
                }
            }
            Ok(Some(tag)) => {
                let current_uid = format!("{:?}", tag.uid);
                if last_uid == Some(current_uid.clone()) {
                    continue;
                }
                // new tag!
                match handle_tag(tag) {
                    Ok(_) => {
                        last_uid = Some(current_uid);
                    }
                    Err(err) => {
                        error!("Failed to handle tag: {}", err);
                    }
                }
            }
        }
    }
}

fn main() -> Fallible<()> {
    let decorator = slog_term::TermDecorator::new().build();
    let drain = slog_term::FullFormat::new(decorator).build().fuse();
    let drain = slog_async::Async::new(drain).build().fuse();
    let logger = slog::Logger::root(drain, o!());
    let _guard = slog_scope::set_global_logger(logger);
    slog_scope::scope(&slog_scope::logger().new(o!()), || run2())
}
