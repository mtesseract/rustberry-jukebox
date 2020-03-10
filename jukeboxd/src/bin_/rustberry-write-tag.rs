use failure::Fallible;

use rustberry::playback_requests::PlaybackRequest;
use rustberry::rfid::*;

fn main() -> Fallible<()> {
    let mut rc = RfidController::new()?;
    let tag = rc.open_tag()?.unwrap();
    println!("{:?}", tag.uid);
    let mut tag_writer = tag.new_writer();

    let uri = dialoguer::Input::<String>::new()
        .with_prompt("URI")
        .interact()?;
    let request = serde_json::to_string(&PlaybackRequest::SpotifyUri(uri)).unwrap();

    tag_writer.write_string(&request).expect("write to tag");

    Ok(())
}
