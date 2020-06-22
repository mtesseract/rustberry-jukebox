use failure::Fallible;
use reqwest;
use rodio::Sink;
use slog_scope::{info, warn};
use std::convert::From;
use std::env;
use std::fmt::{self, Display};
use std::io::BufReader;
use std::sync::Arc;

use async_trait::async_trait;
use tokio::task::spawn_blocking;

pub use err::*;

use crate::components::finite_stream::FiniteStream;
use crate::player::{PauseState, PlaybackHandle};

pub struct HttpPlayer {
    basic_auth: Option<(String, String)>,
    http_client: Arc<reqwest::Client>,
}

pub struct HttpPlaybackHandle {
    // tx: Sender<()>,
    sink: Arc<Sink>,
    basic_auth: Option<(String, String)>,
    url: String,
    http_client: Arc<reqwest::Client>,
}

impl HttpPlaybackHandle {
    pub async fn queue(&self) -> Fallible<()> {
        let mut builder = self.http_client.get(&self.url);
        if let Some((ref username, ref password)) = &self.basic_auth {
            builder = builder.basic_auth(username, Some(password));
        }
        let response = builder.send().await.unwrap();
        let stream = spawn_blocking(move || FiniteStream::from_response(response).unwrap()).await?;
        let source =
            spawn_blocking(move || rodio::Decoder::new(BufReader::new(stream)).unwrap()).await?;
        self.sink.append(source);

        Ok(())
    }
}

#[async_trait]
impl PlaybackHandle for HttpPlaybackHandle {
    async fn stop(&self) -> Fallible<()> {
        // info!("Cancelling HTTP Player");
        // self.tx.send(()).unwrap();
        self.sink.stop();
        Ok(())
    }
    async fn is_complete(&self) -> Fallible<bool> {
        Ok(self.sink.empty())
    }

    async fn pause(&self) -> Fallible<()> {
        self.sink.pause();
        Ok(())
    }
    async fn cont(&self, _pause_state: PauseState) -> Fallible<()> {
        self.sink.play();
        Ok(())
    }

    async fn replay(&self) -> Fallible<()> {
        self.sink.stop();
        self.queue().await?;
        self.sink.play();
        Ok(())
    }
}

impl HttpPlayer {
    pub fn new() -> Fallible<Self> {
        info!("Creating new HttpPlayer...");
        // let (tx, rx) = crossbeam_channel::bounded(1);
        let http_client = Arc::new(reqwest::Client::new());
        let basic_auth = {
            let username: Option<String> = env::var("HTTP_PLAYER_USERNAME")
                .map(|x| Some(x))
                .unwrap_or(None);
            let password: Option<String> = env::var("HTTP_PLAYER_PASSWORD")
                .map(|x| Some(x))
                .unwrap_or(None);
            if let (Some(username), Some(password)) = (username, password) {
                Some((username, password))
            } else {
                None
            }
        };
        let player = HttpPlayer {
            basic_auth,
            http_client,
        };

        Ok(player)
    }

    pub async fn start_playback(
        &self,
        url: &str,
        pause_state: Option<PauseState>,
    ) -> Result<HttpPlaybackHandle, failure::Error> {
        if let Some(pause_state) = pause_state {
            warn!("Ignoring pause state: {:?}", pause_state);
        }
        let device = rodio::default_output_device().unwrap();
        let url = url.clone().to_string();
        let http_client = self.http_client.clone();
        let basic_auth = self.basic_auth.clone();
        let sink = Arc::new(Sink::new(&device));
        // let _handle = Builder::new()
        //     .name("http-player".to_string())
        //     .spawn(move || {
        //         let mut rt = Runtime::new().unwrap();
        //         let f = async {
        //             let _msg = rx.recv();
        //         };
        //         rt.block_on(f);
        //     })
        //     .unwrap();

        let handle = HttpPlaybackHandle {
            // tx,
            sink,
            basic_auth,
            url,
            http_client,
        };
        handle.queue().await?;
        handle
            .cont(PauseState {
                pos: std::time::Duration::from_secs(0),
            })
            .await?;
        Ok(handle)
    }
}

pub mod err {
    use super::*;

    #[derive(Debug)]
    pub enum Error {
        IO(std::io::Error),
        Http(reqwest::Error),
    }

    impl Display for Error {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            match self {
                Error::IO(err) => write!(f, "HTTP Player IO Error {}", err),
                Error::Http(err) => write!(f, "HTTP Player HTTP Error {}", err),
            }
        }
    }

    impl From<std::io::Error> for Error {
        fn from(err: std::io::Error) -> Self {
            Error::IO(err)
        }
    }

    impl From<reqwest::Error> for Error {
        fn from(err: reqwest::Error) -> Self {
            Error::Http(err)
        }
    }

    impl std::error::Error for Error {}
}
