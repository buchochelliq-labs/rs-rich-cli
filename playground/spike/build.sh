#!/bin/sh
# Build the spike for the browser: ./build.sh [cargo feature flags...]
# e.g. ./build.sh --no-default-features   (core, no syntect)
#      ./build.sh --features app           (core + the intuiTUIve to-do app)
# Needs: rustup target add wasm32-unknown-unknown, and wasm-bindgen-cli at
# the version pinned in Cargo.toml (cargo install wasm-bindgen-cli --version …).
set -eu
cd "$(dirname "$0")"
cargo build --release --target wasm32-unknown-unknown "$@"
target="${CARGO_TARGET_DIR:-target}/wasm32-unknown-unknown/release"
wasm-bindgen --target web --no-typescript --out-dir web/pkg "$target/rich_playground_spike.wasm"
# The page's terminal: rs-rich-web's vendored xterm.js (MIT, licence beside it).
mkdir -p web/xterm
cp ../../crates/rich-web/assets/xterm/xterm.js ../../crates/rich-web/assets/xterm/xterm.css \
   ../../crates/rich-web/assets/xterm/LICENSE-xterm web/xterm/
wasm=web/pkg/rich_playground_spike_bg.wasm
echo "wasm: $(wc -c < "$wasm") bytes raw, $(gzip -9c "$wasm" | wc -c) gzipped"
