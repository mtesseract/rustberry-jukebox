#!/bin/sh

set -e
set -x

# docker run --restart always --name rustberry-builder -d -p 4022:22
#   rustberry-builder

DIR=$(ssh rustberry-builder mktemp -d)

PROGRAM=${1:-jukeboxd}
BRANCH=${2:-master}
MODE=${3:-release}

echo "Building $PROGRAM on branch $BRANCH in $MODE mode"

if [ "$MODE" == "debug" ]; then
    MODE_SWITCH=""
else
    MODE_SWITCH="--release"
fi

ssh rustberry-builder "\
set -x && \
. ~/.cargo/env && \
cd $DIR && \
git clone https://github.com/mtesseract/rustberry.git && \
cd rustberry/jukeboxd && \
git checkout $BRANCH && \
ln -sf /cache target
export OPENSSL_LIB_DIR=/usr/local/openssl && \
export OPENSSL_INCLUDE_DIR=/usr/local/openssl/include && \
export OPENSSL_LIB_DIR=/usr/local/openssl && \
export OPENSSL_INCLUDE_DIR=/usr/local/openssl/include && \
export PKG_CONFIG_ALLOW_CROSS=1 && \
cargo build $MODE_SWITCH --bin $PROGRAM --target=armv7-unknown-linux-gnueabihf
"

scp rustberry-builder:$DIR/rustberry/jukeboxd/target/armv7-unknown-linux-gnueabihf/$MODE/$PROGRAM .
scp $PROGRAM rustberry:~
