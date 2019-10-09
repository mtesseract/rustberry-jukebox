use failure::Fallible;
#[macro_use]
extern crate rust_embed;
use signal_hook::{iterator::Signals, SIGINT};
use slog::{self, o, Drain};
use slog_async;
use slog_scope::{info, warn};
use slog_term;

mod user_requests {

    use failure::Fallible;
    use serde::de::DeserializeOwned;
    use slog_scope::info;
    use std::io::BufRead;
    use std::sync::mpsc;
    use std::sync::mpsc::{Receiver, Sender};
    pub trait UserRequestTransmitter<T: DeserializeOwned> {
        fn run(&self, tx: Sender<Option<T>>) -> Fallible<()>;
    }

    pub struct UserRequests<T>
    where
        T: Sync + Send + 'static,
    {
        rx: Receiver<Option<T>>,
    }

    pub mod stdin {
        use super::*;
        use std::env;

        pub struct UserRequestTransmitterStdin<T> {
            first_req: Option<T>,
        }

        impl<T: DeserializeOwned + std::fmt::Debug> UserRequestTransmitterStdin<T> {
            pub fn new() -> Self {
                let first_req = env::var("FIRST_REQUEST")
                    .ok()
                    .map(|x| serde_json::from_str(&x).unwrap());
                if let Some(ref first_req) = first_req {
                    info!("Using first request {:?}", first_req);
                }
                UserRequestTransmitterStdin { first_req }
            }
        }

        impl<T: DeserializeOwned + PartialEq + Clone> UserRequestTransmitter<T>
            for UserRequestTransmitterStdin<T>
        {
            fn run(&self, tx: Sender<Option<T>>) -> Fallible<()> {
                let mut last: Option<T> = None;

                if self.first_req.is_some() {
                    tx.send(self.first_req.clone()).unwrap();
                    last = self.first_req.clone();
                }

                let stdin = std::io::stdin();
                for line in stdin.lock().lines() {
                    if let Ok(ref line) = line {
                        let req = if line == "" {
                            None
                        } else {
                            Some(serde_json::from_str(line).unwrap())
                        };
                        if last != req {
                            tx.send(req.clone()).unwrap();
                        }
                        last = req;
                    }
                }

                panic!();
            }
        }
    }

    pub mod tcp {
        use super::*;
        use std::env;
        use std::io::{BufReader, Read};
        use std::net::{TcpListener, TcpStream};

        pub struct UserRequestTransmitterTcp<T> {
            listen_addr: String,
            phantom: Option<T>,
        }

        impl<T: DeserializeOwned + std::fmt::Debug> UserRequestTransmitterTcp<T> {
            pub fn new() -> Self {
                let listen_addr = env::var("USER_REQUESTS_LISTEN_ADDR").unwrap().clone();
                info!("Listening on {} for user requests", listen_addr);
                UserRequestTransmitterTcp {
                    listen_addr,
                    phantom: None,
                }
            }
        }

        impl<T: DeserializeOwned + PartialEq + Clone> UserRequestTransmitter<T>
            for UserRequestTransmitterTcp<T>
        {
            fn run(&self, tx: Sender<Option<T>>) -> Fallible<()> {
                let mut last: Option<T> = None;
                let listener = TcpListener::bind(&self.listen_addr)?;

                // accept connections and process them serially
                for stream in listener.incoming() {
                    let stream = BufReader::new(stream.unwrap());

                    for line in stream.lines() {
                        if let Ok(ref line) = line {
                            let req = if line == "" {
                                None
                            } else {
                                Some(serde_json::from_str(line).unwrap())
                            };
                            if last != req {
                                tx.send(req.clone()).unwrap();
                            }
                            last = req;
                        }
                    }
                }

                panic!();
            }
        }
    }

    impl<T: DeserializeOwned + Clone + PartialEq + Sync + Send + 'static> UserRequests<T> {
        pub fn new<TX>(transmitter: TX) -> Self
        where
            TX: Send + 'static + UserRequestTransmitter<T>,
        {
            let (tx, rx): (Sender<Option<T>>, Receiver<Option<T>>) = mpsc::channel();
            std::thread::spawn(move || transmitter.run(tx));
            Self { rx }
        }
    }

    impl<T: Sync + Send + 'static> Iterator for UserRequests<T> {
        // we will be counting with usize
        type Item = Option<T>;

        // next() is the only required method
        fn next(&mut self) -> Option<Self::Item> {
            Some(self.rx.recv().unwrap())
        }
    }
}

