#!/bin/sh -l

set -x

echo "Hello $1"
time=$(date)
echo "::set-output name=time::$time"

export OPENSSL_LIB_DIR=/usr/local/openssl
export OPENSSL_INCLUDE_DIR=/usr/local/openssl/include
export OPENSSL_LIB_DIR=/usr/local/openssl
export OPENSSL_INCLUDE_DIR=/usr/local/openssl/include
export PKG_CONFIG_ALLOW_CROSS=1
export PATH="/root/.cargo/bin:$PATH"
export CARGO_HOME=/root/.cargo
export RUSTUP_HOME=/root/.rustup

echo caches
ls /github/home/caches
echo .

ln -sf /github/home/caches/registry $CARGO_HOME/registry
# ln -sf /github/home/caches/target /github/workspace/jukeboxd/target

ls -lh $CARGO_HOME/
ls -lh $CARGO_HOME/registry

cd jukeboxd
# cargo build-deps --release  --target=armv7-unknown-linux-gnueabihf
cargo build --release --bin jukeboxd --target=armv7-unknown-linux-gnueabihf
# cp -r target target-deps
#cargo build --release --bin jukeboxd --target=armv7-unknown-linux-gnueabihf