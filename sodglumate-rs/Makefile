LIVE_SCENARIOS := $(basename $(notdir $(wildcard src/testing/performance/*.toml)))
SCENARIO ?= $(or $(filter $(LIVE_SCENARIOS),$(MAKECMDGOALS)),animated-high-res)
LIVE_BIN := target/perf/sodglumate-rs

.PHONY: run debug live live-perf perf-setup perf-sysctl build-win64 build-linux64 build-wasm licenses publish $(LIVE_SCENARIOS)

setup:
	sudo apt-get update
	sudo apt-get install -y clang
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

live:
	RUSTFLAGS='-C force-frame-pointers=y' cargo build --profile perf --features live-perf
	$(LIVE_BIN) --live-perf $(SCENARIO)

$(LIVE_SCENARIOS):

live-perf:
	RUSTFLAGS='-C force-frame-pointers=y' cargo build --profile perf --features live-perf
	perf record --call-graph dwarf -g $(LIVE_BIN) --live-perf $(SCENARIO)

perf-setup:
	sudo apt-get update
	sudo apt-get install -y linux-tools-common linux-tools-generic linux-tools-$(shell uname -r)

perf-sysctl:
	sudo sysctl -w kernel.perf_event_paranoid=-1
	sudo sysctl -w kernel.kptr_restrict=0

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
