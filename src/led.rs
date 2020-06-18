use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, RwLock};
use std::time::Duration;

use crate::effects::Interpreter;
use failure::Fallible;
use futures::future::AbortHandle;
use slog_scope::{info};

#[derive(Clone)]
pub struct Blinker {
    interpreter: Arc<Box<dyn Send + Sync + 'static + Interpreter>>,
    abort_handle: Arc<RwLock<Option<AbortHandle>>>,
}

#[derive(Debug, Clone)]
pub enum Cmd {
    Repeat(u32, Box<Cmd>),
    Loop(Box<Cmd>),
    On(Duration),
    Off(Duration),
    Many(Vec<Cmd>),
}

impl Blinker {
    pub fn new(interpreter: Arc<Box<dyn Send + Sync + 'static + Interpreter>>) -> Fallible<Self> {
        let abort_handle = Arc::new(RwLock::new(None));
        let blinker = Self {
            interpreter,
            abort_handle,
        };
        Ok(blinker)
    }

    fn run(
        interpreter: Arc<Box<dyn Send + Sync + 'static + Interpreter>>,
        cmd: Cmd,
    ) -> Pin<Box<dyn Future<Output = ()> + Send>> {
        Box::pin(async move {
            info!("Inside Blinker::run()");
            match cmd {
                Cmd::On(duration) => {
                    info!("Blinker switches on");
                    let _ = interpreter.led_on();
                    tokio::time::delay_for(duration).await;
                }
                Cmd::Off(duration) => {
                    info!("Blinker switches off");
                    let _ = interpreter.led_off();
                    tokio::time::delay_for(duration).await;
                }
                Cmd::Many(cmds) => {
                    info!("Blinker processes Many");
                    for cmd in &cmds {
                        Self::run(interpreter.clone(), cmd.clone()).await;
                    }
                }
                Cmd::Repeat(n, cmd) => {
                    info!("Blinker processes Repeat (n = {})", n);
                    for _i in 0..n {
                        Self::run(interpreter.clone(), (*cmd).clone()).await;
                    }
                }
                Cmd::Loop(cmd) => loop {
                    Self::run(interpreter.clone(), (*cmd).clone()).await;
                },
            }
        })
    }

    pub fn stop(&self) {
        let mut opt_abort_handle = self.abort_handle.write().unwrap();
        if let Some(ref abort_handle) = *opt_abort_handle {
            info!("Terminating current blinking task");
            abort_handle.abort();
            *opt_abort_handle = None;
        }
    }

    pub async fn run_async(&self, spec: Cmd) {
        info!("Blinker run_async()");
        if let Some(ref abort_handle) = *(self.abort_handle.write().unwrap()) {
            info!("Terminating current blinking task");
            abort_handle.abort();
        }
        let interpreter = self.interpreter.clone();

        let (f, handle) =
            futures::future::abortable(async move { Self::run(interpreter, spec).await });

        info!("run_async: Spawning future");
        let _join_handle = tokio::spawn(f);
        info!("Created new blinking task");
        // let _ = _join_handle.is_ready();

        tokio::time::delay_for(std::time::Duration::from_secs(0)).await; // FIXME: why is this necessary??
        *(self.abort_handle.write().unwrap()) = Some(handle);
    }
}