mod spotify_util {
    use failure::{Fail, Fallible};
    use hyper::header::AUTHORIZATION;
    use reqwest::Client;
    use serde::Deserialize;

    use crate::access_token_provider::AccessTokenProvider;

    #[derive(Debug, Clone, Deserialize)]
    pub struct Device {
        pub id: String,
        pub name: String,
        pub is_active: bool,
    }

    #[derive(Debug, Clone, Deserialize)]
    struct DevicesResponse {
        pub devices: Vec<Device>,
    }

    #[derive(Debug, Fail)]
    pub enum JukeboxError {
        #[fail(display = "Device not found: {}", device_name)]
        DeviceNotFound { device_name: String },
    }

    pub fn lookup_device_by_name(
        access_token_provider: &mut AccessTokenProvider,
        device_name: &str,
    ) -> Fallible<Device> {
        let http_client = Client::new();
        let access_token = access_token_provider.get_bearer_token().unwrap();
        let mut rsp = http_client
            .get("https://api.spotify.com/v1/me/player/devices")
            .header(AUTHORIZATION, &access_token)
            .send()?;
        let rsp: DevicesResponse = rsp.json()?;
        let opt_dev = rsp
            .devices
            .into_iter()
            .filter(|x| x.name == device_name)
            .next();
        match opt_dev {
            Some(dev) => Ok(dev),
            None => Err((JukeboxError::DeviceNotFound {
                device_name: device_name.clone().to_string(),
            })
            .into()),
        }
    }
}

mod spotify_play {

    use crate::access_token_provider::AccessTokenProvider;
    use failure::Fallible;
    use hyper::header::AUTHORIZATION;
    use reqwest::Client;
    use serde::Serialize;

    #[derive(Debug, Clone)]
    pub struct Player {
        device_id: String,
        access_token_provider: AccessTokenProvider,
        http_client: Client,
    }

    impl Drop for Player {
        fn drop(&mut self) {
            println!("Destroying Player, stopping music");
            let _ = self.stop_playback();
        }
    }

    #[derive(Debug, Clone, Serialize)]
    struct StartPlayback {
        uris: Vec<String>,
    }

    #[derive(Debug, Clone, Serialize)]
    struct TransferPlayback {
        play: bool,
        device_ids: Vec<String>,
        context_uri: String,
    }

    impl Player {
        pub fn new(access_token_provider: AccessTokenProvider, device_id: &str) -> Self {
            let http_client = Client::new();
            Player {
                device_id: device_id.clone().to_string(),
                access_token_provider,
                http_client,
            }
        }

        pub fn start_playback(&mut self, spotify_uri: &str) -> Fallible<()> {
            let access_token = self.access_token_provider.get_bearer_token().unwrap();
            let req = StartPlayback {
                uris: vec![spotify_uri.clone().to_string()],
            };
            let rsp = self
                .http_client
                .put("https://api.spotify.com/v1/me/player/play")
                .query(&[("device_id", &self.device_id)])
                .header(AUTHORIZATION, &access_token)
                .json(&req)
                .send()?;
            // dbg!(&rsp);
            // let body = rsp.text();
            // dbg!(&body);
            assert!(rsp.status().is_success());

            Ok(())
        }

        pub fn stop_playback(&mut self) -> Fallible<()> {
            let access_token = self.access_token_provider.get_bearer_token().unwrap();
            let rsp = self
                .http_client
                .put("https://api.spotify.com/v1/me/player/pause")
                .query(&[("device_id", &self.device_id)])
                .body("")
                .header(AUTHORIZATION, &access_token)
                .send()?;
            assert!(rsp.status().is_success());
            Ok(())
        }
    }
}

mod access_token_provider {

    use std::sync::{Arc, RwLock};
    use std::thread;

    use failure::Error;
    use gotham_derive::StateData;
    use slog_scope::{info, warn};

    use spotify_auth::request_fresh_token;

    #[derive(Debug, Clone, StateData)]
    pub struct AccessTokenProvider {
        client_id: String,
        client_secret: String,
        refresh_token: String,
        access_token: Arc<RwLock<Result<String, Error>>>,
    }

    impl AccessTokenProvider {
        pub fn get_token(&mut self) -> Option<String> {
            let access_token = self.access_token.read().unwrap();
            (*access_token).as_ref().ok().cloned()
        }
        pub fn get_bearer_token(&mut self) -> Option<String> {
            self.get_token().map(|token| format!("Bearer {}", &token))
        }

