pub mod button;
pub mod playback;

use std::sync::{Arc, RwLock};

use failure::Fallible;
use futures::future::{abortable, AbortHandle, Abortable};
use tokio::{
    sync::broadcast::{channel, Receiver, Sender},
    task,
};

use crate::player::PlaybackRequest;

#[derive(Clone, Debug)]
pub enum Input {
    Button(button::Command),
    Playback(PlaybackRequest),
}

pub struct InputSourceFactory {
    // rx: Receiver<Input>,
    // tx: Sender<Input>,
    buttons: Option<Box<dyn Sync + Send + Fn() -> button::Handle<button::Command>>>, // This spawn a separate thread implementing the blocking event retrieval.
    button_controller: Arc<RwLock<Option<button::Handle<button::Command>>>>,
}

pub struct InputSource {
    buttons_transmitter: Option<AbortHandle>,
    rx: Receiver<Input>,
}

impl Drop for InputSource {
    fn drop(&mut self) {
        eprintln!("Dropping InputSource");

        if let Some(buttons_transmitter) = &self.buttons_transmitter {
            eprintln!("Aborting button input controller");
            buttons_transmitter.abort();
        }
    }
}

impl InputSourceFactory {
    pub fn consume(&self) -> Fallible<InputSource> {
        let (tx, rx) = channel(2);

        let opt_buttons_handle =
            if let Some(ref button_controller) = *(self.button_controller.read().unwrap()) {
                // button controller exists already. reuse it.
                Some(button_controller.clone())
            } else if let Some(mk_buttons) = &self.buttons {
                // have a closure for creating a button controller, execute it.
                let button_controller = mk_buttons(); // spawns thread.
                {
                    let mut writer = self.button_controller.write().unwrap();
                    *writer = Some(button_controller.clone());
                }
                Some(button_controller)
            } else {
                // no button controller configured
                None
            };

        let buttons_transmitter = if let Some(buttons_handle) = opt_buttons_handle {
            // spawn button controller transmitter.
            let mut receiver = buttons_handle.receiver();
            let (f, abortable_handle) = futures::future::abortable(async move {
                loop {
                    let el = Input::Button(receiver.recv().await.unwrap());
                    tx.send(el);
                }
            });
            tokio::spawn(f);
            Some(abortable_handle)
        } else {
            None
        };

        let input_source = InputSource {
            rx,
            buttons_transmitter,
        };
        Ok(input_source)
    }

    pub fn new() -> Fallible<Self> {
        let input_source = InputSourceFactory {
            buttons: None,
            button_controller: Arc::new(RwLock::new(None)),
        };
        Ok(input_source)
    }

    pub fn with_buttons<I: 'static>(mut self, input_controller: I) -> Self
    where
        I: Fn() -> button::Handle<button::Command> + Send + Sync,
    {
        self.buttons = Some(Box::new(input_controller));
        self
    }
}
