# Rustberry Jukebox

This is the *Rustberry Jukebox* software project, which can be used for turning a
Raspberry Pi into a simple jukebox device with the music playback being
implemented via [Spotify](https://www.spotify.com) and controlled with RFID tags. It is implemented in [Rust](https://www.rust-lang.org).

## Summary

*Rustberry Jukebox* consists of the following:

* A service (`jukeboxd`), which processes commands from hardware periphery (GPIOs, including
  an attached MF RC522 RFID reader). Such commands include:
  * Playback start/stop requests,
  * volume control (to be implemented).
  
  Playback requests are derived
  from RFID tags as seen by the RFID reader and induce calls to the Spotify Web
  API allowing the service to control a Spotify client. Other commands are associated
  with GPIO events (i.e. button press).

* CLI tools for reading and writing RFID tags using the format expected by
  `jukeboxd`.

* A Docker image and shell scripts used for cross-compiling all included
  binaries for the armv7 architecture used by Raspberry Pi.

## Status

This is very much work in progress.
But there exists already one happy user (my daughter).

## Build Environment

```
$ docker run --rm -it -v $PWD:/tmp/src -w /tmp/src
mtesseract/rustberry-builder-arm-unknown-linux-gnueabihf:latest /bin/bash
```

And then, for example:
```
$ cargo watch -x 'check --target=arm-unknown-linux-gnueabihf'
```


