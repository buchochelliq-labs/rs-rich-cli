//! `.richmicro` packages and packs (#580), read-only.
//!
//! A **package** is one asset: a zip archive or a directory holding a
//! `manifest.json` (`schema_version: 1`), a static image, and optionally an
//! animation (GIF, APNG or WebP) and a frame sequence:
//!
//! ```json
//! {
//!   "schema_version": 1,
//!   "name": "status/success",
//!   "alt": "green check mark",
//!   "size": "2x1",
//!   "fallback": { "emoji": "✅", "text": "OK" },
//!   "static": "static.png",
//!   "animation": "animation.gif",
//!   "aliases": ["ok"],
//!   "version": "1.0.0",
//!   "license": "CC0-1.0",
//!   "author": "rs-rich contributors"
//! }
//! ```
//!
//! A **pack** is a directory or zip archive with a `pack.json` listing its
//! packages (folders or, in a pack directory, `.richmicro` archives):
//!
//! ```json
//! { "schema_version": 1, "name": "status", "version": "1.0.0",
//!   "packages": ["success", "warning.richmicro"] }
//! ```
//!
//! Everything is untrusted input. Hard [`Limits`] bound the archive, every
//! file, the manifest, the entry count, pixel dimensions, frame counts and
//! the bytes an asset takes decoded; images are checked by their headers,
//! never decoded here. Paths must stay inside the package: absolute paths,
//! `..`, backslashes, and symbolic links (in an archive, any; in a
//! directory, any that leads out of it) are refused.

use std::cell::Cell;
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};

use serde_json::{Map, Value};

use crate::error::MicroError;
use crate::image::sniff;
use crate::model::{
    AssetKind, CellSize, ImageFormat, ImageRef, Layer, MicroAsset, Origin, PackageLocation,
    Variants,
};
use crate::name::check_name;

/// The manifest's file name.
pub const MANIFEST: &str = "manifest.json";
/// A pack's index file name.
pub const PACK_INDEX: &str = "pack.json";
/// The only manifest and pack schema this release reads.
pub const SCHEMA_VERSION: u64 = 1;

/// Hard limits on untrusted packages. The defaults suit emoji-sized assets
/// with generous headroom; anything beyond them is refused, not truncated.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Limits {
    /// An archive's size on disk.
    pub max_archive_bytes: u64,
    /// Any one file, uncompressed.
    pub max_file_bytes: u64,
    /// `manifest.json` and `pack.json`.
    pub max_manifest_bytes: u64,
    /// Entries in one archive.
    pub max_entries: usize,
    /// Bytes read, uncompressed, from one archive in total.
    pub max_total_bytes: u64,
    /// Width or height of any image, in pixels.
    pub max_dimension: u32,
    /// Frames in one asset: its animation's plus its frame sequence's.
    pub max_frames: u32,
    /// What one asset takes decoded (RGBA, every frame at canvas size).
    pub max_decoded_bytes: u64,
    /// Packages in one pack.
    pub max_packages: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Limits {
            max_archive_bytes: 8 << 20,
            max_file_bytes: 2 << 20,
            max_manifest_bytes: 64 << 10,
            max_entries: 512,
            max_total_bytes: 16 << 20,
            max_dimension: 256,
            max_frames: 240,
            max_decoded_bytes: 32 << 20,
            max_packages: 1024,
        }
    }
}

/// A loaded pack: its assets, and the packages in it that failed, each with
/// its reason. One bad package does not reject the pack.
#[derive(Clone, Debug)]
pub struct Pack {
    pub name: String,
    pub version: Option<String>,
    pub assets: Vec<MicroAsset>,
    /// `(package path within the pack, why it was rejected)`.
    pub rejected: Vec<(String, MicroError)>,
}

/// What a path held: one package or a pack.
#[derive(Clone, Debug)]
pub enum Loaded {
    Package(Box<MicroAsset>),
    Pack(Pack),
}

