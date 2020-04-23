use slog_scope::info;
use std::thread;

use gotham;
use gotham::state::State;

const SERVER_PORT: u32 = 8080;
const SERVER_HOST: &'static str = "0.0.0.0";

const HELLO_WORLD: &'static str = "Hello World!";

/// Create a `Handler` which is invoked when responding to a `Request`.
///
/// How does a function become a `Handler`?.
/// We've simply implemented the `Handler` trait, for functions that match the signature used here,
/// within Gotham itself.
pub fn say_hello(state: State) -> (State, &'static str) {
    (state, HELLO_WORLD)
}

pub struct Server;

impl Server {
    pub fn start() {
        let addr = format!("{}:{}", SERVER_HOST, SERVER_PORT);
        info!("Spawning Web Server");
        let _handle = thread::Builder::new()
            .name("webserver".to_string())
            .spawn(move || gotham::start(addr, || Ok(say_hello)))
            .unwrap();
    }
}
