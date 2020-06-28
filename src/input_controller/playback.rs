use failure::Fallible;
use futures::future::AbortHandle;
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

pub struct Handle {
    channel: Receiver<PlaybackRequest>,
    abortable_handle: AbortHandle,
}

impl Drop for Handle {
    fn drop(&mut self) {
        info!("Dropping Playback Input Handle, terminating Playback Input Controller");
        self.abortable_handle.abort();
    }
}

impl Handle {
    // pub fn channel(self) -> Receiver<PlaybackRequest> {
    //     self.channel
    // }
    pub async fn recv(&mut self) -> Option<PlaybackRequest> {
        self.channel.recv().await
    }
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
            let (tx, rx) = channel(128);
            let picc = RfidController::new()?;
            let transmitter = Self { picc, tx };
            let (f, abortable_handle) =
                futures::future::abortable(async move { transmitter.run().await.unwrap() });
            tokio::spawn(f);
            Ok(Handle {
                channel: rx,
                abortable_handle,
            })
        }

        async fn run(self) -> Fallible<()> {
            let mut last_uid: Option<String> = None;

            loop {
                let mut picc2 = self.picc.clone();
                match tokio::task::spawn_blocking(move || picc2.open_tag()).await? {
                    Err(err) => {
                        // Do not change playback state in this case.
                        warn!("Failed to open RFID tag: {}", err);
                        tokio::time::delay_for(std::time::Duration::from_millis(80)).await;
                    }
                    Ok(None) => {
                        if last_uid.is_some() {
                            info!("RFID Tag gone");
                            last_uid = None;
                            let mut tx = self.tx.clone();
                            if let Err(err) = tx.send(PlaybackRequest::Stop).await {
                                error!("Failed to transmit Playback Stop Request: {}", err);
                            }
                            tokio::time::delay_for(std::time::Duration::from_millis(80)).await;
                        }
                    }
                    Ok(Some(tag)) => {
                        let current_uid = format!("{:?}", tag.uid);
                        if last_uid != Some(current_uid.clone()) {
                            // new tag!
                            if let Err(err) =
                                Self::handle_tag(tag.clone(), &mut self.tx.clone()).await
                            {
                                error!("Failed to handle tag: {}", err);
                                // tokio::time::delay_for(Duration::from_millis(80)).await;
                                // continue;
                            }
                            last_uid = Some(current_uid);
                        }

                        // wait for card status change
                        loop {
                            let tag2 = tag.clone();
                            let mut reader =
                                tokio::task::spawn_blocking(move || tag2.new_reader()).await?;
                            if let Err(_err) =
                                tokio::task::spawn_blocking(move || reader.tag_still_readable())
                                    .await?
                            {
                                tokio::time::delay_for(std::time::Duration::from_millis(80)).await;
                                break;
                            } else {
                                tokio::time::delay_for(std::time::Duration::from_millis(80)).await;
                            }
                        }
                    }
                }
            }
        }

        async fn handle_tag(tag: Tag, tx: &mut Sender<PlaybackRequest>) -> Fallible<()> {
            let mut tag_reader = tokio::task::spawn_blocking(move || tag.new_reader()).await?;
            let request_string =
                tokio::task::spawn_blocking(move || tag_reader.read_string()).await??;
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
            tx.send(PlaybackRequest::Start(request_deserialized))
                .await?;
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
