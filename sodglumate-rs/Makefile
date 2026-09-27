.PHONY: run debug build-win64 build-linux64 build-wasm licenses publish

setup:
	sudo apt-get update
	sudo apt-get install -y clang ffmpeg libavcodec-dev libavformat-dev libavutil-dev libavfilter-dev libavdevice-dev libswscale-dev libswresample-dev libgstreamer1.0-dev libgstreamer-plugins-base1.0-dev
	sudo apt-get install -y gcc-multilib mingw-w64
	sudo apt install -y clang lld nsis
	rustup target add x86_64-pc-windows-gnu
	rustup target add x86_64-pc-windows-msvc
	cargo install --locked --features cli cargo-about
	cargo install cargo-packager --locked
	cargo install --locked cargo-xwin

run:
	cargo run --release

debug:
	cargo run

build-win64:
	cargo xwin build --release --target x86_64-pc-windows-msvc

build-linux64:
	cargo build --release --target x86_64-unknown-linux-gnu

build-wasm:
	cargo build --release --target wasm32-unknown-unknown

licenses:
	cargo about generate --locked --config distribution/about.toml distribution/about.hbs --output-file distribution/license.html

publish-linux: licenses
	cargo build --release --target x86_64-unknown-linux-gnu
	cargo packager --release --target x86_64-unknown-linux-gnu --formats appimage

publish-windows: licenses
	cargo xwin build --release --target x86_64-pc-windows-msvc
	cargo packager --release --target x86_64-pc-windows-msvc --formats nsis