/// Check a path from a manifest or an archive: relative, `/`-separated, no
/// empty, `.` or `..` segments, no backslashes, drive letters or NULs.
/// Returns it normalised (without a trailing `/`).
pub fn safe_relative(path: &str) -> Result<String, MicroError> {
    let unsafe_path = |why: &str| MicroError::UnsafePath(format!("unsafe path {path:?}: {why}"));
    if path.is_empty() {
        return Err(unsafe_path("empty"));
    }
    if path.starts_with('/') {
        return Err(unsafe_path("absolute"));
    }
    if path.contains('\\') || path.contains('\0') || path.contains(':') {
        return Err(unsafe_path("backslashes, colons and NULs are not allowed"));
    }
    let trimmed = path.strip_suffix('/').unwrap_or(path);
    for segment in trimmed.split('/') {
        match segment {
            "" | "." => return Err(unsafe_path("empty or `.` segment")),
            ".." => return Err(unsafe_path("`..` leads out of the package")),
            _ => {}
        }
    }
    Ok(trimmed.to_string())
}

fn io_error(path: &Path, error: impl std::fmt::Display) -> MicroError {
    MicroError::Io(format!("{}: {error}", path.display()))
}

/// Read at most `limit` bytes; more is an error, whatever a header claimed.
fn read_bounded(mut reader: impl Read, limit: u64, what: &str) -> Result<Vec<u8>, MicroError> {
    let mut out = Vec::new();
    reader
        .by_ref()
        .take(limit + 1)
        .read_to_end(&mut out)
        .map_err(|e| MicroError::Io(format!("{what}: {e}")))?;
    if out.len() as u64 > limit {
        return Err(MicroError::Limit(format!(
            "{what} is larger than {limit} bytes"
        )));
    }
    Ok(out)
}

/// Where files are read from: a directory, or a checked zip archive.
enum Source {
    Directory {
        root: PathBuf,
        canonical: PathBuf,
    },
    Archive {
        path: PathBuf,
        archive: Box<zip::ZipArchive<File>>,
        read_total: Cell<u64>,
    },
    /// The compiled-in built-in set ([`crate::builtin`]), under `root`.
    Embedded {
        root: String,
    },
}

impl Source {
    fn directory(root: &Path) -> Result<Self, MicroError> {
        let canonical = root.canonicalize().map_err(|e| io_error(root, e))?;
        Ok(Source::Directory {
            root: root.to_path_buf(),
            canonical,
        })
    }

    /// Open an archive and check every entry before reading any: the size,
    /// the count, each name, and no links, encryption or duplicates.
    fn archive(path: &Path, limits: &Limits) -> Result<Self, MicroError> {
        let metadata = std::fs::metadata(path).map_err(|e| io_error(path, e))?;
        if metadata.len() > limits.max_archive_bytes {
            return Err(MicroError::Limit(format!(
                "{} is larger than {} bytes",
                path.display(),
                limits.max_archive_bytes
            )));
        }
        let file = File::open(path).map_err(|e| io_error(path, e))?;
        let mut archive = zip::ZipArchive::new(file).map_err(|e| io_error(path, e))?;
        if archive.len() > limits.max_entries {
            return Err(MicroError::Limit(format!(
                "{} has more than {} entries",
                path.display(),
                limits.max_entries
            )));
        }
        let mut seen = std::collections::HashSet::new();
        for index in 0..archive.len() {
            let entry = archive.by_index_raw(index).map_err(|e| io_error(path, e))?;
            let name = entry.name().to_string();
            let normal = safe_relative(&name)
                .map_err(|e| MicroError::UnsafePath(format!("{}: {e}", path.display())))?;
            if entry.is_symlink() {
                return Err(MicroError::UnsafePath(format!(
                    "{}: {name:?} is a symbolic link",
                    path.display()
                )));
            }
            if entry.encrypted() {
                return Err(MicroError::Manifest(format!(
                    "{}: {name:?} is encrypted",
                    path.display()
                )));
            }
            if !entry.is_dir() && entry.size() > limits.max_file_bytes {
                return Err(MicroError::Limit(format!(
                    "{}: {name:?} is larger than {} bytes",
                    path.display(),
                    limits.max_file_bytes
                )));
            }
            if !seen.insert(normal) {
                return Err(MicroError::Manifest(format!(
                    "{}: {name:?} appears twice",
                    path.display()
                )));
            }
        }
        Ok(Source::Archive {
            path: path.to_path_buf(),
            archive: Box::new(archive),
            read_total: Cell::new(0),
        })
    }

