#!/bin/sh

# docker stop rustberry-builder
# docker rm rustberry-builder
# docker run --name "rustberry-builder" -d -p 4022:22 --mount source=rustberry-builder,target=/cache rustberry-builder

set -e
set -x


# docker run --restart always --name rustberry-builder -d -p 4022:22
#   rustberry-builder

DIR=$(ssh rustberry-builder mktemp -d)

PROGRAM=${1:-jukeboxd}
BRANCH=${2:-master}

echo "Building $PROGRAM on branch $BRANCH"

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
cargo build --release --bin $PROGRAM --target=armv7-unknown-linux-gnueabihf
"

scp rustberry-builder:$DIR/rustberry/jukeboxd/target/armv7-unknown-linux-gnueabihf/release/$PROGRAM .
