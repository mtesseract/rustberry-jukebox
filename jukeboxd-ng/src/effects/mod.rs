/*

input:
- user controls
- playback requests

effects:
- play via spotify, stop via spotify
- led on/off
- shutdown

*/

pub mod http_player;
pub mod led;
pub mod spotify;

use crate::config::Config;
use crossbeam_channel::Receiver;
use failure::Fallible;
use led::{Led, LedController};
use slog_scope::{error, info, warn};
use spotify::player::SpotifyPlayer;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effects {
    PlayHttp { url: String },
    StopHttp,
    PlaySpotify { spotify_uri: String },
    NewPlaySpotify { spotify_uri: String },
    NewStopSpotify,
    StopSpotify,
    LedOn,
    LedOff,
    VolumeUp,
    VolumeDown,
    GenericCommand(String),
}

use Effects::*;

pub struct ProdInterpreter {
    spotify_player: SpotifyPlayer,
    led_controller: Box<dyn LedController + 'static + Send>,
    config: Config,
}

impl ProdInterpreter {
    pub fn new(config: &Config) -> Fallible<Self> {
        let config = config.clone();
        let spotify_player = SpotifyPlayer::new(&config);
        let led_controller = Box::new(led::gpio_cdev::GpioCdev::new()?);
        Ok(ProdInterpreter {
            spotify_player,
            led_controller,
            config,
        })
    }

    fn handle(&self, effect: &Effects) -> Fallible<()> {
        match effect {
            PlaySpotify { spotify_uri } => {
                self.spotify_player.start_playback(&spotify_uri)?;
                Ok(())
            }
            StopSpotify => {
                self.spotify_player.stop_playback()?;
                Ok(())
            }
            LedOn => self.led_controller.switch_on(Led::Playback),
            LedOff => self.led_controller.switch_off(Led::Playback),
            VolumeUp => unimplemented!(),
            VolumeDown => unimplemented!(),
            GenericCommand(cmd) => unimplemented!(),
            Shutdown => unimplemented!(),
        }
    }

    pub fn run(&self, channel: Receiver<Effects>) -> Fallible<()> {
        // FIXME
        for effect in channel.iter() {
            if let Err(err) = self.handle(&effect) {
                error!("Failed to execute effect {:?}: {}", effect, err);
            }
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
