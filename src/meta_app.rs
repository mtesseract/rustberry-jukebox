use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use async_std::sync::RwLock;

use failure::Fallible;
use slog_scope::info;

use crate::config::Config;
use crate::effects::{DynInterpreter, DynInterpreterFactory, Interpreter, InterpreterFactory};
use crate::input_controller::{DynInputSourceFactory, InputSourceFactory};
use crate::player::{PlaybackRequest, PlaybackResource};
use futures::future::AbortHandle;

use crate::app_jukebox::App;
use crate::components::rfid::RfidController;

use std::convert::Infallible;
use warp::http::StatusCode;
use warp::Filter;

pub struct MetaApp {
    control_rx: tokio::sync::mpsc::Receiver<AppControl>,
    control_tx: tokio::sync::mpsc::Sender<AppControl>,
    initialized: Arc<RwLock<bool>>,
    config: Config,
    input_source_factory: DynInputSourceFactory,
    interpreter_factory: DynInterpreterFactory,
}

#[derive(Clone)]
pub struct MetaAppHandle {
    control_tx: tokio::sync::mpsc::Sender<AppControl>,
    initialized: Arc<RwLock<bool>>,
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

    pub async fn is_ready(&self) -> bool {
        loop {
            let ready = {
                let r = self.initialized.read().await;
                *r
            };
            if ready {
                return ready;
            } else {
                tokio::time::delay_for(Duration::from_millis(50)).await;
            }
        }
    }
}

impl MetaApp {
    pub fn handle(&self) -> MetaAppHandle {
        let control_tx = self.control_tx.clone();
        let initialized = self.initialized.clone();
        let meta_app_handle = MetaAppHandle {
            control_tx,
            initialized,
        };
        meta_app_handle
    }

    pub async fn new(
        config: Config,
        interpreter_factory: DynInterpreterFactory,
        input_source_factory: Box<dyn InputSourceFactory + Sync + Send + 'static>,
    ) -> Fallible<Self> {
        let (control_tx, control_rx) = tokio::sync::mpsc::channel(128);
        let meta_app = MetaApp {
            input_source_factory,
            interpreter_factory,
            config,
            control_rx,
            control_tx,
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

    async fn set_current_mode(
        meta_app_handle: MetaAppHandle,
        mode: AppMode,
    ) -> Result<impl warp::Reply, Infallible> {
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
        _meta_app_handle: MetaAppHandle,
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

    async fn get_rfid_tag(_meta_app_handle: MetaAppHandle) -> Result<impl warp::Reply, Infallible> {
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

    pub async fn run(mut self, initial_mode: Option<AppMode>) -> Fallible<()> {
        let mut initial_mode = initial_mode;
        let routes = {
            let meta_app_handle = self.handle();
            let ep_current_mode = {
                let meta_app_handle = meta_app_handle.clone();
                warp::path!("current-mode")
                    .and(
                        (warp::get().and(Self::with_meta_app_handle(meta_app_handle.clone())).and_then(Self::get_current_mode))
                        .or(warp::put().and(Self::with_meta_app_handle(meta_app_handle.clone())).and(warp::body::json::<AppMode>()).and_then(Self::set_current_mode)))
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
            (warp::get().and(ep_current_mode))
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
            let cmd = {
                if let Some(mode) = initial_mode {
                    initial_mode = None;
                    AppControl::SetMode(mode)
                } else {
                    info!("Waiting for new Meta App Control Command");
                    self.control_rx.recv().await.unwrap()
                }
            };
            info!("MetaApp Ctrl Cmd: {:?}", &cmd);
            match cmd {
                AppControl::RequestCurrentMode(os_tx) => {
                    // Only fails if the Receiver has hung up already.
                    // In that case, what else should we do than simply ignoring
                    // this request?
                    let _ = os_tx.send(current_mode.clone());
                }

                AppControl::SetMode(mode) => {
                    abortable.map(|x: AbortHandle| {
                        info!("Shutting down mode {:?}", current_mode);
                        x.abort();
                    });
                    info!("Starting {:?} mode", mode);
                    let abortable_handle = match mode {
                        AppMode::Starting => None,
                        AppMode::Jukebox => {
                            let config = self.config.clone();
                            let app = App::new(
                                config,
                                &self.interpreter_factory,
                                &self.input_source_factory,
                            )
                            .await
                            .unwrap();
                            let (f, abortable_handle) =
                                futures::future::abortable(async move { app.run().await });
                            tokio::spawn(f);
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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
