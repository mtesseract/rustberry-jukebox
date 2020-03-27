use rustberry::effects::http_player::HttpPlayer;

fn main() {
    let player = HttpPlayer::new().unwrap();
    println!("starting...");
    let _ = player.start_playback("https://tortoise.silverratio.net/rustberry/TestRecording.mp3");
    println!("started...");
    std::thread::sleep(std::time::Duration::from_secs(2));
    let _ = player.stop_playback();
    std::thread::sleep(std::time::Duration::from_secs(60));
}
