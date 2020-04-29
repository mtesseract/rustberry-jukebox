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

rm -rf $CARGO_HOME/registry
rm -rf $CARGO_HOME/git

ln -sf /github/home/caches/registry $CARGO_HOME/registry
ln -sf /github/home/caches/git $CARGO_HOME/git

cd jukeboxd
cargo build --release --bin jukeboxd --target=armv7-unknown-linux-gnueabihf