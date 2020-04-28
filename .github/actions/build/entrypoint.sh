#!/bin/sh -l

echo "Hello $1"
time=$(date)
echo "::set-output name=time::$time"

export OPENSSL_LIB_DIR=/usr/local/openssl
export OPENSSL_INCLUDE_DIR=/usr/local/openssl/include
export OPENSSL_LIB_DIR=/usr/local/openssl
export OPENSSL_INCLUDE_DIR=/usr/local/openssl/include
export PKG_CONFIG_ALLOW_CROSS=1
export PATH="/root/.cargo/bin:$PATH"
cargo build --release --bin jukeboxd --target=armv7-unknown-linux-gnueabihf
