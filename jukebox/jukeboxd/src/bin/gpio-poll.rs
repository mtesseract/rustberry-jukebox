use failure::Fallible;

use sysfs_gpio::{Direction, Pin};
use rustberry::rfid::*;
use rustberry::user_requests::UserRequest;

fn main() -> Fallible<()> {
    let line_id = 5;
    let input = Pin::new(line_id);
    input.with_exported(|| {
        input.set_direction(Direction::In)?;
        loop {
            let val = input.get_value()?;
            println!("{}", if val == 0 { "Low" } else { "High" });
            std::thread::sleep(std::time::Duration::from_millis(500));
        }
    })?;

    // let gpio_controller = GpioController::new_from_env()?;
    // println!("Created GPIO Controller");
    // for cmd in gpio_controller {
    //     println!("Received {:?} command from GPIO Controller", cmd);
    //     match cmd {
    //         gpio_sysfs::Command::Shutdown => {
    //             println!("Shutting down");
    //         }
    //     }
    // }
    Ok(())
}
