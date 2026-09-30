//! Package builders for the conformance tests (#586).
#![allow(dead_code)]

use std::io::Write;
use std::path::{Path, PathBuf};

use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

pub fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

/// A PNG header (signature, IHDR, IDAT, IEND): enough for the header check.
pub fn png(width: u32, height: u32) -> Vec<u8> {
    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut chunk = |kind: &[u8], data: &[u8]| {
        out.extend((data.len() as u32).to_be_bytes());
        out.extend(kind);
        out.extend(data);
        out.extend([0; 4]);
    };
    let mut ihdr = width.to_be_bytes().to_vec();
    ihdr.extend(height.to_be_bytes());
    ihdr.extend([8, 6, 0, 0, 0]);
    chunk(b"IHDR", &ihdr);
    chunk(b"IDAT", &[]);
    chunk(b"IEND", &[]);
    out
}

/// A GIF with `frames` frames.
pub fn gif(width: u16, height: u16, frames: u32) -> Vec<u8> {
    let mut out = b"GIF89a".to_vec();
    out.extend(width.to_le_bytes());
    out.extend(height.to_le_bytes());
    out.extend([0x80, 0, 0, 0, 0, 0, 255, 255, 255]);
    for _ in 0..frames {
        out.extend([0x21, 0xF9, 4, 0, 10, 0, 0, 0, 0x2C, 0, 0, 0, 0]);
        out.extend(width.to_le_bytes());
        out.extend(height.to_le_bytes());
        out.extend([0, 2, 2, 0x4C, 0x01, 0]);
    }
    out.push(0x3B);
    out
}

pub fn manifest(name: &str) -> serde_json::Value {
    serde_json::json!({
        "schema_version": 1,
        "name": name,
        "alt": format!("{name} icon"),
        "fallback": {"text": "ok"},
        "static": "static.png",
    })
}

/// Write a package directory: `manifest.json` and `files`.
pub fn package_dir(dir: &Path, manifest: &serde_json::Value, files: &[(&str, Vec<u8>)]) {
    std::fs::create_dir_all(dir).unwrap();
    std::fs::write(dir.join("manifest.json"), serde_json::to_vec_pretty(manifest).unwrap())
        .unwrap();
    for (name, bytes) in files {
        let path = dir.join(name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, bytes).unwrap();
    }
}

/// A minimal valid package directory named `name`.
pub fn simple_package(dir: &Path, name: &str) {
    package_dir(dir, &manifest(name), &[("static.png", png(16, 16))]);
}

pub enum Entry<'a> {
    File(&'a str, Vec<u8>),
    Deflated(&'a str, Vec<u8>),
    Symlink(&'a str, &'a str),
    Dir(&'a str),
}

/// Write a zip archive of `entries`.
pub fn zip(path: &Path, entries: &[Entry<'_>]) {
    let file = std::fs::File::create(path).unwrap();
    let mut writer = ZipWriter::new(file);
    let stored = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
    let deflated = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    for entry in entries {
        match entry {
            Entry::File(name, bytes) => {
                writer.start_file(*name, stored).unwrap();
                writer.write_all(bytes).unwrap();
            }
            Entry::Deflated(name, bytes) => {
                writer.start_file(*name, deflated).unwrap();
                writer.write_all(bytes).unwrap();
            }
            Entry::Symlink(name, target) => {
                writer.add_symlink(*name, *target, stored).unwrap();
            }
            Entry::Dir(name) => {
                writer.add_directory(*name, stored).unwrap();
            }
        }
    }
    writer.finish().unwrap();
}

pub fn manifest_bytes(manifest: &serde_json::Value) -> Vec<u8> {
    serde_json::to_vec(manifest).unwrap()
}
