use failure::Fallible;
use regex::Regex;

use rustberry::playback_requests::PlaybackRequest;
use rustberry::rfid::*;

fn derive_spotify_uri_from_url(url: &str) -> Fallible<String> {
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
    Ok(uri)
}

struct Written {
    _request: PlaybackRequest,
    _uid: String,
}

fn run_application() -> Fallible<Written> {
    let url = dialoguer::Input::<String>::new()
        .with_prompt("Spotify URL")
        .interact()?;
    let uri = derive_spotify_uri_from_url(&url)?;
    let request = PlaybackRequest::SpotifyUri(uri);
    println!("Play Request: {:?}", &request);
    let request_deserialized = serde_json::to_string(&request)?;
    let mut rc = RfidController::new()?;
    let tag = rc.open_tag().expect("Failed to open RFID tag").unwrap();
    let uid = format!("{:?}", tag.uid);
    println!("RFID Tag UID: {}", uid);
    let mut tag_writer = tag.new_writer();
    tag_writer.write_string(&request_deserialized)?;
    Ok(Written { _request: request, _uid: uid })
}

fn main() {
    match run_application() {
        Ok(_written) => {
            println!("Successfully written play request to RFID tag.");
        }
        Err(err) => {
            println!("Failed to write the play request to RFID tag: {}", err);
        }
    }
}
