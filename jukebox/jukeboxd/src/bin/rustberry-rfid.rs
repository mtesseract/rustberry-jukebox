use failure::Fallible;

use rustberry::rfid::*;

fn main() -> Fallible<()> {
    let mut rc = RfidController::new()?;
    let card = rc.read_card()?;
    println!("{:?}", card);
    Ok(())
}
