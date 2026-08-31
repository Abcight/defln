setup:
	sudo apt-get update
	sudo apt-get install -y clang ffmpeg libavcodec-dev libavformat-dev libavutil-dev libavfilter-dev libavdevice-dev libswscale-dev libswresample-dev libgstreamer1.0-dev libgstreamer-plugins-base1.0-dev
	sudo apt-get install -y gcc-multilib mingw-w64
	rustup target add x86_64-pc-windows-gnu

run:
	cargo +nightly run --release

debug:
	cargo run

build-win64:
	cargo build --release --target x86_64-pc-windows-gnu

build-linux64:
	cargo build --release --target x86_64-unknown-linux-gnu
