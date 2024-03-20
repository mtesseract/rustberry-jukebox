pub mod button;
pub mod playback;

use std::convert::From;

use crate::player::PlaybackRequest;

#[derive(Clone, Debug)]
pub enum Input {
    Button(button::Command),
    Playback(PlaybackRequest),
}

impl From<button::Command> for Input {
    fn from(cmd: button::Command) -> Self {
        Input::Button(cmd)
    }
}

impl From<PlaybackRequest> for Input {
    fn from(req: PlaybackRequest) -> Self {
        Input::Playback(req)
    }
}
