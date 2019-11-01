#!/bin/sh

# docker stop rustberry-builder
# docker rm rustberry-builder
# docker run --name "rustberry-builder" -d -p 4022:22 --mount source=rustberry-builder,target=/cache rustberry-builder

set -e

DIR=$(ssh rustberry-builder mktemp -d)

# ssh rustberry-builder "\
# set -x && \
# . ~/.cargo/env && \
# cd /tmp/rustberry && \
# git pull && \
# cd jukebox/$PROGRAM && \
# export OPENSSL_LIB_DIR=/usr/local/openssl && \
# export OPENSSL_INCLUDE_DIR=/usr/local/openssl/include && \
# cargo build --bin jukeboxd --target=armv7-unknown-linux-gnueabihf
# "

PROGRAM="jukeboxd"

ssh rustberry-builder "\
set -x && \
. ~/.cargo/env && \
cd $DIR && \
git clone https://github.com/mtesseract/rustberry.git && \
cd rustberry/jukebox/jukeboxd && \
ln -sf /cache target
export OPENSSL_LIB_DIR=/usr/local/openssl && \
export OPENSSL_INCLUDE_DIR=/usr/local/openssl/include && \
export OPENSSL_LIB_DIR=/usr/local/openssl && \
export OPENSSL_INCLUDE_DIR=/usr/local/openssl/include && \
cargo build --release --bin $PROGRAM --target=armv7-unknown-linux-gnueabihf
"

scp rustberry-builder:$DIR/rustberry/jukebox/jukeboxd/target/armv7-unknown-linux-gnueabihf/release/$PROGRAM .
