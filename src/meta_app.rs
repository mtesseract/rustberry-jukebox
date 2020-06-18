use std::sync::Arc;
use std::time::Duration;

use async_std::sync::RwLock;

use tokio::stream::StreamExt;
use tokio::sync::mpsc::{channel, Receiver};
use tokio::sync::oneshot;

use failure::Fallible;
use slog::{self, o};
use slog_async;
use slog_scope::{error, info, warn};
use slog_term;

use crate::config::Config;
use crate::effects::{test::TestInterpreter, DynInterpreter, Interpreter, ProdInterpreter};
use crate::input_controller::{
    button, mock, playback, Input, InputSource, InputSourceFactory, ProdInputSource,
    ProdInputSourceFactory,
};
use crate::player::{PlaybackRequest, PlaybackResource};
use futures::future::AbortHandle;

use crate::led::{self, Blinker};

use crate::app_jukebox::App;
use crate::components::rfid::RfidController;

use std::convert::Infallible;
use warp::http::StatusCode;
use warp::Filter;

pub struct MetaApp {
    control_rx: tokio::sync::mpsc::Receiver<AppControl>,
    control_tx: tokio::sync::mpsc::Sender<AppControl>,
    jukebox_app: App,
    initialized: Arc<RwLock<bool>>,
}

#[derive(Clone)]
pub struct MetaAppHandle {
    control_tx: tokio::sync::mpsc::Sender<AppControl>,
}

impl MetaAppHandle {
    pub async fn current_mode(&self) -> AppMode {
        let (os_tx, os_rx) = tokio::sync::oneshot::channel();
        let mut control_tx = self.control_tx.clone();
        control_tx
            .try_send(AppControl::RequestCurrentMode(os_tx))
            .unwrap(); // FIXME
        os_rx.await.unwrap()
    }

    pub async fn set_mode(&self, mode: AppMode) -> Fallible<()> {
        let mut control_tx = self.control_tx.clone();
        control_tx.try_send(AppControl::SetMode(mode))?;
        Ok(())
    }
}

impl MetaApp {
    pub async fn is_ready(&self) -> bool {
        loop {
            let ready = {
                let r = self.initialized.read().await;
                *r
            };
            tokio::time::delay_for(std::time::Duration::from_millis(50)).await;
        }
    }

    pub fn handle(&self) -> MetaAppHandle {
        let control_tx = self.control_tx.clone();
        let meta_app_handle = MetaAppHandle { control_tx };
        meta_app_handle
    }

    pub async fn new(
        config: Config,
        interpreter: Arc<Box<dyn Interpreter + Sync + Send + 'static>>,
        blinker: Blinker,
        input_factory: Box<dyn InputSourceFactory + Sync + Send + 'static>,
    ) -> Fallible<Self> {
        let (control_tx, control_rx) = tokio::sync::mpsc::channel(1);
        let input_source_factory = Arc::new(input_factory);

        let jukebox_app = App::new(
            config.clone(),
            interpreter.clone(),
            blinker.clone(),
            input_source_factory,
        )
        .unwrap();

        let meta_app = MetaApp {
            control_rx,
            control_tx,
            jukebox_app,
            initialized: Arc::new(RwLock::new(false)),
        };
        Ok(meta_app)
    }

    async fn get_current_mode(
        meta_app_handle: MetaAppHandle,
    ) -> Result<impl warp::Reply, Infallible> {
        info!("get_current_mode()");

        let current_mode = meta_app_handle.current_mode().await;
        let current_mode: String = format!("{:?}", current_mode);

        Ok(warp::reply::json(&current_mode))
    }

    fn with_meta_app_handle(
        handle: MetaAppHandle,
    ) -> impl Filter<Extract = (MetaAppHandle,), Error = std::convert::Infallible> + Clone {
        warp::any().map(move || handle.clone())
    }

    async fn set_mode_admin(
        meta_app_handle: MetaAppHandle,
    ) -> Result<impl warp::Reply, Infallible> {
        info!("set_mode_admin()");
        Self::set_mode(meta_app_handle, AppMode::Admin).await
    }

    async fn set_mode_jukebox(
        meta_app_handle: MetaAppHandle,
    ) -> Result<impl warp::Reply, Infallible> {
        info!("set_mode_jukebox()");
        Self::set_mode(meta_app_handle, AppMode::Jukebox).await
    }

    async fn set_mode(
        meta_app_handle: MetaAppHandle,
        mode: AppMode,
    ) -> Result<impl warp::Reply, Infallible> {
        info!("set_mode_jukebox()");

        let inner = |meta_app_handle: MetaAppHandle| async move {
            Ok(meta_app_handle.set_mode(mode).await?)
        };

        let res: Fallible<()> = inner(meta_app_handle).await;

        match res {
            Ok(()) => Ok(StatusCode::OK),
            Err(_) => Ok(StatusCode::INTERNAL_SERVER_ERROR),
        }
    }

