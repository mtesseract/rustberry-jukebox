use failure::Fallible;
use regex::Regex;

use rustberry::components::rfid::*;
use rustberry::player::PlaybackResource;

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
    _resource: PlaybackResource,
    _uid: String,
}

fn run_application() -> Fallible<Written> {
    let url = dialoguer::Input::<String>::new()
        .with_prompt("Spotify URL")
        .interact()?;
    let uri = derive_spotify_uri_from_url(&url)?;
    let resource = PlaybackResource::SpotifyUri(uri);
    println!("Play Resource: {:?}", &resource);
    let resource_deserialized = serde_json::to_string(&resource)?;
    let mut rc = RfidController::new()?;
    let tag = rc.open_tag().expect("Failed to open RFID tag").unwrap();
    let uid = format!("{:?}", tag.uid);
    println!("RFID Tag UID: {}", uid);
    let mut tag_writer = tag.new_writer();
    tag_writer.write_string(&resource_deserialized)?;
    Ok(Written {
        _resource: resource,
        _uid: uid,
    })
}

fn main() {
    match run_application() {
        Ok(_written) => {
            println!("Successfully written play resource to RFID tag.");
        }
        Err(err) => {
            println!("Failed to write the play resource to RFID tag: {}", err);
        }
    }
}
