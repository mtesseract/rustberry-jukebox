use blinkt;

use std::time::Duration;
use std::{mem, thread};

use blinkt::Blinkt;

fn main() {
    let mut blinkt = Blinkt::new().unwrap();
    let (red, green, blue) = (&mut 255, &mut 0, &mut 0);

    loop {
        blinkt.set_all_pixels(*red, *green, *blue);
        blinkt.show().unwrap();

        thread::sleep(Duration::from_millis(250));

        mem::swap(red, green);
        mem::swap(red, blue);
    }
}
