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
#   xterm/  rs-rich-web's vendored xterm.js (MIT, its licence beside it)
#   playground.json   the stamp the page checks first: modules, kinds, versions
#
# Needs: rustup target add wasm32-unknown-unknown; wasm-bindgen-cli at the
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

# The page's terminal, for the blocks a reader switches to it.
mkdir -p "$out/xterm"
for file in xterm.js xterm.css LICENSE-xterm; do
    cp "$root/crates/rich-web/assets/xterm/$file" "$out/xterm/"
done

node "$here/stamp.mjs" "$out"

if [ -n "$tar_version" ]; then
    archive="$(dirname "$out")/playground-$tar_version.tar.gz"
    tar -czf "$archive" -C "$out" playground.json lean full xterm
    echo "wrote $archive ($(wc -c < "$archive") bytes)"
fi