    fn display(&self, relative: &str) -> String {
        match self {
            Source::Directory { root, .. } => root.join(relative).display().to_string(),
            Source::Archive { path, .. } => format!("{}!{relative}", path.display()),
            Source::Embedded { root } => format!("builtin:{}", join(root, relative)),
        }
    }

    fn exists(&mut self, relative: &str) -> bool {
        match self {
            Source::Directory { root, .. } => root.join(relative).is_file(),
            Source::Archive { archive, .. } => archive.index_for_name(relative).is_some(),
            Source::Embedded { root } => crate::builtin::file(&join(root, relative)).is_some(),
        }
    }

    fn is_dir(&mut self, relative: &str) -> bool {
        match self {
            Source::Directory { root, .. } => root.join(relative).is_dir(),
            Source::Archive { archive, .. } => {
                let prefix = format!("{relative}/");
                archive.file_names().any(|name| name.starts_with(&prefix))
            }
            Source::Embedded { root } => crate::builtin::is_dir(&join(root, relative)),
        }
    }

    /// Read `relative` (already [`safe_relative`]), at most `limit` bytes.
    fn read(&mut self, relative: &str, limit: u64, limits: &Limits) -> Result<Vec<u8>, MicroError> {
        let shown = self.display(relative);
        match self {
            Source::Directory { root, canonical } => {
                let path = root.join(relative);
                let resolved = path.canonicalize().map_err(|e| io_error(&path, e))?;
                if !resolved.starts_with(&*canonical) {
                    return Err(MicroError::UnsafePath(format!(
                        "{shown} links out of the package"
                    )));
                }
                let metadata = std::fs::metadata(&resolved).map_err(|e| io_error(&path, e))?;
                if !metadata.is_file() {
                    return Err(MicroError::Io(format!("{shown} is not a file")));
                }
                if metadata.len() > limit {
                    return Err(MicroError::Limit(format!(
                        "{shown} is larger than {limit} bytes"
                    )));
                }
                let file = File::open(&resolved).map_err(|e| io_error(&path, e))?;
                read_bounded(file, limit, &shown)
            }
            Source::Archive {
                archive,
                read_total,
                ..
            } => {
                let entry = archive
                    .by_name(relative)
                    .map_err(|e| MicroError::Io(format!("{shown}: {e}")))?;
                if !entry.is_file() {
                    return Err(MicroError::Io(format!("{shown} is not a file")));
                }
                let left = limits.max_total_bytes.saturating_sub(read_total.get());
                let bytes = read_bounded(entry, limit.min(left), &shown).map_err(|e| {
                    if limit > left {
                        MicroError::Limit(format!(
                            "{shown}: the archive holds more than {} bytes uncompressed",
                            limits.max_total_bytes
                        ))
                    } else {
                        e
                    }
                })?;
                read_total.set(read_total.get() + bytes.len() as u64);
                Ok(bytes)
            }
            Source::Embedded { root } => {
                let bytes = crate::builtin::file(&join(root, relative))
                    .ok_or_else(|| MicroError::Io(format!("{shown}: no such file")))?;
                if bytes.len() as u64 > limit {
                    return Err(MicroError::Limit(format!(
                        "{shown} is larger than {limit} bytes"
                    )));
                }
                Ok(bytes.to_vec())
            }
        }
    }

