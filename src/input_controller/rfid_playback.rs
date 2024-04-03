use anyhow::{Context, Result};
use crossbeam_channel::{self, Receiver, Sender};
use std::{thread, time::Duration};
use tracing::{error, info, trace, warn};

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
    where
        T: From<PlaybackRequest>,
    {
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
            let mut last_playing_opt: Option<Uid> = None;
            let mut last_uid_opt: Option<Uid> = None;
            let mut deflicker: u32 = 0;
            let deflicker_threshold: u32 = 3;

            trace!("PlaybackRequestTransmitterRfid loop running");
            loop {
                trace!("loop()");
                thread::sleep(Duration::from_millis(200));
                trace!("about to read_picc_uid()");
                match self.picc.read_picc_uid() {
                    Err(err) => {
                        // Do not change playback state in this case.
                        warn!("Failed to open RFID tag: {}", err);
                    }
                    Ok(None) => {
                        trace!("No PICC found.");
                        if last_uid_opt.is_some() {
                            // Switch from PICC present to no PICC.
                            info!("PICC not present anymore.");
                            last_uid_opt = None;
                            deflicker = 0;
                        } else {
                            // Another iteration without PICC.
                            // Same PICC UID. Apply deflicker logic.
                            if deflicker == deflicker_threshold {
                                continue;
                            }
                            deflicker += 1;
                            if deflicker < deflicker_threshold {
                                continue;
                            }
                            // Deflicker threshold reached, propagate stop message, if necessary.
                            if last_playing_opt.is_none() {
                                continue;
                            }
                            if let Err(err) = self.tx.send(PlaybackRequest::Stop.into()) {
                                error!("Failed to transmit playback stop request: {}", err);
                            }
                            last_playing_opt = None;
                        }
                    }
                    Ok(Some(current_tag)) => {
                        trace!("Detected PICC {:?}.", current_tag);
                        let current_uid = current_tag.uid.clone();

                        if let Some(ref last_uid) = last_uid_opt {
                            // A PICC has been detected previously.
                            if current_uid != *last_uid {
                                // Different UID, reset deflicker counter.
                                deflicker = 0;
                                last_uid_opt = Some(current_uid);
                                continue;
                            }

                            // Same PICC UID. Apply deflicker logic.
                            if deflicker == deflicker_threshold {
                                continue;
                            }
                            deflicker += 1;
                            if deflicker < deflicker_threshold {
                                continue;
                            }
                            // Stable event, process it.
                            // Might trigger Stop and Start playback requests, depending on the current playing state.
                            match last_playing_opt {
                                None => {
                                    if let Err(err) =
                                        self.tx.send(PlaybackRequest::Start(current_tag).into())
                                    {
                                        error!(
                                            "Failed to send playback start event for PICC {}: {}",
                                            current_uid, err
                                        );
                                        continue;
                                    }
                                    last_playing_opt = Some(current_uid);
                                }
                                Some(ref last_playing_uid) if *last_playing_uid == current_uid => {
                                    // This PICC is already playing, nothing to do here.
                                }
                                Some(_) => {
                                    // Different PICC is currently playing.
                                    if let Err(err) = self.tx.send(PlaybackRequest::Stop.into()) {
                                        error!("Failed to send playback stop event: {}", err);
                                        continue;
                                    }
                                    last_playing_opt = None;
                                    if let Err(err) =
                                        self.tx.send(PlaybackRequest::Start(current_tag).into())
                                    {
                                        error!(
                                            "Failed to send playback start event for PICC {}: {}",
                                            current_uid, err
                                        );
                                        continue;
                                    }
                                    last_playing_opt = Some(current_uid);
                                }
                            }
                        } else {
                            // PICC detected after a phase of no PICCs.
                            info!("New PICC: {}.", current_uid);
                            deflicker = 0;
                            last_uid_opt = Some(current_uid);
                        }
                    }
                }
            }
        }
    }
}
