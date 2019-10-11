
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

    let index_html = Asset::get("index.html").unwrap();

    let res = create_response(
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
