//! Check the vendored xterm.js files against the hashes they were pinned at
//! (assets/xterm/README.md says where each came from). A file that changed
//! by accident, or by an update that did not update this list, fails the
//! build rather than reaching a browser.

use sha2::{Digest, Sha256};

/// Each vendored file and the SHA-256 of the copy in its npm package.
const PINNED: [(&str, &str); 3] = [
    (
        "assets/xterm/xterm.js",
        "14903579ff54664cd72f8e8699e6961a6272c21863ec1c3b118cdc8af5d4a972",
    ),
    (
        "assets/xterm/xterm.css",
        "854a7c0fb70e8b1a083c16797ab827299fb18744f5ad34f227b48337e33293c6",
    ),
    (
        "assets/xterm/addon-fit.js",
        "ba3ea256ce0620a0992a197d6c9baea64823fc93d8da07a9e366ca9943c18527",
    ),
];

fn main() {
    for (path, pinned) in PINNED {
        println!("cargo:rerun-if-changed={path}");
        let bytes = std::fs::read(path).unwrap_or_else(|e| panic!("reading {path}: {e}"));
        let hash: String = Sha256::digest(&bytes)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        assert!(
            hash == pinned,
            "{path} does not match its pinned SHA-256 (expected {pinned}, found {hash}); \
             see crates/rich-web/assets/xterm/README.md to update xterm.js"
        );
    }
}