    async fn put_rfid_tag(
        meta_app_handle: MetaAppHandle,
        resource: PlaybackResource,
    ) -> Result<impl warp::Reply, Infallible> {
        let resource_deserialized =
            serde_json::to_string(&resource).expect("Resource Deserialization");
        let mut rc = RfidController::new().unwrap();
        let tag = rc.open_tag().expect("Failed to open RFID tag").unwrap();
        let mut tag_writer = tag.new_writer();
        tag_writer.write_string(&resource_deserialized).unwrap();
        Ok(StatusCode::OK)
    }

    async fn get_rfid_tag(meta_app_handle: MetaAppHandle) -> Result<impl warp::Reply, Infallible> {
        let mut rc = RfidController::new().unwrap();
        let tag = rc.open_tag().unwrap().unwrap();
        println!("{:?}", tag.uid);
        let mut tag_reader = tag.new_reader();
        let s = tag_reader.read_string().expect("read_string");
        let req: PlaybackRequest =
            serde_json::from_str(&s).expect("PlaybackRequest Deserialization");
        dbg!(&req);
        Ok(StatusCode::OK)
    }

    pub async fn run(mut self) -> Fallible<()> {
        let routes = {
            let meta_app_handle = self.handle();
            let hello = warp::path!("hello" / String).map(|name| format!("Hello, {}!", name));
            let ep_mode = {
                let meta_app_handle = meta_app_handle.clone();
                warp::path!("mode")
                    .and(Self::with_meta_app_handle(meta_app_handle))
                    .and_then(Self::get_current_mode)
            };
            let ep_mode_admin = {
                let meta_app_handle = meta_app_handle.clone();
                warp::path!("mode-admin")
                    .and(Self::with_meta_app_handle(meta_app_handle))
                    .and_then(Self::set_mode_admin)
            };
            let eps_admin = {
                warp::path!("rfid-tag").and(
                    (warp::put()
                        .and(Self::with_meta_app_handle(meta_app_handle.clone()))
                        .and(warp::body::json::<PlaybackResource>())
                        .and_then(Self::put_rfid_tag))
                    .or(warp::get().and(
                        Self::with_meta_app_handle(meta_app_handle.clone())
                            .and_then(Self::get_rfid_tag),
                    )),
                )
            };
            let ep_mode_jukebox = {
                let meta_app_handle = meta_app_handle.clone();
                warp::path!("mode-jukebox")
                    .and(Self::with_meta_app_handle(meta_app_handle))
                    .and_then(Self::set_mode_jukebox)
            };
            (warp::get().and(hello.or(ep_mode).or(ep_mode_admin).or(ep_mode_jukebox)))
                .or(warp::path!("admin" / ..).and(eps_admin))
        };

        tokio::spawn(warp::serve(routes).run(([0, 0, 0, 0], 3030)));

        let mut current_mode = AppMode::Starting;
        let mut abortable = None;

        {
            info!("MetaApp is ready");
            let mut w = self.initialized.write().await;
            *w = true;
        }

        loop {
            let cmd = self.control_rx.recv().await.unwrap();
            info!("MetaApp Ctrl Cmd: {:?}", &cmd);
            match cmd {
                AppControl::RequestCurrentMode(os_tx) => {
                    // Only fails if the Receiver has hung up already.
                    // In that case, what else should we do than simply ignoring
                    // this request?
                    let _ = os_tx.send(current_mode.clone());
                }

                AppControl::SetMode(mode) => {
                    info!("Shutting down mode {:?}", current_mode);
                    abortable.map(|x: AbortHandle| x.abort());
                    info!("Starting {:?} mode", mode);
                    let abortable_handle = match mode {
                        AppMode::Starting => None,
                        AppMode::Jukebox => {
                            let abortable_handle = self.jukebox_app.run().await?;
                            // let isf2 = self.input_factory.clone();
                            // let blinker = self.blinker.clone();
                            // let interpreter = self.interpreter.clone();
                            // let config = self.config.clone();
                            // let (f, abortable_handle) = futures::future::abortable(async move {
                            //     let input_source = isf2.consume().unwrap();
                            //     Self::run_jukebox(config, input_source, blinker, interpreter).await
                            // });
                            // tokio::spawn(f);
                            Some(abortable_handle)
                        }
                        AppMode::Admin => None,
                    };
                    current_mode = mode;
                    abortable = abortable_handle;
                }
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AppMode {
    Starting,
    Jukebox,
    Admin,
}

#[derive(Debug)]
pub enum AppControl {
    SetMode(AppMode),
    RequestCurrentMode(tokio::sync::oneshot::Sender<AppMode>),
}
