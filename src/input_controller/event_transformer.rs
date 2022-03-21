use crate::input_controller::{
    button::{ButtonEvent, Command},
    Input,
};

pub struct EventTransformer {
    volume_up_pressed: bool,
    skip_volume_up_emitting: bool,
    volume_down_pressed: bool,
    skip_volume_down_emitting: bool,
}

impl EventTransformer {
    pub fn new() -> Self {
        EventTransformer {
            volume_down_pressed: false,
            volume_up_pressed: false,
            skip_volume_down_emitting: false,
            skip_volume_up_emitting: false,
        }
    }
    pub fn transform(&mut self, event: &Input) -> Vec<Command> {
        match event {
            Input::Button(ButtonEvent::ShutdownPress) => vec![Command::Shutdown],
            Input::Button(ButtonEvent::ShutdownRelease) => vec![],
            Input::Button(ButtonEvent::VolumeUpPress) => {
                self.volume_up_pressed = true;
                if self.volume_down_pressed {
                    self.skip_volume_down_emitting = true;
                    self.skip_volume_up_emitting = true;
                    vec![Command::LockPlayer]
                } else {
                    vec![]
                }
            }
            Input::Button(ButtonEvent::VolumeUpRelease) => {
                self.volume_up_pressed = false;
                if self.skip_volume_up_emitting {
                    self.skip_volume_up_emitting = false;
                    return vec![];
                }
                vec![Command::VolumeUp]
            }
            Input::Button(ButtonEvent::VolumeDownPress) => {
                self.volume_down_pressed = true;
                if self.volume_up_pressed {
                    self.skip_volume_down_emitting = true;
                    self.skip_volume_up_emitting = true;
                    vec![Command::LockPlayer]
                } else {
                    vec![]
                }
            }
            Input::Button(ButtonEvent::VolumeDownRelease) => {
                self.volume_down_pressed = false;
                if self.skip_volume_down_emitting {
                    self.skip_volume_down_emitting = false;
                    return vec![];
                }
                vec![Command::VolumeDown]
            }
            Input::Button(ButtonEvent::PauseContinuePress) => vec![Command::PauseContinue],
            Input::Button(ButtonEvent::PauseContinueRelease) => vec![],
            Input::Playback(req) => vec![Command::Playback(req.clone())],
        }
    }
}
