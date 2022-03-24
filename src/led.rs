use std::cell::RefCell;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, RwLock};
use std::time::Duration;
use tokio::runtime;

use crate::effects::Interpreter;
use failure::Fallible;
use futures::future::AbortHandle;
use slog_scope::info;

struct State {
    is_on: bool,
}

#[derive(Clone)]
pub struct Blinker {
    interpreter: Arc<Box<dyn Send + Sync + 'static + Interpreter>>,
    abort_handle: RefCell<Option<AbortHandle>>,
    rt: runtime::Handle,
    state: Arc<RwLock<State>>,
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
        rt: &runtime::Handle,
        interpreter: Arc<Box<dyn Send + Sync + 'static + Interpreter>>,
    ) -> Fallible<Self> {
        let abort_handle = RefCell::new(None);
        let blinker = Self {
            interpreter,
            abort_handle,
            rt: rt.clone(),
            state: Arc::new(RwLock::new(State { is_on: false })),
        };
        Ok(blinker)
    }

    fn run(
        interpreter: Arc<Box<dyn Send + Sync + 'static + Interpreter>>,
        state: Arc<RwLock<State>>,
        cmd: Cmd,
    ) -> Pin<Box<dyn Future<Output = ()> + Send>> {
        Box::pin(async move {
            match cmd {
                Cmd::On(duration) => {
                    info!("Blinker switches on");
                    let _ = interpreter.led_on();
                    {
                        let mut state = state.write().unwrap();
                        (*state).is_on = true;
                    }
                    // state.is_on = true;
                    tokio::time::delay_for(duration).await;
                }
                Cmd::Off(duration) => {
                    info!("Blinker switches off");
                    let _ = interpreter.led_off();
                    {
                        let mut state = state.write().unwrap();
                        (*state).is_on = false;
                    }
                    tokio::time::delay_for(duration).await;
                }
                Cmd::Many(cmds) => {
                    info!("Blinker processes Many");
                    for cmd in &cmds {
                        Self::run(interpreter.clone(), Arc::clone(&state), cmd.clone()).await;
                    }
                }
                Cmd::Repeat(n, cmd) => {
                    info!("Blinker processes Repeat (n = {})", n);
                    for _i in 0..n {
                        Self::run(interpreter.clone(), Arc::clone(&state), (*cmd).clone()).await;
                    }
                }
                Cmd::Loop(cmd) => loop {
                    Self::run(interpreter.clone(), Arc::clone(&state), (*cmd).clone()).await;
                },
            }
        })
    }

    fn run_and_reset(
        interpreter: Arc<Box<dyn Send + Sync + 'static + Interpreter>>,
        state: Arc<RwLock<State>>,
        cmd: Cmd,
    ) -> Pin<Box<dyn Future<Output = ()> + Send>> {
        Box::pin(async move {
            let is_on = {
                let state = state.read().unwrap();
                state.is_on
            };

            Self::run(interpreter, state, cmd).await;

            let cmd = if is_on {
                Cmd::On(Duration::from_millis(0))
            } else {
                Cmd::Off(Duration::from_millis(0))
            };
            Self::run(interpreter, state, cmd).await;
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
        let state = Arc::clone(&self.state);
        let (f, handle) = futures::future::abortable(async move {
            Self::run_and_reset(interpreter, state, spec).await
        });
        let _join_handle = self.rt.spawn(f);
        info!("Created new blinking task");
        *(self.abort_handle.borrow_mut()) = Some(handle);
    }
}
