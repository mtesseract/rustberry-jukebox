use failure::Fallible;
use std::io::Read;

use rustberry::rfid::*;

fn main() -> Fallible<()> {
    let mut rc = RfidController::new()?;
    let tag = rc.open_tag()?.unwrap();
    println!("{:?}", tag.uid);
    let mut buf: [u8; 3] = [0; 3];
    let mut tag_reader = tag.new_reader();
    tag_reader.read_exact(&mut buf).unwrap();
    println!("read: {:?}", buf);
    Ok(())
}
