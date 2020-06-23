pub mod button;
pub mod playback;

use std::sync::{Arc, RwLock};

use failure::Fallible;
use futures::future::AbortHandle;
use slog_scope::{error, info};
use tokio::sync::broadcast::{channel, Receiver, Sender};

use crate::player::PlaybackRequest;

#[derive(Clone, Debug)]
pub enum Input {
    Button(button::Command),
    Playback(PlaybackRequest),
}

pub trait InputSourceFactory {
    fn consume(&self) -> Fallible<Box<dyn InputSource + Sync + Send + 'static>>;
}

pub mod test {
    use super::*;

    impl InputSourceFactory for Vec<Input> {
        fn consume(&self) -> Fallible<Box<dyn InputSource + Sync + Send + 'static>> {
            Ok(Box::new(self.clone()))
        }
    }

    impl InputSource for Vec<Input> {
        fn receiver(&self) -> Receiver<Input> {
            let (tx, rx) = channel(self.len());
            for i in self.iter() {
                tx.send(i.clone()).unwrap();
            }
            rx
        }
    }
}

pub trait InputSource {
    fn receiver(&self) -> Receiver<Input>;
}

pub struct ProdInputSourceFactory {
    buttons: Option<Box<dyn Sync + Send + Fn() -> Fallible<button::Handle>>>, // This spawn a separate thread implementing the blocking event retrieval.
    playback: Option<Box<dyn Sync + Send + Fn() -> Fallible<playback::Handle>>>,
    button_controller: Arc<RwLock<Option<button::Handle>>>,
}

pub struct ProdInputSource {
    buttons_transmitter: Option<AbortHandle>,
    playback_transmitter: Option<AbortHandle>,
    sender: Sender<Input>,
}

impl Drop for ProdInputSource {
    fn drop(&mut self) {
        if let Some(buttons_transmitter) = &self.buttons_transmitter {
            info!("Aborting button input controller");
            buttons_transmitter.abort();
        }

        if let Some(playback_transmitter) = &self.playback_transmitter {
            info!("Aborting playback input controller");
            playback_transmitter.abort();
        }
    }
}

impl InputSource for ProdInputSource {
    fn receiver(&self) -> Receiver<Input> {
        self.sender.subscribe()
    }
}

impl InputSourceFactory for ProdInputSourceFactory {
    fn consume(&self) -> Fallible<Box<dyn InputSource + Sync + Send + 'static>> {
        info!("Setting up Prod Input Source from Factor");
        let (tx, _rx) = channel(2);

        let opt_buttons_handle = {
            let reader = self.button_controller.read().unwrap();
            let opt_button_controller = (*reader).clone();
            drop(reader);
            if let Some(button_controller) = opt_button_controller {
                // button controller exists already. reuse it.
                Some(button_controller)
            } else if let Some(mk_buttons) = &self.buttons {
                // have a closure for creating a button controller, execute it.
                let button_controller = mk_buttons()?; // spawns thread.
                {
                    info!("about to acquire writer");
                    let mut writer = self.button_controller.write().unwrap();
                    info!("acquired writer");
                    *writer = Some(button_controller.clone());
                }
                Some(button_controller)
            } else {
                // no button controller configured
                None
            }
        };

        info!("Preparing button transmitter");

        let buttons_transmitter = if let Some(buttons_handle) = opt_buttons_handle {
            // spawn button controller transmitter.
            let mut receiver = buttons_handle.receiver();
            let tx = tx.clone();
            let (f, abortable_handle) = futures::future::abortable(async move {
                loop {
                    let el = Input::Button(receiver.recv().await.unwrap());
                    if let Err(err) = tx.send(el.clone()) {
                        error!(
                            "Failed to transmit button event {:?} in InputSource: {:?}",
                            &el, err
                        );
                    }
                }
            });
            tokio::spawn(f);
            Some(abortable_handle)
        } else {
            None
        };

        info!("About to setup playback input");

        let playback_transmitter = if let Some(mk_playback) = &self.playback {
            info!("Creating Playback Controller...");
            let mut playback_controller = mk_playback()?;
            let tx = tx.clone();
            let (f, abortable) = futures::future::abortable(async move {
                loop {
                    let el = Input::Playback(playback_controller.recv().await.unwrap());
                    if let Err(err) = tx.send(el.clone()) {
                        error!(
                            "Failed to transmit playback event {:?} in InputSource: {:?}",
                            &el, err
                        );
                    }
                }
            });
            info!("Spawning Playback Controller Task");
            tokio::spawn(f);
            Some(abortable)
        } else {
            None
        };

        info!("Creating Production Input Source");

        let input_source = ProdInputSource {
            sender: tx,
            buttons_transmitter,
            playback_transmitter,
        };
        Ok(Box::new(input_source))
    }
}

impl ProdInputSourceFactory {
    pub fn new() -> Fallible<Self> {
        let input_source = ProdInputSourceFactory {
            buttons: None,
            playback: None,
            button_controller: Arc::new(RwLock::new(None)),
        };
        Ok(input_source)
    }
    pub fn with_buttons(
        &mut self,
        input_controller: Box<dyn Fn() -> Fallible<button::Handle> + Send + Sync + 'static>,
    ) {
        self.buttons = Some(input_controller);
    }
    pub fn with_playback(
        &mut self,
        input_controller: Box<dyn Fn() -> Fallible<playback::Handle> + Send + Sync + 'static>,
    ) {
        self.playback = Some(input_controller);
    }
}

pub mod mock {
    use super::{button, playback, InputSource, InputSourceFactory};
    use failure::Fallible;

    use super::Input;
    use tokio::sync::broadcast::{channel, Receiver, Sender};

    pub struct MockInputSourceFactory;
    pub struct MockInputSource {
        sender: Sender<Input>,
    }

    impl InputSourceFactory for MockInputSourceFactory {
        fn consume(&self) -> Fallible<Box<dyn InputSource + Sync + Send + 'static>> {
            let (tx, _rx) = channel(2);
            Ok(Box::new(MockInputSource { sender: tx }))
        }
    }

    impl MockInputSourceFactory {
        pub fn new() -> Fallible<MockInputSourceFactory> {
            Ok(MockInputSourceFactory)
        }
        pub fn with_buttons(
            &mut self,
            _input_controller: Box<dyn Fn() -> Fallible<button::Handle> + Send + Sync + 'static>,
        ) {
            unimplemented!()
        }
        pub fn with_playback(
            &mut self,
            _input_controller: Box<dyn Fn() -> Fallible<playback::Handle> + Send + Sync + 'static>,
        ) {
            unimplemented!()
        }
    }

    impl InputSource for MockInputSource {
        fn receiver(&self) -> Receiver<Input> {
            self.sender.subscribe()
        }
    }
}
