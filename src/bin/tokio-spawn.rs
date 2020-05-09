fn main() {
    // Create the runtime
    // let rt = tokio::runtime::Builder::new().threaded_scheduler().enable_all().build().unwrap();
    // let rt = tokio::runtime::Builder::new().basic_scheduler().enable_all().build().unwrap();
    let rt = tokio::runtime::Runtime::new().unwrap(); // working

    // Spawn a future onto the runtime
    rt.spawn(async {
        loop {
            println!("now running on a worker thread");
            tokio::time::delay_for(std::time::Duration::from_secs(1)).await;
        }
    });

    loop {
        std::thread::sleep(std::time::Duration::from_secs(1));
    }
}
