pub mod button;
pub mod playback;

use crate::player::PlaybackRequest;

#[derive(Clone, Debug)]
pub enum Input {
    Button(button::Command),
    Playback(PlaybackRequest),
}
