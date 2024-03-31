use anyhow::{Context, Result};
use crossbeam_channel::{self, Receiver, Sender};
use slog_scope::{error, info, trace, warn};
use std::{thread, time::Duration};

use crate::player::PlaybackRequest;

pub struct Handle<T> {
    channel: Receiver<T>,
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

    impl<T: 'static + Send + Sync + Clone + std::fmt::Debug> PlaybackRequestTransmitterRfid<T>
        where T: From<PlaybackRequest> {
        pub fn new() -> Result<Handle<T>> {
            let (tx, rx) = crossbeam_channel::bounded(10);
            let picc = RfidController::new().context("Creating RfidController")?;
            let transmitter = Self { picc, tx };
            thread::Builder::new()
                .name("playback-transmitter".to_string())
                .spawn(move || {
                    info!("Running PlaybackTransmitter");
                    transmitter.run().unwrap()
                })
                .context("Spawning PlaybackRequestTransmitterRfid")?;
            Ok(Handle { channel: rx })
        }

        fn run(mut self) -> Result<()> {
            let mut last_uid: Option<Uid> = None;
            info!("PlaybackRequestTransmitterRfid loop running");

            loop {
                match self.picc.open_tag() {
                    Err(err) => {
                        // Do not change playback state in this case.
                        warn!("Failed to open RFID tag: {}", err);
                        thread::sleep(Duration::from_millis(80));
                    }
                    Ok(None) => {
                        trace!("No PICC seen.");
                        if last_uid.is_some() {
                            info!("PICC gone");
                            last_uid = None;
                            if let Err(err) = self.tx.send(PlaybackRequest::Stop.into()) {
                                error!("Failed to transmit User Request: {}", err);
                            }
                            thread::sleep(Duration::from_millis(80));
                        }
                    }
                    Ok(Some(tag)) => {
                        trace!("Seen PICC {:?}", tag);

                        let tagclone = tag.clone();

                        let current_uid = tag.uid;
                        if last_uid != Some(current_uid.clone()) {
                            info!("Seen new PICC {}", current_uid);
                            if let Err(err) = self.tx.send(PlaybackRequest::Start(tagclone).into()) {
                                error!("Failed to handle tag: {}", err);
                                thread::sleep(Duration::from_millis(80));
                                continue;
                            }
                            last_uid = Some(current_uid);
                        }
                        thread::sleep(Duration::from_millis(80));
                    }
                }
            }
        }
    }
}
