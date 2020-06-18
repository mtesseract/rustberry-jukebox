use std::time::Duration;

// use crossbeam_channel::{self, Receiver, Sender};
use failure::Fallible;
use slog_scope::{error, info, warn};
use tokio::sync::mpsc::{channel, Receiver, Sender};

use crate::player::{PlaybackRequest, PlaybackResource};

#[cfg(test)]
mod test {
    use super::*;
    #[test]
    fn test_user_request_spotify_uri_serialization() {
        let user_req = PlaybackResource::SpotifyUri("foo".to_string());
        let serialized = serde_json::to_string(&user_req).unwrap();
        assert_eq!(serialized, "{\"SpotifyUri\":\"foo\"}".to_string());
    }
}

use tokio::sync::oneshot;

pub struct Handle {
    channel: Receiver<PlaybackRequest>,
    _abort: oneshot::Sender<()>,
}

impl Handle {
    pub fn channel(self) -> Receiver<PlaybackRequest> {
        self.channel
    }

    // pub fn abort(self) {
    //     if let Err(err) = self.abort.send(()) {
    //         error!("Failed to terminate playback controller: {:?}", err);
    //     } else {
    //         info!("Terminated playback controller");
    //     }
    // }
}

pub mod rfid {
    use crate::components::rfid::*;

    use super::*;

    pub struct PlaybackRequestTransmitterRfid {
        picc: RfidController,
        tx: Sender<PlaybackRequest>,
    }

    impl PlaybackRequestTransmitterRfid {
        pub fn new() -> Fallible<Handle> {
            let (tx, rx) = channel(1);
            let (os_tx, os_rx) = oneshot::channel();
            let picc = RfidController::new()?;
            let transmitter = Self { picc, tx };
            std::thread::Builder::new()
                .name("playback-transmitter".to_string())
                .spawn(move || transmitter.run(os_rx).unwrap())?;
            Ok(Handle {
                channel: rx,
                _abort: os_tx,
            })
        }

        fn run(mut self, mut os_rx: oneshot::Receiver<()>) -> Fallible<()> {
            let mut last_uid: Option<String> = None;

            loop {
                if let Err(tokio::sync::oneshot::error::TryRecvError::Closed) = os_rx.try_recv() {
                    info!("Terminating Playback Controller due to closed channel");
                    return Ok(());
                }

                match self.picc.open_tag() {
                    Err(err) => {
                        // Do not change playback state in this case.
                        warn!("Failed to open RFID tag: {}", err);
                        std::thread::sleep(std::time::Duration::from_millis(80));
                    }
                    Ok(None) => {
                        if last_uid.is_some() {
                            info!("RFID Tag gone");
                            last_uid = None;
                            let mut tx = self.tx.clone();
                            futures::executor::block_on(tx.send(PlaybackRequest::Stop));
                            std::thread::sleep(std::time::Duration::from_millis(80));
                        }
                    }
                    Ok(Some(tag)) => {
                        let current_uid = format!("{:?}", tag.uid);
                        if last_uid != Some(current_uid.clone()) {
                            // new tag!
                            if let Err(err) = Self::handle_tag(&tag, &mut self.tx.clone()) {
                                error!("Failed to handle tag: {}", err);
                                std::thread::sleep(Duration::from_millis(80));
                                continue;
                            }
                            last_uid = Some(current_uid);
                        }

                        // wait for card status change
                        loop {
                            let mut reader = tag.new_reader();
                            if let Err(_err) = reader.tag_still_readable() {
                                std::thread::sleep(std::time::Duration::from_millis(80));
                                break;
                            } else {
                                std::thread::sleep(std::time::Duration::from_millis(80));
                            }
                        }
                    }
                }
            }
        }

        fn handle_tag(tag: &Tag, tx: &mut Sender<PlaybackRequest>) -> Fallible<()> {
            let mut tag_reader = tag.new_reader();
            let request_string = tag_reader.read_string()?;
            let request_deserialized: PlaybackResource = match serde_json::from_str(&request_string)
            {
                Ok(deserialized) => deserialized,
                Err(err) => {
                    error!(
                        "Failed to deserialize RFID tag string `{}`: {}",
                        request_string, err
                    );
                    return Err(err.into());
                }
            };
            futures::executor::block_on(tx.send(PlaybackRequest::Start(request_deserialized)))?;
            Ok(())
        }
    }
}

// pub mod stdin {
//     use super::*;

//     pub struct PlaybackRequestTransmitterStdin<T> {
//         tx: Sender<Option<T>>,
//     }

//     impl<T: DeserializeOwned + Clone + std::fmt::Debug + PartialEq> PlaybackRequestTransmitterStdin<T> {
//         pub fn new<F>(msg_transformer: F) -> Fallible<Handle<T>>
//         where
//             F: Fn(PlaybackRequest) -> Option<T> + 'static + Send + Sync,
//         {
//             let (tx, rx) = crossbeam_channel::bounded(1);
//             let transmitter = Self { tx };
//             transmitter.run(msg_transformer)?;
//             Ok(Handle { channel: rx })
//         }

//         fn run<F>(&self, msg_transformer: F) -> Fallible<()>
//         where
//             F: Fn(PlaybackRequest) -> Option<T> + 'static + Send,
//         {
//             let mut last: Option<PlaybackRequest> = None;

//             let stdin = std::io::stdin();
//             for line in stdin.lock().lines() {
//                 if let Ok(ref line) = line {
//                     let req: Option<PlaybackRequest> = if line == "" {
//                         None
//                     } else {
//                         match serde_json::from_str(line) {
//                             Ok(deserialized) => Some(deserialized),
//                             Err(err) => {
//                                 error!("Failed to deserialize line `{}`: {}", line, err);
//                                 None
//                             }
//                         }
//                     };
//                     if last != req {
//                         if let Some(transformed_req) = req.clone().and_then(&msg_transformer) {
//                             self.tx.send(Some(transformed_req)).unwrap();
//                         }
//                         last = req;
//                     }
//                 }
//             }

//             panic!();
//         }
//     }
// }
