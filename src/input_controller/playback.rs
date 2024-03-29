use crossbeam_channel::{self, Receiver, Sender};
use failure::Fallible;
use slog_scope::{error, info, warn};

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

pub struct Handle<T> {
    channel: Receiver<T>,
    // thread: Arc<JoinHandle<()>>,
}

impl<T> Handle<T> {
    pub fn channel(&self) -> Receiver<T> {
        self.channel.clone()
    }
}

pub mod rfid {
    use crate::components::rfid::*;

    use super::*;

    pub struct PlaybackRequestTransmitterRfid<T> {
        picc: RfidController,
        tx: Sender<T>,
    }

    impl<T: 'static + Send + Sync + Clone + std::fmt::Debug> PlaybackRequestTransmitterRfid<T> {
        pub fn new<F>(msg_transformer: F) -> Fallible<Handle<T>>
        where
            F: Fn(PlaybackRequest) -> Option<T> + 'static + Send + Sync,
        {
            let (tx, rx) = crossbeam_channel::bounded(1);
            let picc = RfidController::new()?;
            let transmitter = Self { picc, tx };
            std::thread::Builder::new()
                .name("playback-transmitter".to_string())
                .spawn(move || transmitter.run(msg_transformer).unwrap())?;
            Ok(Handle { channel: rx })
        }

        fn run<F>(mut self, msg_transformer: F) -> Fallible<()>
        where
            F: Fn(PlaybackRequest) -> Option<T> + 'static + Send,
        {
            let mut last_uid: Option<String> = None;

            loop {
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
                            if let Some(msg_transformed) = msg_transformer(PlaybackRequest::Stop) {
                                if let Err(err) = self.tx.send(msg_transformed) {
                                    error!("Failed to transmit User Request: {}", err);
                                }
                            }
                            std::thread::sleep(std::time::Duration::from_millis(80));
                        }
                    }
                    Ok(Some(tag)) => {
                        let current_uid = format!("{:?}", tag.uid);
                        if last_uid != Some(current_uid.clone()) {
                            // new tag!
                            info!("Seen RFID Tag {}", current_uid);
                            if let Err(err) = Self::handle_tag(&tag, &msg_transformer, &self.tx) {
                                error!("Failed to handle tag: {}", err);
                                std::thread::sleep(std::time::Duration::from_millis(80));
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

        fn handle_tag<F>(tag: &Tag, msg_transformer: &F, tx: &Sender<T>) -> Fallible<()>
        where
            F: Fn(PlaybackRequest) -> Option<T> + 'static + Send,
        {
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
            if let Some(req_transformed) =
                msg_transformer(PlaybackRequest::Start(request_deserialized.clone()))
            {
                tx.send(req_transformed)?;
            } else {
                info!("Dropping playback request '{:?}'", &request_deserialized);
            }
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
