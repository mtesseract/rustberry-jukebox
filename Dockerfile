FROM --platform=$BUILDPLATFORM rust:1.74 AS planner
RUN rustup target add armv7-unknown-linux-gnueabihf
RUN cargo install cargo-chef
WORKDIR /proj
# Copy the whole project
COPY . .
# Prepare a build plan ("recipe")
RUN cargo chef prepare --recipe-path recipe.json

FROM --platform=$BUILDPLATFORM rust:1.74 AS builder
RUN rustup target add armv7-unknown-linux-gnueabihf
RUN cargo install cargo-chef
RUN dpkg --add-architecture armhf && apt-get update
RUN apt-get update && \
	apt-get -y install \
		binutils-arm-linux-gnueabihf \
		curl \
		vim \
		build-essential \
		libasound2-dev:armhf \
		gcc-arm-linux-gnueabihf \
		pkg-config:armhf
ENV PKG_CONFIG_PATH="/usr/lib/arm-linux-gnueabihf/pkgconfig"
ENV PKG_CONFIG_ALLOW_CROSS="true"
WORKDIR /proj
COPY --from=planner /proj/recipe.json recipe.json

RUN cargo chef cook --target armv7-unknown-linux-gnueabihf --recipe-path recipe.json

COPY . .
RUN cargo build --target armv7-unknown-linux-gnueabihf

RUN rm -rf out && \
	mkdir -p out/bin out/lib && \
	cp target/armv7-unknown-linux-gnueabihf/debug/jukeboxd out/bin && \
	./scripts/copy-dyn-libs target/armv7-unknown-linux-gnueabihf/debug/jukeboxd out/lib && \
	arm-linux-gnueabihf-strip out/bin/jukeboxd

FROM --platform=linux/arm/v7 alpine:3.12 AS runtime
COPY --from=builder /proj/out/ /usr/local
COPY --from=builder /lib/ld-linux-armhf.so* /lib
ENV LD_LIBRARY_PATH=/usr/local/lib
ENTRYPOINT ["/usr/local/bin/jukeboxd"]
