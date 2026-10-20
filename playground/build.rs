//! The crate versions the module was built from, for `info()`: read from
//! the manifests beside it, since this crate is outside their workspace.

use std::fs;

fn version(manifest: &str) -> String {
    println!("cargo:rerun-if-changed={manifest}");
    let text = fs::read_to_string(manifest).unwrap_or_default();
    let mut section = "";
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            section = line;
        } else if section == "[package]" {
            if let Some(value) = line
                .strip_prefix("version")
                .and_then(|v| v.trim_start().strip_prefix('='))
            {
                return value.trim().trim_matches('"').to_string();
            }
        }
    }
    "unknown".into()
}

fn main() {
    println!(
        "cargo:rustc-env=RS_RICH_VERSION={}",
        version("../crates/rich/Cargo.toml")
    );
    println!(
        "cargo:rustc-env=RS_RICH_EXT_VERSION={}",
        version("../crates/rich-ext/Cargo.toml")
    );
}
