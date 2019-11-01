use failure::Fallible;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use slog_scope::{error, warn, info};
use std::env;
use std::io::BufRead;
use std::sync::mpsc;
use std::sync::mpsc::{Receiver, Sender};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum PlaybackRequest {
    SpotifyUri(String),
}

#[test]
mod test {
    use super::*;
    #[test]
    fn test_user_request_spotify_uri_serialization() {
        let user_req = PlaybackRequest::SpotifyUri("foo".to_string());
        let serialized = serde_json::to_string(&user_req).unwrap();
        assert_eq!(serialized, "{\"SpotifyUri\":\"foo\"}".to_string());
    }
}

pub trait PlaybackRequestTransmitterBackend<T: DeserializeOwned> {
    fn run(&mut self, tx: Sender<Option<T>>) -> Fallible<()>;
}

pub struct PlaybackRequests<T>
where
    T: Sync + Send + 'static,
{
    rx: Receiver<Option<T>>,
}

pub struct PlaybackRequestsTransmitter<
    T: DeserializeOwned + std::fmt::Debug,
    TB: PlaybackRequestTransmitterBackend<T>,
> {
    backend: TB,
    first_req: Option<T>,
}

impl<T: std::fmt::Debug + DeserializeOwned + Clone, TB: PlaybackRequestTransmitterBackend<T>>
    PlaybackRequestsTransmitter<T, TB>
{
    pub fn new(backend: TB) -> Fallible<Self> {
        let first_req = match env::var("FIRST_REQUEST") {
            Ok(first_req) => match serde_json::from_str(&first_req) {
                Ok(first_req) => Some(first_req),
                Err(err) => {
                    error!(
                        "Failed to deserialize first request '{}': {}",
                        first_req, err
                    );
                    None
                }
            },
            Err(env::VarError::NotPresent) => None,
            Err(err) => {
                error!("Failed to retrieve FIRST_REQUEST: {}", err.to_string());
                None
            }
        };
        Ok(PlaybackRequestsTransmitter { backend, first_req })
    }

    pub fn run(&mut self, tx: Sender<Option<T>>) -> Fallible<()> {
        if let Some(ref first_req) = self.first_req {
            let first_req = (*first_req).clone();
            info!(
                "Automatically transmitting first user request: {:?}",
                &first_req
            );
            if let Err(err) = tx.send(Some(first_req.clone())) {
                error!("Failed to transmit first request {:?}: {}", first_req, err);
            }
        }
        self.backend.run(tx)
    }
}

pub mod stdin {
    use super::*;

    pub struct PlaybackRequestTransmitterStdin<T> {
        _phantom: Option<T>,
    }

    impl<T: DeserializeOwned + std::fmt::Debug> PlaybackRequestTransmitterStdin<T> {
        pub fn new() -> Fallible<Self> {
            Ok(PlaybackRequestTransmitterStdin { _phantom: None })
        }
    }

    impl<T: DeserializeOwned + PartialEq + Clone> PlaybackRequestTransmitterBackend<T>
        for PlaybackRequestTransmitterStdin<T>
    {
        fn run(&mut self, tx: Sender<Option<T>>) -> Fallible<()> {
            let mut last: Option<T> = None;

            let stdin = std::io::stdin();
            for line in stdin.lock().lines() {
                if let Ok(ref line) = line {
                    let req = if line == "" {
                        None
                    } else {
                        Some(serde_json::from_str(line).unwrap())
                    };
                    if last != req {
                        tx.send(req.clone()).unwrap();
                    }
                    last = req;
                }
            }

            panic!();
        }
    }
}

pub mod rfid {
    use super::*;
    use crate::rfid::*;

    // use rfid_rs::{picc, MFRC522};

    pub struct PlaybackRequestTransmitterRfid<T> {
        picc: RfidController,
        _phantom: Option<T>,
    }

    impl<T: DeserializeOwned + std::fmt::Debug> PlaybackRequestTransmitterRfid<T> {
        pub fn new() -> Fallible<Self> {
            let picc = RfidController::new()?;

            Ok(PlaybackRequestTransmitterRfid {
                picc,
                _phantom: None,
            })
        }
    }

    fn handle_tag<T: DeserializeOwned + 'static + PartialEq + Clone + Send + Sync>(
        tag: Tag,
        tx: &Sender<Option<T>>,
    ) -> Fallible<()> {
        let mut tag_reader = tag.new_reader();
        let request_string = tag_reader.read_string()?;
        let request_deserialized: T = serde_json::from_str(&request_string)?;
        Ok(tx.send(Some(request_deserialized.clone()))?)
    }

    impl<T: DeserializeOwned + Send + Sync + 'static + PartialEq + Clone>
        PlaybackRequestTransmitterBackend<T> for PlaybackRequestTransmitterRfid<T>
    {
        fn run(&mut self, tx: Sender<Option<T>>) -> Fallible<()> {
            let mut last_uid: Option<String> = None;

            loop {
                match self.picc.open_tag() {
                    Err(err) => {
                        // Do not change playback state in this case.
                        warn!("Failed to open RFID tag: {}", err);
                    }
                    Ok(None) => {
                        if last_uid.is_some() {
                            info!("RFID Tag gone");
                            last_uid = None;
                            if let Err(err) = tx.send(None) {
                                error!("Failed to transmit User Request: {}", err);
                            }
                        }
                    }
                    Ok(Some(tag)) => {
                        let current_uid = format!("{:?}", tag.uid);
                        if last_uid == Some(current_uid.clone()) {
                            std::thread::sleep(std::time::Duration::from_millis(1500));
                            continue;
                        }
                        // new tag!
                        match handle_tag(tag, &tx) {
                            Ok(_) => {
                                last_uid = Some(current_uid);
                            }
                            Err(err) => {
                                error!("Failed to handle tag: {}", err);
                            }
                        }
                    }
                }
                std::thread::sleep(std::time::Duration::from_millis(1500));
            }
        }
    }
}

impl<T: DeserializeOwned + Clone + PartialEq + Sync + Send + 'static> PlaybackRequests<T> {
    pub fn new<TX>(mut transmitter: PlaybackRequestsTransmitter<T, TX>) -> Self
    where
        TX: Send + 'static + PlaybackRequestTransmitterBackend<T>,
        T: DeserializeOwned + std::fmt::Debug,
    {
        let (tx, rx): (Sender<Option<T>>, Receiver<Option<T>>) = mpsc::channel();
        std::thread::spawn(move || transmitter.run(tx));
        Self { rx }
    }
}

impl<T: Sync + Send + 'static> Iterator for PlaybackRequests<T> {
    type Item = Option<T>;

    fn next(&mut self) -> Option<Self::Item> {
        match self.rx.recv() {
            Ok(next_item) => Some(next_item),
            Err(err) => {
                error!(
                    "Failed to receive next user request from PlaybackRequestsTransmitter: {}",
                    err
                );
                None
            }
        }
    }
}