    fn location(&self, prefix: &str) -> PackageLocation {
        match self {
            Source::Directory { root, .. } => PackageLocation::Directory(if prefix.is_empty() {
                root.clone()
            } else {
                root.join(prefix)
            }),
            Source::Archive { path, .. } => PackageLocation::Archive {
                path: path.clone(),
                prefix: prefix.to_string(),
            },
            Source::Embedded { root } => PackageLocation::BuiltIn(join(root, prefix)),
        }
    }
}

fn join(prefix: &str, relative: &str) -> String {
    if prefix.is_empty() {
        relative.to_string()
    } else {
        format!("{prefix}/{relative}")
    }
}

/// Read one file of a loaded package, under `limits`. Used by
/// [`MicroAsset::read_variant`].
pub(crate) fn read_file(
    location: &PackageLocation,
    relative: &str,
    limits: &Limits,
) -> Result<Vec<u8>, MicroError> {
    let relative = safe_relative(relative)?;
    match location {
        PackageLocation::Directory(root) => {
            Source::directory(root)?.read(&relative, limits.max_file_bytes, limits)
        }
        PackageLocation::Archive { path, prefix } => Source::archive(path, limits)?.read(
            &join(prefix, &relative),
            limits.max_file_bytes,
            limits,
        ),
        PackageLocation::BuiltIn(prefix) => Source::Embedded {
            root: prefix.clone(),
        }
        .read(&relative, limits.max_file_bytes, limits),
    }
}

/// Load the compiled-in pack at `root` (`status`) into `layer`.
pub(crate) fn load_embedded_pack(
    root: &str,
    layer: Layer,
    limits: &Limits,
) -> Result<Pack, MicroError> {
    let mut source = Source::Embedded {
        root: root.to_string(),
    };
    read_pack(&mut source, None, layer, limits)
}

fn parse_json(bytes: &[u8], what: &str) -> Result<Map<String, Value>, MicroError> {
    match serde_json::from_slice::<Value>(bytes) {
        Ok(Value::Object(map)) => Ok(map),
        Ok(_) => Err(MicroError::Manifest(format!("{what}: not a JSON object"))),
        Err(e) => Err(MicroError::Manifest(format!("{what}: invalid JSON: {e}"))),
    }
}

fn check_schema(map: &Map<String, Value>, what: &str) -> Result<(), MicroError> {
    match map.get("schema_version") {
        Some(Value::Number(n)) if n.as_u64() == Some(SCHEMA_VERSION) => Ok(()),
        Some(other) => Err(MicroError::Manifest(format!(
            "{what}: unsupported schema_version {other}; this release reads {SCHEMA_VERSION}"
        ))),
        None => Err(MicroError::Manifest(format!(
            "{what}: missing schema_version"
        ))),
    }
}

fn check_keys(map: &Map<String, Value>, allowed: &[&str], what: &str) -> Result<(), MicroError> {
    match map.keys().find(|key| !allowed.contains(&key.as_str())) {
        Some(key) => Err(MicroError::Manifest(format!(
            "{what}: unknown field {key:?}"
        ))),
        None => Ok(()),
    }
}

fn string<'a>(
    map: &'a Map<String, Value>,
    key: &str,
    what: &str,
) -> Result<Option<&'a str>, MicroError> {
    match map.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) => Ok(Some(s)),
        Some(_) => Err(MicroError::Manifest(format!(
            "{what}: {key:?} must be a string"
        ))),
    }
}

fn required<'a>(map: &'a Map<String, Value>, key: &str, what: &str) -> Result<&'a str, MicroError> {
    string(map, key, what)?.ok_or_else(|| MicroError::Manifest(format!("{what}: missing {key:?}")))
}

fn strings(map: &Map<String, Value>, key: &str, what: &str) -> Result<Vec<String>, MicroError> {
    match map.get(key) {
        None | Some(Value::Null) => Ok(Vec::new()),
        Some(Value::Array(items)) => items
            .iter()
            .map(|item| match item {
                Value::String(s) => Ok(s.clone()),
                _ => Err(MicroError::Manifest(format!(
                    "{what}: {key:?} must be a list of strings"
                ))),
            })
            .collect(),
        Some(_) => Err(MicroError::Manifest(format!(
            "{what}: {key:?} must be a list of strings"
        ))),
    }
}

