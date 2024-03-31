use std::cell::RefCell;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;
use tokio::runtime;

use crate::effects::Interpreter;
use anyhow::Result;
use futures::future::AbortHandle;
use tracing::info;

#[derive(Clone)]
pub struct Blinker {
    interpreter: Arc<Box<dyn Send + Sync + 'static + Interpreter>>,
    abort_handle: RefCell<Option<AbortHandle>>,
    rt: runtime::Handle,
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
    pub fn new(
        rt: runtime::Handle,
        interpreter: Arc<Box<dyn Send + Sync + 'static + Interpreter>>,
    ) -> Result<Self> {
        let abort_handle = RefCell::new(None);
        let blinker = Self {
            interpreter,
            abort_handle,
            rt,
        };
        Ok(blinker)
    }

    fn run(
        interpreter: Arc<Box<dyn Send + Sync + 'static + Interpreter>>,
        cmd: Cmd,
    ) -> Pin<Box<dyn Future<Output = ()> + Send>> {
        Box::pin(async move {
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
        let mut opt_abort_handle = self.abort_handle.borrow_mut();
        if let Some(ref abort_handle) = *opt_abort_handle {
            info!("Terminating current blinking task");
            abort_handle.abort();
            *opt_abort_handle = None;
        }
    }

    pub fn run_async(&self, spec: Cmd) {
        info!("Blinker run_async()");
        if let Some(ref abort_handle) = *(self.abort_handle.borrow()) {
            info!("Terminating current blinking task");
            abort_handle.abort();
        }
        let interpreter = self.interpreter.clone();
        // let spec = spec.clone();
        let (f, handle) =
            futures::future::abortable(async move { Self::run(interpreter, spec).await });
        let _join_handle = self.rt.spawn(f);
        info!("Created new blinking task");
        *(self.abort_handle.borrow_mut()) = Some(handle);
    }
}
