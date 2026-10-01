//! Writing packages (#568): the other side of [`crate::package`]. What
//! `rich micro create` runs after the [pipeline](crate::pipeline).
//!
//! [`write_package`] writes `manifest.json`, `static.png` and, for an
//! animation, `animation.gif`, as a package directory or a `.richmicro` zip
//! archive, then reads what it wrote back through the package reader under
//! the default [`Limits`], so a package it returns is one the registry
//! accepts.
//!
//! ```
//! use rich_art::image::{DynamicImage, Rgba, RgbaImage};
//! use rich_micro::create::{write_package, PackageSpec};
//! use rich_micro::pipeline::Pipeline;
//! use rich_micro::CellSize;
//!
//! let dir = std::env::temp_dir().join(format!("micro-doc-{}", std::process::id()));
//! let image = DynamicImage::ImageRgba8(RgbaImage::from_pixel(32, 32, Rgba([0, 160, 0, 255])));
//! let processed = Pipeline::new(CellSize::default()).process_image(&image);
//! let spec = PackageSpec::new("team/ok", "a green square").text("ok");
//! let asset = write_package(&dir.join("ok"), &spec, &processed, false)?;
//! assert_eq!(asset.name(), "team/ok");
//! # std::fs::remove_dir_all(&dir).ok();
//! # Ok::<(), rich_micro::MicroError>(())
//! ```

use std::io::Write;
use std::path::{Path, PathBuf};

use serde_json::{json, Map, Value};

use crate::error::MicroError;
use crate::model::{Layer, MicroAsset};
use crate::package::{self, Limits, MANIFEST, SCHEMA_VERSION};
use crate::pipeline::Processed;

/// What a package says about its asset, besides its images.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PackageSpec {
    pub name: String,
    pub alt: String,
    pub emoji: Option<String>,
    pub text: Option<String>,
    pub aliases: Vec<String>,
    pub version: Option<String>,
    pub license: Option<String>,
    pub author: Option<String>,
}

impl PackageSpec {
    pub fn new(name: impl Into<String>, alt: impl Into<String>) -> PackageSpec {
        PackageSpec {
            name: name.into(),
            alt: alt.into(),
            ..PackageSpec::default()
        }
    }

    pub fn emoji(mut self, emoji: impl Into<String>) -> Self {
        self.emoji = Some(emoji.into());
        self
    }

    pub fn text(mut self, text: impl Into<String>) -> Self {
        self.text = Some(text.into());
        self
    }

    pub fn alias(mut self, alias: impl Into<String>) -> Self {
        self.aliases.push(alias.into());
        self
    }

    pub fn version(mut self, version: impl Into<String>) -> Self {
        self.version = Some(version.into());
        self
    }

    pub fn license(mut self, license: impl Into<String>) -> Self {
        self.license = Some(license.into());
        self
    }

    pub fn author(mut self, author: impl Into<String>) -> Self {
        self.author = Some(author.into());
        self
    }

    /// Check it as the reader will: the name, the alt text, the fallbacks
    /// against `processed`'s size, the aliases.
    fn check(&self, processed: &Processed) -> Result<(), MicroError> {
        let mut asset = MicroAsset::new(&self.name, &self.alt)?.with_size(processed.size)?;
        if let Some(emoji) = &self.emoji {
            asset = asset.with_emoji(emoji)?;
        }
        if let Some(text) = &self.text {
            asset = asset.with_text(text)?;
        }
        for alias in &self.aliases {
            asset = asset.with_alias(alias)?;
        }
        for (key, value) in [
            ("version", &self.version),
            ("license", &self.license),
            ("author", &self.author),
        ] {
            if value
                .as_deref()
                .is_some_and(|value| value.chars().any(char::is_control))
            {
                return Err(MicroError::Manifest(format!(
                    "the {key} holds control characters"
                )));
            }
        }
        Ok(())
    }

    /// The manifest for `processed`, its files named as [`write_package`]
    /// names them.
    pub fn manifest(&self, processed: &Processed) -> Value {
        let mut map = Map::new();
        map.insert("schema_version".into(), json!(SCHEMA_VERSION));
        map.insert("name".into(), json!(self.name));
        map.insert("alt".into(), json!(self.alt.trim()));
        map.insert("size".into(), json!(processed.size.to_string()));
        map.insert(
            "kind".into(),
            json!(if processed.animated() {
                "animated"
            } else {
                "static"
            }),
        );
        let mut fallback = Map::new();
        if let Some(emoji) = &self.emoji {
            fallback.insert("emoji".into(), json!(emoji));
        }
        if let Some(text) = &self.text {
            fallback.insert("text".into(), json!(text));
        }
        if !fallback.is_empty() {
            map.insert("fallback".into(), Value::Object(fallback));
        }
        map.insert("static".into(), json!(STATIC));
        if processed.animated() {
            map.insert("animation".into(), json!(ANIMATION));
        }
        if !self.aliases.is_empty() {
            map.insert("aliases".into(), json!(self.aliases));
        }
        for (key, value) in [
            ("version", &self.version),
            ("license", &self.license),
            ("author", &self.author),
        ] {
            if let Some(value) = value {
                map.insert(key.into(), json!(value));
            }
        }
        Value::Object(map)
    }
}

/// The still image's file name in a written package.
pub const STATIC: &str = "static.png";
/// The animation's file name in a written package.
pub const ANIMATION: &str = "animation.gif";

fn io_error(path: &Path, error: impl std::fmt::Display) -> MicroError {
    MicroError::Io(format!("{}: {error}", path.display()))
}

