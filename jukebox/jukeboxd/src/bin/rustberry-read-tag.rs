use failure::Fallible;
use serde::Serialize;
use std::io::Read;

use rustberry::rfid::*;
use rustberry::user_requests::UserRequest;

fn main() -> Fallible<()> {
    let mut rc = RfidController::new()?;
    let tag = rc.open_tag()?.unwrap();
    println!("{:?}", tag.uid);
    let mut tag_reader = tag.new_reader();
    let s = tag_reader.read_string().expect("read_string");
    let req: UserRequest = serde_json::from_str(&s).expect("UserRequest Deserialization");
    dbg!(&req);
    Ok(())
}
