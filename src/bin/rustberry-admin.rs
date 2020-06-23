use anyhow::Result;
use dialoguer::Input;
use fehler::{throw, throws};
use thiserror::Error;
use url::Url;

use clap::{App, Arg};

use rustberry::config::Config;
use rustberry::meta_app::AppMode;
use rustberry::player::PlaybackResource;

use regex::Regex;

#[throws(Error)]
fn derive_spotify_uri_from_url(url: &str) -> String {
    let re = Regex::new(r"https://open.spotify.com/(?P<type>(track|album))/(?P<id>[a-zA-Z0-9]+)")
        .expect("Failed to compile regex");
    let uri = match re.captures(&url) {
        Some(captures) => {
            println!("ok");
            format!("spotify:{}:{}", &captures["type"], &captures["id"])
        }
        None => {
            println!("Failed to parse Spotify URL: {}", url);
            std::process::exit(1);
        }
    };
    uri
}

#[derive(Error, Debug)]
pub enum Error {
    #[error("HTTP API Error")]
    HttpError(#[from] reqwest::Error),
    #[error("Input Error")]
    Input(#[from] std::io::Error),
}

#[throws(Error)]
fn run_application() {
    let matches = App::new("Rustberry Admin")
        .about("Admin CLI for Rustberry")
        .arg(
            Arg::with_name("url")
                .long("url")
                .value_name("URL")
                .help("Specifies Rustberry Server")
                .takes_value(true),
        )
        .arg(
            Arg::with_name("verbosity")
                .long("verbose")
                .short("v")
                .help("Sets the level of verbosity"),
        )
        .get_matches();

    let client = reqwest::blocking::Client::new();

    let url_s = format!("{}/", matches.value_of("url").unwrap());
    let url = Url::parse(&url_s).unwrap();

    let spotify_uri = Input::<String>::new()
        .with_prompt("Spotify URI")
        .interact()?;
    let spotify_uri = derive_spotify_uri_from_url(&spotify_uri)?;

    let resource = PlaybackResource::SpotifyUri(spotify_uri);

    let current_mode: AppMode = client.get(url.join("/mode").unwrap()).send()?.json()?;
    println!("current_mode = {:?}", current_mode);

    if current_mode != AppMode::Admin {
        client.get(url.join("/mode-admin").unwrap()).send()?;
    }

    client
        .put(url.join("/admin/rfid-tag").unwrap())
        .json(&resource)
        .send()?;

    if current_mode != AppMode::Admin {
        client.get(url.join("/mode-jukebox").unwrap()).send()?;
    }
}

#[throws(std::io::Error)]
fn main() {
    run_application()
}
