use anyhow::Result;
use dialoguer::Input;
use fehler::{throw, throws};
use thiserror::Error;
use url::Url;

use clap::{App, Arg};

use rustberry::meta_app::AppMode;
use rustberry::player::{PlaybackBackend, PlaybackResource};

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
                .required(true)
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

    let playback_backends = vec![PlaybackBackend::Spotify, PlaybackBackend::Http];
    let backend = match dialoguer::Select::with_theme(&dialoguer::theme::ColorfulTheme::default())
        .items(&playback_backends)
        .interact_opt()?
    {
        Some(choice) => playback_backends[choice].clone(),
        None => return (),
    };

    let resource = match backend {
        PlaybackBackend::Spotify => {
            let spotify_uri = Input::<String>::new()
                .with_prompt("Spotify URI")
                .interact()?;
            let spotify_uri = derive_spotify_uri_from_url(&spotify_uri)?;

            PlaybackResource::SpotifyUri(spotify_uri)
        }
        PlaybackBackend::Http => {
            let url = Input::<String>::new().with_prompt("HTTP URL").interact()?;
            PlaybackResource::Http(url)
        }
    };

    eprintln!("1");
    let current_mode: AppMode = client
        .get(url.join("/current-mode").unwrap())
        .send()?
        .json()?;
    eprintln!("2");
    println!("current_mode = {:?}", current_mode);

    if current_mode != AppMode::Admin {
        client
            .put(url.join("/current-mode").unwrap())
            .json(&AppMode::Admin)
            .send()?;
    }

    client
        .put(url.join("/admin/rfid-tag").unwrap())
        .json(&resource)
        .send()?;

    if current_mode != AppMode::Admin {
        client
            .put(url.join("/current-mode").unwrap())
            .json(&AppMode::Jukebox)
            .send()?;
    }
}

#[throws(std::io::Error)]
fn main() {
    match run_application() {
        Ok(_) => println!("Success"),
        Err(err) => {
            eprintln!("Error: {}", err);
            std::process::exit(1);
        }
    }
}
