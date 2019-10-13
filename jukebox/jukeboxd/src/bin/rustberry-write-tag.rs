use failure::Fallible;
use std::io::Read;
use serde::Serialize;

use rustberry::rfid::*;
use rustberry::user_requests::UserRequest;

fn main() -> Fallible<()> {
    let mut rc = RfidController::new()?;
    let tag = rc.open_tag()?.unwrap();
    println!("{:?}", tag.uid);
    let mut buf: [u8; 3] = [0; 3];
    let mut tag_writer = tag.new_writer();

    let uri = dialoguer::Input::<String>::new().with_prompt("URI").interact()?;
    let request = serde_json::to_string(&UserRequest::SpotifyUri(uri)).unwrap();

    tag_writer.write_string(&request).expect("write to tag");

    Ok(())
}