fn size(map: &Map<String, Value>, what: &str) -> Result<CellSize, MicroError> {
    let in_manifest = |e: MicroError| MicroError::InvalidSize(format!("{what}: {e}"));
    match map.get("size") {
        None | Some(Value::Null) => Ok(CellSize::default()),
        Some(Value::String(s)) => s.parse().map_err(in_manifest),
        Some(Value::Object(size)) => {
            let get = |key: &str| match size.get(key) {
                Some(Value::Number(n)) => n
                    .as_u64()
                    .and_then(|n| u16::try_from(n).ok())
                    .ok_or_else(|| {
                        MicroError::InvalidSize(format!("{what}: size.{key} is out of range"))
                    }),
                None if key == "rows" => Ok(1),
                _ => Err(MicroError::InvalidSize(format!(
                    "{what}: size.{key} must be a number"
                ))),
            };
            CellSize::new(get("cols")?, get("rows")?).map_err(in_manifest)
        }
        Some(_) => Err(MicroError::InvalidSize(format!(
            "{what}: size must be \"COLSxROWS\" or {{\"cols\": …, \"rows\": …}}"
        ))),
    }
}

const MANIFEST_KEYS: &[&str] = &[
    "schema_version",
    "name",
    "kind",
    "size",
    "alt",
    "fallback",
    "static",
    "animation",
    "frames",
    "aliases",
    "version",
    "license",
    "author",
];

