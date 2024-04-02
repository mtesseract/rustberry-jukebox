build-jukeboxd-for-rpi:
	cargo build --release --bin jukeboxd && ./scripts/patch-bin target/release/jukeboxd

.PHONY: build-jukeboxd-for-rpi