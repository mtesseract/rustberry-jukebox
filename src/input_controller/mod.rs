pub mod button;
pub mod event_transformer;
pub mod playback;

use crate::input_controller::button::ButtonEvent;
use crate::player::PlaybackRequest;

#[derive(Clone, Debug)]
pub enum Input {
    Button(ButtonEvent),
    Playback(PlaybackRequest),
}
