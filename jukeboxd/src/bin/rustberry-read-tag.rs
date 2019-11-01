use failure::Fallible;

use rustberry::playback_requests::PlaybackRequest;
use rustberry::rfid::*;

fn main() -> Fallible<()> {
    let mut rc = RfidController::new()?;
    let tag = rc.open_tag()?.unwrap();
    println!("{:?}", tag.uid);
    let mut tag_reader = tag.new_reader();
    let s = tag_reader.read_string().expect("read_string");
    let req: PlaybackRequest = serde_json::from_str(&s).expect("PlaybackRequest Deserialization");
    dbg!(&req);
    Ok(())
}