/// Read the package at `prefix` in `source`.
fn read_package(
    source: &mut Source,
    prefix: &str,
    origin: Origin,
    limits: &Limits,
) -> Result<MicroAsset, MicroError> {
    let manifest_path = join(prefix, MANIFEST);
    let what = source.display(&manifest_path);
    let bytes = source.read(&manifest_path, limits.max_manifest_bytes, limits)?;
    let map = parse_json(&bytes, &what)?;
    check_schema(&map, &what)?;
    check_keys(&map, MANIFEST_KEYS, &what)?;

    let name = required(&map, "name", &what)?;
    let alt = required(&map, "alt", &what)?;
    let in_manifest = |e: MicroError| match e {
        MicroError::InvalidName(_) => e,
        other => MicroError::Manifest(format!("{what}: {other}")),
    };
    let mut asset = MicroAsset::new(name, alt)
        .map_err(in_manifest)?
        .with_size(size(&map, &what)?)
        .map_err(in_manifest)?;
    match map.get("fallback") {
        None | Some(Value::Null) => {}
        Some(Value::Object(fallback)) => {
            check_keys(fallback, &["emoji", "text"], &format!("{what}: fallback"))?;
            if let Some(emoji) = string(fallback, "emoji", &what)? {
                asset = asset.with_emoji(emoji).map_err(in_manifest)?;
            }
            if let Some(text) = string(fallback, "text", &what)? {
                asset = asset.with_text(text).map_err(in_manifest)?;
            }
        }
        Some(_) => {
            return Err(MicroError::Manifest(format!(
                "{what}: fallback must be an object with \"emoji\" and/or \"text\""
            )))
        }
    }
    for alias in strings(&map, "aliases", &what)? {
        asset = asset.with_alias(&alias).map_err(in_manifest)?;
    }
    if let Some(version) = string(&map, "version", &what)? {
        asset = asset.with_version(version);
    }
    if let Some(license) = string(&map, "license", &what)? {
        asset = asset.with_license(license);
    }
    if let Some(author) = string(&map, "author", &what)? {
        asset = asset.with_author(author);
    }

    // Images: header-checked against the limits, never decoded.
    let mut load = |relative: &str, animation: bool| -> Result<ImageRef, MicroError> {
        let relative = safe_relative(relative)?;
        let full = join(prefix, &relative);
        let bytes = source.read(&full, limits.max_file_bytes, limits)?;
        let shown = source.display(&full);
        let info = sniff(&bytes, limits.max_frames)
            .map_err(|e| MicroError::Image(format!("{shown}: {e}")))?;
        if info.width > limits.max_dimension || info.height > limits.max_dimension {
            return Err(MicroError::Limit(format!(
                "{shown} is {}x{} pixels; the limit is {} on each side",
                info.width, info.height, limits.max_dimension
            )));
        }
        if info.frames > limits.max_frames {
            return Err(MicroError::Limit(format!(
                "{shown} has more than {} frames",
                limits.max_frames
            )));
        }
        if !animation && info.frames > 1 {
            return Err(MicroError::Image(format!(
                "{shown} is animated; the static image and frames must be still images"
            )));
        }
        if animation && info.format == ImageFormat::Png {
            return Err(MicroError::Image(format!(
                "{shown} is a still PNG; an animation is a GIF, APNG or WebP"
            )));
        }
        Ok(ImageRef {
            path: relative,
            info,
            bytes: bytes.len() as u64,
        })
    };
    let static_path = required(&map, "static", &what)?;
    let static_image = load(static_path, false)?;
    let animation = string(&map, "animation", &what)?
        .map(|path| load(path, true))
        .transpose()?;
    let frame_paths = strings(&map, "frames", &what)?;
    if frame_paths.len() as u64 > limits.max_frames as u64 {
        return Err(MicroError::Limit(format!(
            "{what}: more than {} frames",
            limits.max_frames
        )));
    }
    let frames = frame_paths
        .iter()
        .map(|path| load(path, false))
        .collect::<Result<Vec<_>, _>>()?;
    let variants = Variants {
        static_image: Some(static_image),
        animation,
        frames,
    };
    let total_frames: u64 = variants
        .iter()
        .map(|image| image.info.frames as u64)
        .sum::<u64>()
        - 1;
    if total_frames > limits.max_frames as u64 {
        return Err(MicroError::Limit(format!(
            "{what}: more than {} frames in all",
            limits.max_frames
        )));
    }
    let decoded: u64 = variants
        .iter()
        .map(|image| image.info.decoded_bytes())
        .sum();
    if decoded > limits.max_decoded_bytes {
        return Err(MicroError::Limit(format!(
            "{what}: its images take {decoded} bytes decoded; the limit is {}",
            limits.max_decoded_bytes
        )));
    }

    let kind = match string(&map, "kind", &what)? {
        Some(kind) => kind.parse().map_err(in_manifest)?,
        None if variants.animation.is_some() || !variants.frames.is_empty() => AssetKind::Animated,
        None => AssetKind::Static,
    };
    let asset = asset
        .with_kind(kind)
        .with_variants(variants)
        .with_origin(Origin {
            location: Some(source.location(prefix)),
            ..origin
        });
    asset.validate().map_err(in_manifest)?;
    Ok(asset)
}

const PACK_KEYS: &[&str] = &[
    "schema_version",
    "name",
    "version",
    "description",
    "license",
    "author",
    "packages",
];

