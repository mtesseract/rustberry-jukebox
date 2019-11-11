# Rustberry Jukebox

So, I am one of those nerds, who wanted to build some kind of Jukebox device for
a child.

There are [commercial products](https://tonies.de/) and existing [hobbyist
projects](http://phoniebox.de/index-en.html), but for several reasons I decided
to work on my own project. It is called *Rustberry Jukebox* and the core
software is written in [Rust](https://www.rust-lang.org). Even though the name
might suggest otherwise, please note that this is not related to an existing
Rust [crate](https://crates.io/crates/rustberry) with a similar name.

The feature set I had in mind for the jukebox:
* Rustic/old aesthetics
* Wireless LAN connected
* Stream music via Spotify
* Playback controllable via RFID tags
* Hardware button for switching the jukebox on and off
* Status LEDs (Jukebox running and Jukebox playing)
* Software wise, the code should be conveniently cross-compilable for a Raspberry Pi.

This is the resulting device:
IMG

## Components

### The Case

Initially I had no precise idea of the intended look of the device -- what should
the case me made of, given that I am not particularly experienced when it
comes to physical manufacturing? Fortunately I found something, which seemed
promising an old looking [suitcase / treasure
hest](https://www.amazon.de/BRYNNBERG-Schatztruhe-Marco-38x27x14cm-Aufbewahrungsbox/dp/B07CMPTSD9/ref=sr_1_3?__mk_de_DE=%C3%85M%C3%85%C5%BD%C3%95%C3%91&crid=2X2CTDPJTEGAA&keywords=holzkiste+verschlie%C3%9Fbar&qid=1570263909&s=kitchen&sprefix=holzkiste+vers%2Ckitchen%2C165&sr=1-3).
I thought it might be feasible to build a double bottom into this suitcase,
having enough hidden space for all the tech stuff inside (Raspberry Pi,
speakers, power, circuits, cables / adapters) and providing enough space above
the double bottom for user controls and RFID tags.

IMG

### The Records

What should the RFID tags be attached to, if one does not want to use uninspired RFID
cards or transponders? Again, I was lucky to find a product, which
matches the intended rustic aesthetics and allows for cheap extensibility:
[wooden
discs](https://www.amazon.de/gp/product/B078HB4ZD7/ref=ppx_yo_dt_b_asin_title_o06_s00?ie=UTF8&language=en_GB&psc=1).
The idea is to stick the RFID tag to the bottom side of a disk and use the top
side for artwork.

IMG

### The Tech

I had an old Raspberry Pi 2 laying around, which I intended to use as a
technological foundation for the jukebox. The playback should be controllable
using RFID tags, but I was not familiar with this technology at all. After some
research I had identified the [MIFARE
RC522](http://wiki.sunfounder.cc/index.php?title=Mifare_RC522_Module_RFID_Reader)
as a common and well-supported RFID reader/writerfor suitable for [use with a
Raspberry Pi](https://pimylifeup.com/raspberry-pi-rfid-rc522/). This device is
compatible with RFID tags such as [MIFARE Classic 1K Chip,
13.56mhz](https://www.amazon.de/gp/product/B01HEU96C6).

For enabling audio output, I went with the [Trust Leto 2.0 USB
Speakers](https://www.amazon.de/gp/product/B00JRW0M32). I was somewhat worried
about the energy consumption (6W) of these speakers, since I prefer to not use
additional power sources besides the Raspberry Pi's own USB connectors for
compactness reasons. But they seem to work fine. The USB speakers connect to
the computer via a standard stereo jack, but as is well-known the stereo jack
output of the Raspberry Pi offers poor quality. Therefore I have decided to
extract the audio signal from the Raspberry HDMI output using a simple
[HDMI-to-VGA adapter](https://www.amazon.de/gp/product/B00ZMV7RL2) capable of extracting the HDMI audio signal.

## The Software

As mentioned above there are in fact already software solutions for an
RFID-controllable jukebox. But after a quick look at the [Phoniebox
Software](https://github.com/MiczFlor/RPi-Jukebox-RFID) I decided to build my
own project. The primary motives for this include the following:

* RPi-Jukebox-RFID seems like a rather huge Python project and I am not familiar
  with Python tooling. Also, I have not had the best experiences with Python
  codebases in the past, though I cannot judge about the quality of this
  particular project.
  
* Currently I am primarily interested in one particular
  use-case: Spotify integration -- which is labelled as "experimental" for
  RPio-Jukebox-RFID. I wanted something more compact and simple.

* From the introductory video of the Phoniebox it seems that the RFID-control
  logic is such that an RFID tag is used only for *triggering* playback. What I
  would prefer is that an RFID tag is used for controlling the playback, which
  means *starting* and *stopping* it, similar to the commercial Toniebox:
  Playback is active as long as the RFID tag is in range of the RFID reader.

After evaluation of a few options I decided to build the software with Rust,
since I learned to like that language, it performs well, has a great package
ecosystem, can be used for lower-level hardware access, comes with a low memory
footprint and it has a pretty good cross-compilation story.

Regarding Spotify Playback, my initial plan was to run Firefox on the Raspberry
Pi and use the [Spotify Web
SDK](https://developer.spotify.com/documentation/web-playback-sdk/) for
providing the Spotify streaming capabilities. This worked pretty well on my
development machine. But once I tried it out on the Raspberry I had learn the
hard way that the Spotify Web SDK requires Widevine DRM Support, which the
non-official Firefox builds do not contain (and for ARM there are no official
Firefox builds). So, how do stream from Spotify? Well,
[Librespot](https://github.com/librespot-org/librespot) comes to the rescue:

    librespot is an open source client library for Spotify. It enables applications to use Spotify's service, without using the official but closed-source libspotify. Additionally, it will provide extra features which are not available in the official library.

Librespot is used and packaged by the
[Raspotify](https://github.com/dtcooper/raspotify) project. They provide easy to
install Debian packages for ARM Raspbian. With Raspotify installed and
configured to use a specific Spotify Premium account, the Raspberry is ready to
be used as Spotify client through the [Spotify Web
API](https://developer.spotify.com/documentation/web-api/).

Actually I wanted to use [NixOS](https://nixos.org/) on the Raspberry Pi, but
unfortunately it was way to rough around the edges for my use-case. The issues I
have had with NixOS even on my Raspberry Pi 3, which comes with an AARCH64 CPU
somewhat supported by NixOS upstream, included:

* Missing and/or incomplete documentation, in particular when it comes to
  configuring the Raspberry Pi firmware and the boot process (after having
  written a first `configuration.nix` according to the documentation, the
  Raspberry Pi was unable to boot).
* The boot process is significantly slower than Raspbian's.
* After about 2h work I was still not able to get audio working -- something
  that just works on Raspbian.

With NixOS I like being able to declaratively configure the complete operating
system with all required services and deploy a system configuration to a remote
NixOS with complete rollback functionality built-in.  Maybe somewhen in the
future I can write the NixOS derivations for my Jukebox Software and deploy it
to a Raspberry Pi running NixOS. But this is not today.
