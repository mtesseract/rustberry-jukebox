use rustberry::effects::http_player::HttpPlayer;
use rustberry::player::PlaybackHandle;

#[tokio::main]
async fn main() -> Result<(), failure::Error> {
    let player = HttpPlayer::new().unwrap();
    println!("starting...");
    let handle = player
        .start_playback(
            "https://tortoise.silverratio.net/rustberry/TestRecording.mp3",
            None,
        )
        .await
        .unwrap();
    println!("started...");
    std::thread::sleep(std::time::Duration::from_secs(2));
    let _ = handle.stop();
    std::thread::sleep(std::time::Duration::from_secs(60));
    Ok(())
}
