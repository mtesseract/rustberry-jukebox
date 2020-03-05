pub mod button;
pub mod playback;

#[derive(Clone, Debug)]
pub enum Input {
    Button(button::Command),
    Playback(Option<playback::PlaybackRequest>),
}
