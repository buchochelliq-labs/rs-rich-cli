//! The official marketplace artifacts this repository builds: the WASM
//! plugins `textcase` and `wordplay`, the built-in micro packs as
//! `.richmicro` archives, and the official themes. The builds are
//! reproducible (the `wat` and `zip` crates at the versions in Cargo.lock,
//! stored entries, a fixed time; a theme is its file as it is), and their
//! SHA-256s are pinned in `tests/fixtures/official-artifacts.txt`, which
//! `tests/official_artifacts.rs` checks. The files the marketplace serves
//! are these bytes; the marketplace's own CI checks them against the
//! listings.
//!
//! Shared by `official_artifacts.rs` and `plugin_marketplace.rs`, whose
//! fixture marketplace lists [`artifacts`] (textcase 0.1.0 and the packs)
//! as the real marketplace does. [`all_artifacts`] adds the 0.0.23 set.

#![allow(dead_code)]

use std::io::Write;
use std::path::PathBuf;

/// Where the marketplace repository serves its own artifacts from.
pub const ARTIFACT_BASE: &str =
    "https://raw.githubusercontent.com/buchochelliq-labs/rs-rich-marketplace/main/artifacts";

/// The official WASM plugin's id and version (its manifest's).
pub const TEXTCASE: (&str, &str) = ("textcase", "0.1.0");
/// The rs-rich-cli commit textcase 0.1.0's listing names as its source.
pub const TEXTCASE_SOURCE_REVISION: &str = "124ea70593df1b0b4af65f0c1e9df1e11072ce12";

/// The official WASM plugins after textcase 0.1.0, each built from
/// `crates/rich-cli/official/wasm/ID-VERSION.wat`: one file per version, so
/// a listed version's source never changes.
pub const WASM: [(&str, &str); 2] = [("textcase", "0.2.0"), ("wordplay", "0.1.0")];

/// The official themes, each `crates/rich-cli/official/themes/ID.ini` as it
/// is, and the version each is listed at.
pub const THEMES: [(&str, &str); 5] = [
    ("dusk", "1.0.0"),
    ("paper", "1.0.0"),
    ("ocean", "1.0.0"),
    ("high-contrast", "1.0.0"),
    ("mono", "1.0.0"),
];

/// The built-in micro packs (`rich_micro::builtin::PACKS`) and the version
/// each `pack.json` names.
pub const PACKS: [(&str, &str); 3] = [("status", "1.0.0"), ("dev", "1.0.0"), ("fun", "1.0.0")];

fn crates_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..")
}

/// `textcase.wasm`: `crates/rich-plugin-api/examples/wasm/textcase.wat`,
/// assembled.
pub fn textcase_wasm() -> Vec<u8> {
    let source =
        std::fs::read_to_string(crates_dir().join("rich-plugin-api/examples/wasm/textcase.wat"))
            .unwrap();
    wat::parse_str(source).unwrap()
}

/// `official/wasm/ID-VERSION.wat`, assembled.
pub fn official_wasm(id: &str, version: &str) -> Vec<u8> {
    let path = official_dir().join(format!("wasm/{id}-{version}.wat"));
    wat::parse_str(std::fs::read_to_string(path).unwrap()).unwrap()
}

/// `official/themes/ID.ini`: the file is the artifact.
pub fn theme(id: &str) -> Vec<u8> {
    std::fs::read(theme_path(id)).unwrap()
}

/// Where an official theme's source is.
pub fn theme_path(id: &str) -> PathBuf {
    official_dir().join(format!("themes/{id}.ini"))
}

fn official_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("official")
}

/// A built-in pack (`crates/rich-micro/builtin/NAME/`) as a `.richmicro`
/// zip: every file under it, sorted by path, stored (PNG and GIF are
/// compressed already), dated 1980-01-01.
pub fn micro_pack(name: &str) -> Vec<u8> {
    let root = crates_dir().join("rich-micro/builtin").join(name);
    let mut files = Vec::new();
    collect(&root, "", &mut files);
    files.sort();
    let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Stored)
        .last_modified_time(zip::DateTime::default())
        .unix_permissions(0o644);
    for relative in files {
        let bytes = std::fs::read(root.join(&relative)).unwrap();
        writer.start_file(relative.as_str(), options).unwrap();
        writer.write_all(&bytes).unwrap();
    }
    writer.finish().unwrap().into_inner()
}

fn collect(dir: &std::path::Path, prefix: &str, out: &mut Vec<String>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let entry = entry.unwrap();
        let name = entry.file_name().into_string().unwrap();
        let relative = if prefix.is_empty() {
            name
        } else {
            format!("{prefix}/{name}")
        };
        if entry.file_type().unwrap().is_dir() {
            collect(&entry.path(), &relative, out);
        } else {
            out.push(relative);
        }
    }
}

/// The official artifacts of 0.0.22's listings, textcase 0.1.0 and the
/// micro packs: `(path under the marketplace's artifacts/, bytes)`.
pub fn artifacts() -> Vec<(String, Vec<u8>)> {
    let (id, version) = TEXTCASE;
    let mut all = vec![(format!("{id}/{id}-{version}.wasm"), textcase_wasm())];
    for (name, version) in PACKS {
        all.push((
            format!("{name}/{name}-{version}.richmicro"),
            micro_pack(name),
        ));
    }
    all
}

/// [`artifacts`], then the WASM plugins and themes added in 0.0.23.
pub fn all_artifacts() -> Vec<(String, Vec<u8>)> {
    let mut all = artifacts();
    for (id, version) in WASM {
        all.push((
            format!("{id}/{id}-{version}.wasm"),
            official_wasm(id, version),
        ));
    }
    for (id, version) in THEMES {
        all.push((format!("{id}/{id}-{version}.ini"), theme(id)));
    }
    all
}

/// The artifact's URL once the marketplace serves it.
pub fn url(path: &str) -> String {
    format!("{ARTIFACT_BASE}/{path}")
}

pub fn sha256(bytes: &[u8]) -> String {
    use sha2::Digest;
    sha2::Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}
