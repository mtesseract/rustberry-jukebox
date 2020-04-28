#!/bin/sh -l

echo "Hello $1"
time=$(date)
echo "::set-output name=time::$time"

. /root/.cargo/env
export OPENSSL_LIB_DIR=/usr/local/openssl
export OPENSSL_INCLUDE_DIR=/usr/local/openssl/include
export OPENSSL_LIB_DIR=/usr/local/openssl
export OPENSSL_INCLUDE_DIR=/usr/local/openssl/include
export PKG_CONFIG_ALLOW_CROSS=1
cargo build --release --bin jukeboxd --target=armv7-unknown-linux-gnueabihf
