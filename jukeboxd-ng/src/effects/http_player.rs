use std::fmt::{self, Display};
use std::thread::{self, JoinHandle};

use hyper::header::AUTHORIZATION;
use reqwest::Client;
use serde::Serialize;
use slog_scope::{error, info, warn};
use std::convert::From;
use std::sync::{Arc, RwLock};

use crossbeam_channel::{Receiver, RecvError, RecvTimeoutError, Select, Sender};

pub use err::*;

pub struct HttpPlayer {
    http_client: Client,
}

pub struct HttpPlayerHandle {}

impl HttpPlayer {
    pub fn new() -> Self {
        let http_client = Client::new();
        HttpPlayer { http_client }
    }

    pub fn start_playback(&self, url: &str, username: &str, password: &str) -> Result<(), Error> {
        unimplemented!()
    }

    pub fn stop_playback(&self) -> Result<(), Error> {
        unimplemented!()
    }
}

pub mod err {
    use super::*;

    #[derive(Debug)]
    pub enum Error {
        HTTP(reqwest::Error),
    }

    impl Display for Error {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            match self {
                Error::HTTP(err) => write!(f, "HTTP Player Error {}", err),
            }
        }
    }

    impl From<reqwest::Error> for Error {
        fn from(err: reqwest::Error) -> Self {
            Error::HTTP(err)
        }
    }

    impl std::error::Error for Error {}
}
