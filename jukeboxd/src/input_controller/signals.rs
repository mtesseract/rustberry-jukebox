// use crossbeam_channel::{self, Sender};
// use failure::Fallible;
// use signal_hook;
// use slog_scope::{error, info, warn};
// use std::thread;
// use std::time::Duration;

// use signal_hook::iterator::Signals;

// use super::Input;
// use crate::effects::Effects;

// pub struct SignalController {
//     effects_tx: Sender<Effects>,
//     signals: Signals,
// }

// impl SignalController {
//     pub fn new<F>(effects_tx: Sender<Effects>, transformer: F) -> Result<(), std::io::Error>
//     where
//         F: Fn(Command) -> Option<T> + 'static + Send + Sync,
//     {
//         let signal_ids = vec![
//             signal_hook::SIGHUP,
//             signal_hook::SIGTERM,
//             signal_hook::SIGINT,
//             signal_hook::SIGQUIT,
//         ];
//         let signals = Signals::new(&signal_ids)?;

//         let controller = Self {
//             effects_tx,
//             signals,
//             // signal_ids,
//         };

//         let _handle = thread::Builder::new()
//             .name("signal-controller".to_string())
//             .spawn(move || controller.main())?;

//         Ok(())
//     }

//     fn main(self) {
//         for signal in self.signals.forever() {
//             if let Err(err) = self.effects_tx.send(Effects::Signal(signal)) {
//                 error!(
//                     "Failed to transmit signal {} to effects channel: {}",
//                     signal, err
//                 );
//             }
//         }
//     }
// }
