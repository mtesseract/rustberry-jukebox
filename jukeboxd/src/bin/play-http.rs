use rustberry::effects::http_player::HttpPlayer;

fn main() {
    let mut player = HttpPlayer::new(None).unwrap();
    println!("starting...");
    player.start_playback("https://tortoise.silverratio.net/rustberry/TestRecording.mp3");
    println!("started...");
    std::thread::sleep(std::time::Duration::from_secs(60));
}
