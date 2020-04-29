#!/bin/sh -l

TARGET_ARCH="$1"

echo "Building inside container..."

export OPENSSL_LIB_DIR=/usr/local/openssl
export OPENSSL_INCLUDE_DIR=/usr/local/openssl/include
export OPENSSL_LIB_DIR=/usr/local/openssl
export OPENSSL_INCLUDE_DIR=/usr/local/openssl/include
export PKG_CONFIG_ALLOW_CROSS=1
export PATH="/root/.cargo/bin:$PATH"
export CARGO_HOME=/root/.cargo
export RUSTUP_HOME=/root/.rustup

# Delete Cargo caches shipped with the container

rm -rf $CARGO_HOME/registry
rm -rf $CARGO_HOME/git

# And replace them with those coming from the GitHub Action environment.
ln -sf /github/home/caches/registry $CARGO_HOME/registry
ln -sf /github/home/caches/git $CARGO_HOME/git

cd jukeboxd
cargo build --release --bin jukeboxd --target=$TARGET_ARCH
mkdir _artifacts
cp jukeboxd/target/$TARGET_ARCH/release/jukeboxd _artifacts