        pub fn new(
            client_id: &str,
            client_secret: &str,
            refresh_token: &str,
        ) -> AccessTokenProvider {
            let access_token = Arc::new(RwLock::new(Ok("fixme".to_string())));

            {
                let access_token_clone = Arc::clone(&access_token);
                let client_id = client_id.clone().to_string();
                let client_secret = client_secret.clone().to_string();
                let refresh_token = refresh_token.clone().to_string();

                thread::spawn(move || loop {
                    {
                        let token = request_fresh_token(&client_id, &client_secret, &refresh_token)
                            .map(|x| x.access_token);
                        if let Ok(ref token) = token {
                            info!("Retrieved fresh access token"; "access_token" => token);
                        } else {
                            warn!("Failed to retrieve access token");
                        }
                        let mut access_token_write = access_token_clone.write().unwrap();
                        *access_token_write = token;
                    }
                    thread::sleep(std::time::Duration::from_secs(600));
                });
            }

            AccessTokenProvider {
                client_id: client_id.clone().to_string(),
                client_secret: client_secret.clone().to_string(),
                refresh_token: refresh_token.clone().to_string(),
                access_token,
            }
        }
    }

    pub mod spotify_auth {
        use failure::Fallible;
        const TOKEN_REFRESH_URL: &str = "https://accounts.spotify.com/api/token";
        use base64;
        use reqwest::header::AUTHORIZATION;
        use serde::Deserialize;

        #[derive(Debug, Clone, Deserialize)]
        pub struct AuthResponse {
            pub access_token: String,
            pub token_type: String,
            pub scope: String,
            pub expires_in: i32,
            pub refresh_token: String,
        }

        #[derive(Debug, Clone, Deserialize)]
        pub struct RefreshTokenResponse {
            pub access_token: String,
            pub token_type: String,
            pub scope: String,
            pub expires_in: i32,
        }

        fn encode_client_id_and_secret(client_id: &str, client_secret: &str) -> String {
            let concat = format!("{}:{}", client_id, client_secret);
            let b64 = base64::encode(concat.as_bytes());
            b64
        }

        pub fn request_fresh_token(
            client_id: &str,
            client_secret: &str,
            refresh_token: &str,
        ) -> Fallible<RefreshTokenResponse> {
            let grant_type = "refresh_token";
            let client_id_and_secret = encode_client_id_and_secret(client_id, client_secret);
            let auth_token = format!("Basic {}", client_id_and_secret);
            let params = [("grant_type", grant_type), ("refresh_token", refresh_token)];

            let http_client = reqwest::Client::new();
            let mut res = http_client
                .post(TOKEN_REFRESH_URL)
                .header(AUTHORIZATION, auth_token)
                .form(&params)
                .send()?;
            let rsp_json: RefreshTokenResponse = res.json()?;
            Ok(rsp_json)
        }
    }
}

use serde::Deserialize;

#[derive(Deserialize, Debug)]
struct Config {
    refresh_token: String,
    client_id: String,
    client_secret: String,
    device_name: String,
}

fn run_application() -> Fallible<()> {
    info!("Rustberry/Spotify Starting.");

    let config = envy::from_env::<Config>()?;
    info!("Configuration"; o!("device_name" => &config.device_name));

    let mut access_token_provider = access_token_provider::AccessTokenProvider::new(
        &config.client_id,
        &config.client_secret,
        &config.refresh_token,
    );

    std::thread::sleep(std::time::Duration::from_secs(2));

    let _ = server::spawn(LOCAL_SERVER_PORT, access_token_provider.clone());

    let device = loop {
        match spotify_util::lookup_device_by_name(&mut access_token_provider, &config.device_name) {
            // Err(JukeboxError::DeviceNotFound {..}) => {
            //     println!("Device '{}' not found, waiting", "fixme");
            //     std::thread::sleep(std::time::Duration::from_secs(2));
            // }
            Err(err) => {
                warn!("Failed to lookup device: {}", err.to_string());
                std::thread::sleep(std::time::Duration::from_secs(5));
            }
            Ok(device) => {
                break device;
            }
        }
    };

    info!("Looked up device ID"; o!("device_id" => &device.id));

    let mut player = spotify_play::Player::new(access_token_provider, &device.id);
    info!("Initialized Player");

    {
        let signals = Signals::new(&[SIGINT])?;
        let mut player_clone = player.clone();
        std::thread::spawn(move || {
            let _ = signals.into_iter().next();
            info!("Received signal SIGINT, exiting");
            let _ = player_clone.stop_playback();
            std::process::exit(0);
        });
    }

    // let transmitter = user_requests::stdin::UserRequestTransmitterStdin::new();
    let transmitter = user_requests::tcp::UserRequestTransmitterTcp::new();

    let user_requests_producer: user_requests::UserRequests<String> =
        user_requests::UserRequests::new(transmitter);
    user_requests_producer.for_each(|req| match req {
        Some(req) => {
            info!("Starting playback");
            player.start_playback(&req).unwrap();
        }
        None => {
            info!("Stopping playback");
            player.stop_playback().unwrap();
        }
    });

    Ok(())
}

