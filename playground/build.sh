#!/bin/sh
# Build the docs playground's modules for the site.
#
#   playground/build.sh [OUT]                 # default OUT: docs/assets/playground
#   playground/build.sh --tar VERSION [OUT]   # also writes playground-VERSION.tar.gz
#                                             # beside OUT, for a GitHub release
#
# OUT gets:
#   lean/   rs-rich without syntect (most blocks)        rich_playground{.js,_bg.wasm}
#   full/   with syntect and pulldown-cmark (`syntax`, `markdown` blocks)
#   cli/    the `rich` CLI for wasm32-wasip1 (tier 2's terminal)  rich.wasm
#   xterm/  rs-rich-web's vendored xterm.js and its fit addon (MIT, licences beside them)
#   playground.json   the stamp the page checks first: modules, kinds, versions
#
# Needs: rustup target add wasm32-unknown-unknown wasm32-wasip1; wasm-bindgen-cli at the
# version pinned in Cargo.toml; Node 22 (the stamp is written by loading each
# module); wasm-opt from binaryen (skipped, with a warning, when missing,
# unless PLAYGROUND_REQUIRE_WASM_OPT=1). Set CARGO_TARGET_DIR to share a build.
set -eu
here=$(cd "$(dirname "$0")" && pwd)
root=$(dirname "$here")
tar_version=
if [ "${1:-}" = "--tar" ]; then
    tar_version=$2
    shift 2
fi
out=${1:-$root/docs/assets/playground}
mkdir -p "$out"
out=$(cd "$out" && pwd)
target="${CARGO_TARGET_DIR:-$here/target}/wasm32-unknown-unknown/release"

# The features Rust's wasm32-unknown-unknown target emits; wasm-opt must
# accept them and use nothing newer.
opt_features="--enable-bulk-memory --enable-nontrapping-float-to-int --enable-sign-ext
  --enable-mutable-globals --enable-reference-types --enable-multivalue"

build() {
    name=$1
    shift
    cargo build --manifest-path "$here/Cargo.toml" --release --target wasm32-unknown-unknown "$@"
    rm -rf "${out:?}/$name"
    wasm-bindgen --target web --no-typescript --out-dir "$out/$name" "$target/rich_playground.wasm"
    wasm="$out/$name/rich_playground_bg.wasm"
    raw=$(wc -c < "$wasm")
    if command -v wasm-opt > /dev/null; then
        # shellcheck disable=SC2086
        wasm-opt -Oz $opt_features "$wasm" -o "$wasm.opt"
        mv "$wasm.opt" "$wasm"
    elif [ "${PLAYGROUND_REQUIRE_WASM_OPT:-0}" = 1 ]; then
        echo "wasm-opt not found (binaryen)" >&2
        exit 1
    else
        echo "warning: wasm-opt not found; $name is not size-optimised" >&2
    fi
    echo "$name: $raw bytes from wasm-bindgen, $(wc -c < "$wasm") after wasm-opt," \
         "$(gzip -9c "$wasm" | wc -c) gzipped"
}

build lean
build full --features full

# Tier 2: the `rich` CLI on WASI. Its features: no network (`fetch`), no
# PTY (`record`, `png`) and no raw-terminal input (`interact`); `art` and
# `mermaid` build and run there. Sized as the modules above are.
cli_features="art,mermaid"
CARGO_PROFILE_RELEASE_OPT_LEVEL=z CARGO_PROFILE_RELEASE_LTO=true \
CARGO_PROFILE_RELEASE_CODEGEN_UNITS=1 CARGO_PROFILE_RELEASE_PANIC=abort \
CARGO_PROFILE_RELEASE_STRIP=true \
    cargo build --manifest-path "$root/Cargo.toml" --locked --release --target wasm32-wasip1 \
        -p rs-rich-cli --no-default-features --features "$cli_features" --bin rich
rm -rf "${out:?}/cli"
mkdir -p "$out/cli"
cli_wasm="${CARGO_TARGET_DIR:-$root/target}/wasm32-wasip1/release/rich.wasm"
raw=$(wc -c < "$cli_wasm")
if command -v wasm-opt > /dev/null; then
    # shellcheck disable=SC2086
    wasm-opt -Oz $opt_features "$cli_wasm" -o "$out/cli/rich.wasm"
elif [ "${PLAYGROUND_REQUIRE_WASM_OPT:-0}" = 1 ]; then
    echo "wasm-opt not found (binaryen)" >&2
    exit 1
else
    cp "$cli_wasm" "$out/cli/rich.wasm"
fi
echo "cli: $raw bytes from cargo, $(wc -c < "$out/cli/rich.wasm") after wasm-opt," \
     "$(gzip -9c "$out/cli/rich.wasm" | wc -c) gzipped"

# The page's terminal, for the blocks a reader switches to it and the CLI.
mkdir -p "$out/xterm"
for file in xterm.js xterm.css LICENSE-xterm addon-fit.js LICENSE-addon-fit; do
    cp "$root/crates/rich-web/assets/xterm/$file" "$out/xterm/"
done

node "$here/stamp.mjs" "$out"

if [ -n "$tar_version" ]; then
    archive="$(dirname "$out")/playground-$tar_version.tar.gz"
    tar -czf "$archive" -C "$out" playground.json lean full cli xterm
    echo "wrote $archive ($(wc -c < "$archive") bytes)"
fi
