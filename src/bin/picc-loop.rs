use anyhow::Result;
use std::thread;
use std::time::Duration;
use tracing::info;
use tracing_subscriber;

use rustberry::components::rfid::RfidController;

fn main() -> Result<()> {
    tracing_subscriber::fmt::init();
    let mut mf = RfidController::new()?;
    loop {
        let res = mf.open_tag();
        info!("res = {:?}", res);
        thread::sleep(Duration::from_millis(200));
    }
}
