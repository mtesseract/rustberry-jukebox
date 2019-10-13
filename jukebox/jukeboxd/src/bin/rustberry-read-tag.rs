use failure::Fallible;
use std::io::Read;
use serde::Serialize;

use rustberry::rfid::*;
use rustberry::user_requests::UserRequest;

fn main() -> Fallible<()> {
    let mut rc = RfidController::new()?;
    let tag = rc.open_tag()?.unwrap();
    println!("{:?}", tag.uid);
    let mut tag_reader = tag.new_reader();
    let s = tag_reader.read_string().expect("read_string");
    println!("s = {}", s);

    Ok(())
}
