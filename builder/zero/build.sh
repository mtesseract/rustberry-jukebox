#!/bin/sh

set -e
set -x

BUILDER=rustberry-builder-armv6
DEST=rustberry-kitchen
TARGET_ARCH=arm-unknown-linux-gnueabihf

DIR=$(ssh $BUILDER mktemp -d)

PROGRAM=${1:-jukeboxd}
BRANCH=${2:-master}
MODE=${3:-release}

echo "Building $PROGRAM on branch $BRANCH in $MODE mode using builder $BUILDER"

if [ "$MODE" == "debug" ]; then
    MODE_SWITCH=""
else
    MODE_SWITCH="--release"
fi

ssh $BUILDER "\
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
export PATH=/usr/local/arm-bcm2708/arm-linux-gnueabihf/bin:\$PATH && \
export PATH=/usr/local/arm-bcm2708/arm-linux-gnueabihf/libexec/gcc/arm-linux-gnueabihf/4.9.3:\$PATH && \
cargo build $MODE_SWITCH --bin $PROGRAM --target=$TARGET_ARCH --features raspberry
"

scp $BUILDER:$DIR/rustberry/jukeboxd/target/$TARGET_ARCH/$MODE/$PROGRAM .
scp $PROGRAM $DEST:~
