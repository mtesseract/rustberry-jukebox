use failure::Fallible;
use std::io::Read;

use rustberry::rfid::*;

fn main() -> Fallible<()> {
    let mut rc = RfidController::new()?;
    let mut tag = rc.open_tag()?.unwrap();
    println!("{:?}", tag.uid);
    let mut buf: [u8; 3] = [0; 3];
    tag.read_exact(&mut buf).unwrap();
    println!("read: {:?}", buf);
    Ok(())
}