// mod gpio {
//     use gpio_cdev::{Chip, LineRequestFlags};

//     struct GpioControl;

//     impl GpioControl {
//         pub fn new() -> Self {
//             let mut chip = Chip::new("/dev/gpiochip0")?;
//             GpioControl
//         }
//     }
// }

fn main() -> Fallible<()> {
    let decorator = slog_term::TermDecorator::new().build();
    let drain = slog_term::FullFormat::new(decorator).build().fuse();
    let drain = slog_async::Async::new(drain).build().fuse();
    let logger = slog::Logger::root(drain, o!());
    let _guard = slog_scope::set_global_logger(logger);

    slog_scope::scope(&slog_scope::logger().new(o!()), || run_application())
}

const LOCAL_SERVER_PORT: u32 = 8000;

mod server {

    use crate::access_token_provider::AccessTokenProvider;

    use failure::Fallible;
    use gotham::middleware::state::StateMiddleware;
    use gotham::pipeline::single::single_pipeline;
    use gotham::pipeline::single_middleware;
    use gotham::router::builder::*;
    use gotham::router::Router;
    use gotham::{self, state::State as GothamState};
    use hyper;
    use hyper::{Body, Response};
    use mime;
    use slog_scope::info;

    fn router(access_token_provider: AccessTokenProvider) -> Router {
        // create our state middleware to share the counter
        let middleware = StateMiddleware::new(access_token_provider);

        // create a middleware pipeline from our middleware
        let pipeline = single_middleware(middleware);

        // construct a basic chain from our pipeline
        let (chain, pipelines) = single_pipeline(pipeline);

        // build a router with the chain & pipeline
        build_router(chain, pipelines, |route| {
            route.get("/access-token").to(access_token_handler);
            route.get("/player").to(player_handler);
        })
    }

    #[derive(RustEmbed)]
    #[folder = "frontend/"]
    struct Asset;

    pub fn player_handler(state: GothamState) -> (GothamState, Response<Body>) {
        use gotham::helpers::http::response::create_response;
        use hyper::header::HeaderValue;

        let index_html = Asset::get("index.html").unwrap();

        let mut res = create_response(
            &state,
            hyper::StatusCode::OK,
            mime::TEXT_HTML_UTF_8,
            index_html,
        );

        (state, res)
    }

    pub fn access_token_handler(mut state: GothamState) -> (GothamState, Response<Body>) {
        use gotham::helpers::http::response::create_response;
        use hyper::header::HeaderValue;

        let access_token_provider = state.borrow_mut::<AccessTokenProvider>(); //::borrow_mut(&state);
        let access_token = access_token_provider.get_token().unwrap();

        info!("Access token requested via endpoint");
        let mut res = create_response(
            &state,
            hyper::StatusCode::OK,
            mime::TEXT_PLAIN,
            access_token,
        );

        let headers = res.headers_mut();
        headers.insert(
            "Access-Control-Allow-Methods",
            HeaderValue::from_static("GET"),
        );
        headers.insert("Access-Control-Allow-Origin", HeaderValue::from_static("*"));
        headers.insert(
            "Access-Control-Allow-Headers",
            HeaderValue::from_static("content-type"),
        );

        (state, res)
    }

    pub fn spawn(port: u32, access_token_provider: AccessTokenProvider) -> Fallible<()> {
        let listen_addr = format!("127.0.0.1:{}", port);
        info!("Starting web server at {}", listen_addr);
        std::thread::spawn(|| gotham::start(listen_addr, router(access_token_provider)));

        Ok(())
    }
}