/// Read the pack whose `pack.json` is at `source`'s root.
fn read_pack(
    source: &mut Source,
    pack_dir: Option<&Path>,
    layer: Layer,
    limits: &Limits,
) -> Result<Pack, MicroError> {
    let what = source.display(PACK_INDEX);
    let bytes = source.read(PACK_INDEX, limits.max_manifest_bytes, limits)?;
    let map = parse_json(&bytes, &what)?;
    check_schema(&map, &what)?;
    check_keys(&map, PACK_KEYS, &what)?;
    let name = required(&map, "name", &what)?;
    check_name(name)?;
    let packages = strings(&map, "packages", &what)?;
    if packages.len() > limits.max_packages {
        return Err(MicroError::Limit(format!(
            "{what}: more than {} packages",
            limits.max_packages
        )));
    }
    let mut pack = Pack {
        name: name.to_string(),
        version: string(&map, "version", &what)?.map(str::to_string),
        assets: Vec::new(),
        rejected: Vec::new(),
    };
    for package in packages {
        let origin = Origin {
            layer,
            pack: Some(pack.name.clone()),
            location: None,
        };
        let result = safe_relative(&package).and_then(|relative| {
            if source.is_dir(&relative) {
                return read_package(source, &relative, origin, limits);
            }
            // A `.richmicro` archive inside a pack directory. (Inside a pack
            // archive, packages are folders: archives do not nest.)
            match (pack_dir, source.exists(&relative)) {
                (Some(dir), true) => {
                    let path = dir.join(&relative);
                    // `read` checks it does not link out of the pack.
                    let canonical_dir = dir.canonicalize().map_err(|e| io_error(dir, e))?;
                    let resolved = path.canonicalize().map_err(|e| io_error(&path, e))?;
                    if !resolved.starts_with(&canonical_dir) {
                        return Err(MicroError::UnsafePath(format!(
                            "{} links out of the pack",
                            path.display()
                        )));
                    }
                    let mut inner = Source::archive(&path, limits)?;
                    read_package(&mut inner, "", origin, limits)
                }
                _ => Err(MicroError::Io(format!(
                    "{}: no such package",
                    source.display(&relative)
                ))),
            }
        });
        match result {
            Ok(asset) => pack.assets.push(asset),
            Err(error) => pack.rejected.push((package, error)),
        }
    }
    Ok(pack)
}

/// Load a package from a directory or a zip archive.
pub fn load_package(path: &Path, layer: Layer, limits: &Limits) -> Result<MicroAsset, MicroError> {
    match load(path, layer, limits)? {
        Loaded::Package(asset) => Ok(*asset),
        Loaded::Pack(_) => Err(MicroError::Manifest(format!(
            "{} is a pack, not a package",
            path.display()
        ))),
    }
}

/// Load a pack from a directory or a zip archive.
pub fn load_pack(path: &Path, layer: Layer, limits: &Limits) -> Result<Pack, MicroError> {
    match load(path, layer, limits)? {
        Loaded::Pack(pack) => Ok(pack),
        Loaded::Package(_) => Err(MicroError::Manifest(format!(
            "{} is a package, not a pack",
            path.display()
        ))),
    }
}

/// Load whatever `path` holds: a package (`manifest.json`) or a pack
/// (`pack.json`), as a directory or a zip archive.
pub fn load(path: &Path, layer: Layer, limits: &Limits) -> Result<Loaded, MicroError> {
    let metadata = std::fs::symlink_metadata(path).map_err(|e| io_error(path, e))?;
    let is_dir = if metadata.file_type().is_symlink() {
        std::fs::metadata(path)
            .map_err(|e| io_error(path, e))?
            .is_dir()
    } else {
        metadata.is_dir()
    };
    let mut source = if is_dir {
        Source::directory(path)?
    } else {
        Source::archive(path, limits)?
    };
    let origin = Origin {
        layer,
        pack: None,
        location: None,
    };
    if source.exists(MANIFEST) {
        read_package(&mut source, "", origin, limits).map(|asset| Loaded::Package(Box::new(asset)))
    } else if source.exists(PACK_INDEX) {
        read_pack(&mut source, is_dir.then_some(path), layer, limits).map(Loaded::Pack)
    } else {
        Err(MicroError::Manifest(format!(
            "{}: no {MANIFEST} or {PACK_INDEX}",
            path.display()
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relative_paths() {
        assert_eq!(safe_relative("a/b.png").unwrap(), "a/b.png");
        assert_eq!(safe_relative("frames/").unwrap(), "frames");
        for bad in [
            "",
            "/etc/passwd",
            "../x",
            "a/../../x",
            "a/./b",
            "a//b",
            "C:/x",
            "a\\b",
            "a\0",
        ] {
            assert!(
                matches!(safe_relative(bad), Err(MicroError::UnsafePath(_))),
                "{bad:?}"
            );
        }
    }
}
