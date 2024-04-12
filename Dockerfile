FROM --platform=$BUILDPLATFORM rust:1.76 AS planner
RUN rustup target add aarch64-unknown-linux-gnu
RUN cargo install cargo-chef
WORKDIR /proj
# Copy the whole project
COPY . .
# Prepare a build plan ("recipe")
RUN cargo chef prepare --recipe-path recipe.json

FROM --platform=$BUILDPLATFORM rust:1.76 AS builder
RUN rustup target add aarch64-unknown-linux-gnu
RUN cargo install cargo-chef
RUN dpkg --add-architecture arm64 && apt-get update
RUN apt-get update && \
	apt-get -y install \
		binutils-aarch64-linux-gnu \
		gcc-aarch64-linux-gnu \
		curl \
		vim \
		build-essential \
		libasound2-dev:arm64 \
		pkg-config:arm64
ENV PKG_CONFIG_PATH="/usr/lib/aarch64-linux-gnu/pkgconfig"
ENV PKG_CONFIG_ALLOW_CROSS="true"
WORKDIR /proj
COPY --from=planner /proj/recipe.json recipe.json

RUN cargo chef cook --release --target aarch64-unknown-linux-gnu --recipe-path recipe.json

COPY . .
RUN cargo build --release --target aarch64-unknown-linux-gnu

FROM --platform=linux/arm64/v8 debian:12 as runtime
RUN apt-get update && apt-get dist-upgrade -y && \
	apt-get -y install \
		libasound2 tini alsa-utils
RUN mkdir -p /app/bin

COPY --from=builder /proj/target/aarch64-unknown-linux-gnu/release/jukeboxd /app/bin
COPY --from=builder /proj/scripts/jukeboxd-wrapper /app/bin

ENTRYPOINT ["/usr/bin/tini", "--"]