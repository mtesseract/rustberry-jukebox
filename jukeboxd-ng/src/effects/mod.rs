/*

input:
- user controls
- playback requests

effects:
- play via spotify, stop via spotify
- led on/off
- shutdown

*/

use slog_scope::{error, info, warn};

pub mod led;
pub mod spotify_player;

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
    // use crate::playback_requests::PlaybackRequestsTransmitter;
    // use crate::spotify_connect::{SpotifyConnector, SupervisorCommands, SupervisorStatus};
    use crate::effects::spotify_player::SpotifyPlayer;
    use crossbeam_channel::Receiver;

    use failure::Fallible;

    struct Interpreter {
        effects: Receiver<Effects>,
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
