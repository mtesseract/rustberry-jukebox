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

RUN cargo chef cook --target aarch64-unknown-linux-gnu --recipe-path recipe.json

COPY . .
RUN cargo build --target aarch64-unknown-linux-gnu

RUN rm -rf out && \
	mkdir -p out/bin out/lib && \
	cp target/aarch64-unknown-linux-gnu/debug/jukeboxd out/bin && \
	./scripts/copy-dyn-libs target/aarch64-unknown-linux-gnu/debug/jukeboxd out/lib
#	aarch64-linux-gnu-strip out/bin/jukeboxd

FROM --platform=linux/arm64/v8 alpine:3.16.9 AS pre-runtime
RUN apk add patchelf
RUN mkdir /app
COPY --from=builder /proj/out/ /app
COPY --from=builder /lib/ld-linux-aarch64.so.1 /app/lib
RUN patchelf --set-rpath /app/lib /app/bin/jukeboxd
RUN patchelf --set-interpreter /app/lib/ld-linux-aarch64.so.1 /app/bin/jukeboxd 

FROM --platform=linux/arm64/v8 alpine:3.16.9 AS runtime
RUN apk add alsa-utils pulseaudio-utils

COPY --from=pre-runtime /app /app
