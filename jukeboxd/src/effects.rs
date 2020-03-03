/*

input:
- user controls
- playback requests

effects:
- play via spotify, stop via spotify
- led on/off
- shutdown

*/
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effects {
    PlaySpotify { spotify_uri: String },
    StopSpotify,
    LedOn,
    LedOff,
    VolumeUp,
    VolumeDown,
    GenericCommand(String),
}

pub mod interpreter {
    use super::*;
    use crate::access_token_provider::{self, AccessTokenProvider, AtpError};
    use crate::button_controller::UserControlTransmitter;
    use crate::playback_requests::PlaybackRequestsTransmitter;
    use crate::spotify_connect::{SpotifyConnector, SupervisorCommands, SupervisorStatus};
    use crate::spotify_player::SpotifyPlayer;

    use failure::Fallible;

    struct Interpreter {
        effects: Receiver<Effects>,
        handler: Box<dyn Fn(Input) + Send>,
        spotify_player: SpotifyPlayer,
    }

    impl Interpreter {
        pub fn new(spotify_player: SpotifyPlayer) {}

        pub fn run(self) -> Fallible<()> {
            use Effects::*;

            for effect in self.effects.iter() {
                info!("Effect: {:?}", effect);

                match effect {
                    PlaySpotify { .. } => {}
                    _ => {
                        // unhandled
                        unimplemented!()
                    }
                }
            }

            Ok(())
        }
    }
}
