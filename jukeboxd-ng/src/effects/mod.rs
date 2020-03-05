/*

input:
- user controls
- playback requests

effects:
- play via spotify, stop via spotify
- led on/off
- shutdown

*/

pub mod led;
pub mod spotify_player;

use crossbeam_channel::Receiver;
use failure::Fallible;
use slog_scope::{error, info, warn};
use spotify_player::SpotifyPlayer;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effects {
    PlaySpotify {
        spotify_uri: String,
        access_token: String,
        device_id: String,
    },
    StopSpotify {
        access_token: String,
        device_id: String,
    },
    LedOn,
    LedOff,
    VolumeUp,
    VolumeDown,
    GenericCommand(String),
}

use Effects::*;

struct ProdInterpreter {
    spotify_player: SpotifyPlayer,
}

impl ProdInterpreter {
    pub fn new() -> Fallible<Self> {
        let spotify_player = SpotifyPlayer::new();
        Ok(ProdInterpreter { spotify_player })
    }

    fn handle(&self, effect: Effects) -> Fallible<()> {
        match effect {
            PlaySpotify {
                spotify_uri,
                access_token,
                device_id,
            } => {
                self.spotify_player
                    .start_playback(&access_token, &device_id, &spotify_uri)?;
                Ok(())
            }
            StopSpotify {
                access_token,
                device_id,
            } => {
                self.spotify_player
                    .stop_playback(&access_token, &device_id)?;
                Ok(())
            }
            LedOn => unimplemented!(),
            LedOff => unimplemented!(),
            VolumeUp => unimplemented!(),
            VolumeDown => unimplemented!(),
            GenericCommand(cmd) => unimplemented!(),
            Shutdown => unimplemented!(),
        }
    }

    pub fn run(&self, channel: Receiver<Effects>) -> Fallible<()> {
        // FIXME
        for effect in channel.iter() {
            self.handle(effect)?;
        }
        Ok(())
    }
}

// pub mod interpreter {
//     use super::*;
//     use crate::access_token_provider::{self, AccessTokenProvider, AtpError};
//     use crate::effects::spotify_player::SpotifyPlayer;
//     use crossbeam_channel::Receiver;

//     use failure::Fallible;

//     struct Interpreter {
//         effects: Receiver<Effects>,
//         spotify_player: SpotifyPlayer,
//     }

//     impl Interpreter {
//         pub fn new(spotify_player: SpotifyPlayer) {}

//         pub fn run(self) -> Fallible<()> {
//             use Effects::*;

//             for effect in self.effects.iter() {
//                 info!("Effect: {:?}", effect);

//                 match effect {
//                     PlaySpotify { .. } => {}
//                     _ => {
//                         // unhandled
//                         unimplemented!()
//                     }
//                 }
//             }

//             Ok(())
//         }
//     }
// }