/// The files of a package for `spec` and `processed`: `(name, bytes)`,
/// manifest first.
pub fn package_files(
    spec: &PackageSpec,
    processed: &Processed,
) -> Result<Vec<(&'static str, Vec<u8>)>, MicroError> {
    spec.check(processed)?;
    let manifest = serde_json::to_vec_pretty(&spec.manifest(processed))
        .map_err(|e| MicroError::Manifest(e.to_string()))?;
    let mut files = vec![(MANIFEST, manifest), (STATIC, processed.png()?)];
    if let Some(gif) = processed.gif()? {
        files.push((ANIMATION, gif));
    }
    Ok(files)
}

/// Write a package for `spec` and `processed` at `dest`: a directory, or
/// with `archive` a zip file (`dest` should then end in `.richmicro`).
/// `dest` must not exist. The package is read back under the default
/// [`Limits`] and returned, in the user layer; a package that would not
/// load is removed and its error returned.
pub fn write_package(
    dest: &Path,
    spec: &PackageSpec,
    processed: &Processed,
    archive: bool,
) -> Result<MicroAsset, MicroError> {
    let files = package_files(spec, processed)?;
    if dest.exists() {
        return Err(MicroError::Io(format!("{} already exists", dest.display())));
    }
    if let Some(parent) = dest.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent).map_err(|e| io_error(parent, e))?;
    }
    let written = if archive {
        write_archive(dest, &files)
    } else {
        write_directory(dest, &files)
    };
    let loaded =
        written.and_then(|()| package::load_package(dest, Layer::User, &Limits::default()));
    if loaded.is_err() {
        remove(dest);
    }
    loaded
}

fn write_directory(dest: &Path, files: &[(&str, Vec<u8>)]) -> Result<(), MicroError> {
    std::fs::create_dir(dest).map_err(|e| io_error(dest, e))?;
    for (name, bytes) in files {
        let path = dest.join(name);
        std::fs::write(&path, bytes).map_err(|e| io_error(&path, e))?;
    }
    Ok(())
}

fn write_archive(dest: &Path, files: &[(&str, Vec<u8>)]) -> Result<(), MicroError> {
    use zip::write::SimpleFileOptions;
    let file = std::fs::File::create_new(dest).map_err(|e| io_error(dest, e))?;
    let mut writer = zip::ZipWriter::new(file);
    let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    for (name, bytes) in files {
        writer
            .start_file(*name, options)
            .map_err(|e| io_error(dest, e))?;
        writer.write_all(bytes).map_err(|e| io_error(dest, e))?;
    }
    writer.finish().map_err(|e| io_error(dest, e))?;
    Ok(())
}

fn remove(path: &Path) {
    if path.is_dir() {
        let _ = std::fs::remove_dir_all(path);
    } else {
        let _ = std::fs::remove_file(path);
    }
}

/// Where a package for `name` goes in a layer directory: its name with `/`
/// as `.` (`status/ok` → `status.ok`), plus `.richmicro` for an archive.
pub fn package_path(layer_dir: &Path, name: &str, archive: bool) -> PathBuf {
    let stem = name.replace('/', ".");
    if archive {
        layer_dir.join(format!("{stem}.richmicro"))
    } else {
        layer_dir.join(stem)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::CellSize;
    use crate::pipeline::Pipeline;
    use rich_art::graphics::AnimationFrame;
    use rich_art::image::{Rgba, RgbaImage};
    use std::time::Duration;

    fn animated() -> Processed {
        let frame = |c: u8| AnimationFrame {
            image: RgbaImage::from_pixel(8, 8, Rgba([c, 0, 0, 255])),
            delay: Duration::from_millis(120),
        };
        Pipeline::new(CellSize::default()).process_frames(&[frame(10), frame(200)])
    }

    #[test]
    fn writes_directories_and_archives_the_reader_accepts() {
        let dir = tempfile::tempdir().unwrap();
        let processed = animated();
        let spec = PackageSpec::new("team/spin", "a red square blinking")
            .text("*")
            .license("MIT")
            .version("1.0.0");
        let asset = write_package(&dir.path().join("spin"), &spec, &processed, false).unwrap();
        assert_eq!(asset.kind(), crate::AssetKind::Animated);
        assert_eq!(asset.license(), Some("MIT"));
        let zipped = dir.path().join("spin.richmicro");
        let asset = write_package(&zipped, &spec, &processed, true).unwrap();
        assert!(asset.variants().animation.is_some());
        assert!(zipped.is_file());
        // Exists already.
        assert!(write_package(&zipped, &spec, &processed, true).is_err());
    }

    #[test]
    fn bad_metadata_writes_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let processed = animated();
        let dest = dir.path().join("bad");
        let spec = PackageSpec::new("Bad Name", "alt");
        assert!(write_package(&dest, &spec, &processed, false).is_err());
        let spec = PackageSpec::new("ok", "alt").text("TOO-WIDE");
        assert!(write_package(&dest, &spec, &processed, false).is_err());
        let spec = PackageSpec::new("ok", "alt").author("\u{1b}]0;title\u{7}");
        let error = write_package(&dest, &spec, &processed, false).unwrap_err();
        assert!(error.to_string().contains("control"), "{error}");
        assert!(!dest.exists());
    }

    #[test]
    fn package_paths() {
        let root = Path::new("/r");
        assert_eq!(package_path(root, "a/b", false), Path::new("/r/a.b"));
        assert_eq!(package_path(root, "a", true), Path::new("/r/a.richmicro"));
    }
}
