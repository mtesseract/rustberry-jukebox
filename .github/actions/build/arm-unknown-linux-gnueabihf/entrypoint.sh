#!/bin/sh -l

set -e

echo "Building inside container..."

export PKG_CONFIG_ALLOW_CROSS=1

# Delete Cargo caches shipped with the container
rm -rf $CARGO_HOME/registry
rm -rf $CARGO_HOME/git

# And replace them with those coming from the GitHub Action environment.
ln -sf /github/home/caches/registry $CARGO_HOME/registry
ln -sf /github/home/caches/git $CARGO_HOME/git

mkdir _artifacts

# Build it
cargo build --release --bin jukeboxd --target=arm-unknown-linux-gnueabihf
cp target/arm-unknown-linux-gnueabihf/release/jukeboxd _artifacts

cargo build --bin jukeboxd --target=arm-unknown-linux-gnueabihf
cp target/arm-unknown-linux-gnueabihf/debug/jukeboxd _artifacts/jukeboxd-debug

cargo build --release --bin rustberry-write-spotify-uri --target=arm-unknown-linux-gnueabihf
cp target/arm-unknown-linux-gnueabihf/release/rustberry-write-spotify-uri _artifacts/

version=$(cargo pkgid | cut -d# -f2 | cut -d: -f2)
echo "::set-output name=version::${version}"
