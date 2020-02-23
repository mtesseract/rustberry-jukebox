use failure::Fallible;

use rustberry::components::rfid::*;
use rustberry::player::PlaybackResource;

struct Written {
    _resource: PlaybackResource,
    _uid: String,
}

fn run_application() -> Fallible<Written> {
    let url = dialoguer::Input::<String>::new()
        .with_prompt("HTTP URL")
        .interact()?;
    let resource = PlaybackResource::Http(url);
    println!("Playback resource: {:?}", &resource);
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
            println!("Successfully written playback resource to RFID tag.");
        }
        Err(err) => {
            println!("Failed to write the playback resource to RFID tag: {}", err);
        }
    }
}
