use failure::Fallible;
use slog::{self, o, Drain};
// use slog_async;
use slog_scope::{error, info, warn};
use slog_term;

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

fn main() -> Fallible<()> {
    let decorator = slog_term::TermDecorator::new().build();
    let drain = slog_term::FullFormat::new(decorator).build().fuse();
    let drain = slog_async::Async::new(drain).build().fuse();
    let logger = slog::Logger::root(drain, o!());
    let _guard = slog_scope::set_global_logger(logger);
    slog_scope::scope(&slog_scope::logger().new(o!()), || run())
}
