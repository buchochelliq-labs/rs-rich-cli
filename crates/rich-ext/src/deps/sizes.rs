//! Artifact sizes (#706): what each crate adds to a build, from a Cargo
//! target directory or from cargo-bloat, as bars and a table.
//!
//! [`Sizes::from_target_dir`] reads one profile's directory (`target/debug`,
//! `target/release`, a cross build's `target/<triple>/<profile>`, or that
//! directory itself) and matches each artifact to a package in `cargo
//! metadata` output:
//!
//! - libraries in `deps/`: `lib<crate>-<hash>.rlib`, else the `.rmeta` a
//!   check build leaves; proc-macros and dylibs (`.so`, `.dylib`, `.dll`)
//!   and static libraries (`.a`, `.lib`);
//! - binaries in the profile directory itself, named after a workspace
//!   member's `bin` target.
//!
//! An artifact's package is found by the sources its dep-info file
//! (`deps/<crate>-<hash>.d`) lists, which sit in one package's directory,
//! else by its crate name (a crate resolved at two versions needs the
//! dep-info file). What `cargo check` leaves for a package's tests,
//! examples and binaries (their `.rmeta`) is not a library and is skipped.
//! Older builds leave artifacts behind, so of each package's artifacts of a
//! kind the newest is taken. Artifacts no package claims are counted, not
//! shown.
//!
//! With [`SizeOptions::sources`], each package's source size is added: the
//! files under its manifest directory, leaving out `target/`, `.git/` and
//! nested packages, without following symbolic links.
//!
//! [`Sizes::parse`] reads `cargo bloat --crates --message-format json`
//! output (or its per-function listing, without `--crates`, added up by
//! crate), or a report [`Sizes::to_json`] saved.
//!
//! [`SizeReport`] draws the largest as bars, then a table, with the total
//! above. Running `cargo` (or `cargo bloat`) is the caller's business.
//!
//! ```
//! use rich::Console;
//! use rich_ext::deps::sizes::{SizeReport, Sizes};
//!
//! let bloat = r#"{"file-size": 3145728, "text-section-size": 1048576,
//!   "crates": [{"name": "std", "size": 409600}, {"name": "regex", "size": 307200},
//!              {"name": "app", "size": 102400}]}"#;
//! let sizes = Sizes::parse(bloat).unwrap();
//! assert_eq!(sizes.total(), 819_200);
//! let console = Console::builder().width(80).color_system(None).build();
//! let out = console.render_to_string(&SizeReport::new(sizes));
//! assert!(out.starts_with("3 crates: 800.0 KiB of code in a 3.0 MiB binary (1.0 MiB of .text)"));
//! ```

use std::collections::{BTreeMap, HashMap, HashSet};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use rich::cells::cell_len;
use rich::table::Table;
use rich::{Console, ConsoleOptions, Justify, Renderable, Segment, Text};
use serde_json::{json, Value};

use super::{
    check_count, check_size, clean, saved_report, stack, stack_measure, theme_style, DepsError,
    SAVED_FORMAT, SAVED_VERSION,
};
use crate::chart::BarChart;
use crate::format;

/// The largest units [`SizeReport`] shows by default.
pub const DEFAULT_LIMIT: usize = 20;
/// The most directory entries read from a profile's `deps/` (or from the
/// profile directory itself); more is refused.
pub const MAX_ENTRIES: usize = 1_000_000;
/// The most files counted under one package's directory for its source
/// size; past it the size is not known.
pub const MAX_SOURCE_FILES: usize = 100_000;
/// The deepest directory walked for a source size.
pub const MAX_SOURCE_DEPTH: usize = 64;
/// The most bytes read of one dep-info file.
pub const MAX_DEP_INFO: u64 = 16 * 1024 * 1024;

/// What an artifact is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ArtifactKind {
    /// A Rust library: the `.rlib`.
    Lib,
    /// A library's metadata only (`.rmeta`): what `cargo check` leaves.
    Rmeta,
    /// A procedural macro, loaded by the compiler.
    ProcMacro,
    /// A dynamic library (`dylib` or `cdylib`).
    Dylib,
    /// A static library (`staticlib`).
    Staticlib,
    /// An executable.
    Bin,
    /// A crate's share of a binary, as cargo-bloat measures it.
    Crate,
}

impl ArtifactKind {
    /// The word for it: `lib`, `rmeta`, `proc-macro`, ….
    pub fn name(self) -> &'static str {
        match self {
            ArtifactKind::Lib => "lib",
            ArtifactKind::Rmeta => "rmeta",
            ArtifactKind::ProcMacro => "proc-macro",
            ArtifactKind::Dylib => "dylib",
            ArtifactKind::Staticlib => "staticlib",
            ArtifactKind::Bin => "bin",
            ArtifactKind::Crate => "crate",
        }
    }

    /// The kind [`name`](Self::name) writes.
    pub fn from_name(name: &str) -> Option<Self> {
        [
            ArtifactKind::Lib,
            ArtifactKind::Rmeta,
            ArtifactKind::ProcMacro,
            ArtifactKind::Dylib,
            ArtifactKind::Staticlib,
            ArtifactKind::Bin,
            ArtifactKind::Crate,
        ]
        .into_iter()
        .find(|kind| kind.name() == name)
    }

    /// Whether it is named by its crate alone: a library or a crate's
    /// share of a binary.
    fn by_crate(self) -> bool {
        matches!(
            self,
            ArtifactKind::Lib | ArtifactKind::Rmeta | ArtifactKind::Crate
        )
    }
}

/// One crate's (or binary's) size.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SizeEntry {
    /// The crate's name, or the binary's.
    pub name: String,
    /// The package's version, when known.
    pub version: Option<String>,
    /// Cargo's package id, when the artifact was matched to one.
    pub id: Option<String>,
    pub kind: ArtifactKind,
    pub bytes: u64,
    /// The bytes of the package's source files, when measured.
    pub source_bytes: Option<u64>,
    /// The artifact's file name, for one read from a target directory.
    pub file: Option<String>,
}

impl SizeEntry {
    /// An entry of `bytes`.
    pub fn new(name: impl Into<String>, kind: ArtifactKind, bytes: u64) -> Self {
        SizeEntry {
            name: name.into(),
            version: None,
            id: None,
            kind,
            bytes,
            source_bytes: None,
            file: None,
        }
    }

    /// The name a [comparison](super::compare) matches it by: the crate's
    /// name for a library, else the name and the kind (`app (bin)`).
    pub fn key(&self) -> String {
        if self.kind.by_crate() {
            self.name.clone()
        } else {
            format!("{} ({})", self.name, self.kind.name())
        }
    }
}

/// Where the sizes came from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SizeSource {
    /// A profile directory of a target directory.
    TargetDir {
        /// The profile directory, as given (`target/debug`).
        dir: String,
        /// Its name: `debug`, `release`, ….
        profile: String,
        /// Crates with artifacts there that no package claims.
        unmatched: usize,
    },
    /// cargo-bloat's measure of one binary.
    Bloat {
        file_size: Option<u64>,
        text_size: Option<u64>,
    },
    /// Entries built by the caller.
    Entries,
}

/// How [`Sizes::from_target_dir`] reads a target directory.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[must_use = "options do nothing until they are passed to what takes them"]
pub struct SizeOptions {
    /// The profile: `dev` (the default, `debug/`), `release`, `test`,
    /// `bench`, or a custom profile's name. Without one, `debug/` is read
    /// if it was built, else `release/`.
    pub profile: Option<String>,
    /// Also measure each package's source files.
    pub sources: bool,
}

impl SizeOptions {
    /// Read this profile's artifacts.
    pub fn profile(mut self, profile: impl Into<String>) -> Self {
        self.profile = Some(profile.into());
        self
    }

    /// Measure each package's source files too.
    pub fn sources(mut self, sources: bool) -> Self {
        self.sources = sources;
        self
    }
}

/// The directory a profile builds into: `dev` and `test` into `debug`,
/// `bench` into `release`, any other profile into its own name.
pub fn profile_dir_name(profile: &str) -> &str {
    match profile {
        "dev" | "test" => "debug",
        "bench" => "release",
        other => other,
    }
}

fn shown(path: &Path) -> String {
    clean(&path.display().to_string()).into_owned()
}

/// The profile directory to read under `target`, and its name: `target`
/// itself when it holds `deps/`; else `target/<profile>` (without a
/// profile, `debug` if it was built, else `release`); else the one cross
/// build's `target/<triple>/<profile>`.
pub fn profile_dir(target: &Path, profile: Option<&str>) -> Result<(PathBuf, String), DepsError> {
    if let Some(profile) = profile {
        let valid = !profile.is_empty()
            && profile
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
        if !valid {
            return Err(DepsError::new(format!(
                "{:?} is not a profile name",
                clean(profile)
            )));
        }
    }
    let wanted = profile.map(profile_dir_name);
    if target.join("deps").is_dir() {
        let name = target
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default()
            .to_string();
        if let Some(wanted) = wanted.filter(|wanted| *wanted != name) {
            return Err(DepsError::new(format!(
                "{} is a profile directory ({}), not {wanted}'s",
                shown(target),
                clean(&name)
            )));
        }
        return Ok((target.to_path_buf(), name));
    }
    let names: Vec<&str> = match wanted {
        Some(wanted) => vec![wanted],
        None => vec!["debug", "release"],
    };
    for name in &names {
        let dir = target.join(name);
        if dir.join("deps").is_dir() {
            return Ok((dir, (*name).to_string()));
        }
    }
    let mut cross = Vec::new();
    if let Ok(entries) = std::fs::read_dir(target) {
        for entry in entries.flatten().take(MAX_ENTRIES) {
            if let Some(name) = names
                .iter()
                .find(|name| entry.path().join(name).join("deps").is_dir())
            {
                cross.push((entry.path().join(name), (*name).to_string()));
            }
        }
    }
    cross.sort();
    match cross.len() {
        1 => Ok(cross.remove(0)),
        0 => Err(DepsError::new(format!(
            "{}: no {} directory with artifacts in it (build first, or name the profile)",
            shown(target),
            names.join(" or ")
        ))),
        _ => Err(DepsError::new(format!(
            "{}: several targets were built ({}): give one of their directories",
            shown(target),
            cross
                .iter()
                .map(|(dir, _)| shown(dir.parent().unwrap_or(dir)))
                .collect::<Vec<_>>()
                .join(", ")
        ))),
    }
}

/// The target directory `cargo metadata` names.
pub fn target_directory(metadata: &str) -> Result<PathBuf, DepsError> {
    let value: Value = serde_json::from_str(metadata)
        .map_err(|e| DepsError::new(format!("not cargo metadata JSON: {e}")))?;
    value
        .get("target_directory")
        .and_then(Value::as_str)
        .map(PathBuf::from)
        .ok_or_else(|| DepsError::new("not cargo metadata: no `target_directory`"))
}

/// What `cargo metadata` says of a package that matters here.
struct MetaPackage {
    id: String,
    name: String,
    version: String,
    dir: Option<PathBuf>,
    /// Its library's crate name (`-` as `_`).
    lib: Option<String>,
    /// Whether that library is a proc-macro, when the targets say.
    proc_macro: Option<bool>,
    /// Its binaries' names, for a workspace member.
    bins: Vec<String>,
    /// The crate names of its other targets (tests, examples, benches,
    /// binaries), whose artifacts `cargo check` leaves in `deps/` too.
    others: Vec<String>,
}

fn read_packages(value: &Value) -> Result<Vec<MetaPackage>, DepsError> {
    let list = value
        .get("packages")
        .and_then(Value::as_array)
        .ok_or_else(|| DepsError::new("not cargo metadata: no `packages`"))?;
    check_count(list.len(), "packages")?;
    let members: HashSet<&str> = value
        .get("workspace_members")
        .and_then(Value::as_array)
        .map(|ids| ids.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    let mut packages = Vec::with_capacity(list.len());
    for package in list {
        let field = |key: &str| package.get(key).and_then(Value::as_str);
        let (Some(id), Some(name), Some(version)) = (field("id"), field("name"), field("version"))
        else {
            return Err(DepsError::new(
                "not cargo metadata: a package has no id, name or version",
            ));
        };
        let member = members.contains(id);
        let dir = field("manifest_path")
            .and_then(|path| Path::new(path).parent())
            .filter(|dir| !dir.as_os_str().is_empty())
            .map(Path::to_path_buf);
        let mut lib = None;
        let mut proc_macro = None;
        let mut bins = Vec::new();
        let mut others = Vec::new();
        match package.get("targets").and_then(Value::as_array) {
            Some(targets) => {
                for target in targets {
                    let Some(target_name) = target.get("name").and_then(Value::as_str) else {
                        continue;
                    };
                    let kinds: Vec<&str> = target
                        .get("kind")
                        .and_then(Value::as_array)
                        .map(|kinds| kinds.iter().filter_map(Value::as_str).collect())
                        .unwrap_or_default();
                    let library = kinds.iter().any(|kind| {
                        matches!(
                            *kind,
                            "lib" | "rlib" | "dylib" | "cdylib" | "staticlib" | "proc-macro"
                        )
                    });
                    if library {
                        lib = Some(target_name.replace('-', "_"));
                        proc_macro = Some(kinds.contains(&"proc-macro"));
                    } else {
                        others.push(target_name.replace('-', "_"));
                    }
                    if member && kinds.contains(&"bin") {
                        bins.push(target_name.to_string());
                    }
                }
            }
            // No targets (trimmed metadata): the defaults Cargo infers.
            None => {
                lib = Some(name.replace('-', "_"));
                if member {
                    bins.push(name.to_string());
                }
            }
        }
        packages.push(MetaPackage {
            id: id.to_string(),
            name: name.to_string(),
            version: version.to_string(),
            dir,
            lib,
            proc_macro,
            bins,
            others,
        });
    }
    Ok(packages)
}

/// A library artifact's file type.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
enum Ext {
    Rlib,
    Rmeta,
    Dylib,
    Staticlib,
}

/// The crate name, hash and type in a `deps/` file name: `libsyn-0a1b….rlib`,
/// `serde_derive-0a1b….dll`. `None` for anything else.
fn split_artifact(file: &str) -> Option<(&str, &str, Ext)> {
    let (stem, extension) = file.rsplit_once('.')?;
    let (ext, prefixed) = match extension {
        "rlib" => (Ext::Rlib, true),
        "rmeta" => (Ext::Rmeta, true),
        "so" | "dylib" => (Ext::Dylib, true),
        "a" => (Ext::Staticlib, true),
        // Windows names neither with `lib`.
        "dll" => (Ext::Dylib, false),
        "lib" => (Ext::Staticlib, false),
        _ => return None,
    };
    let stem = if prefixed {
        stem.strip_prefix("lib")?
    } else {
        stem
    };
    let (name, hash) = split_hash(stem)?;
    Some((name, hash, ext))
}

/// `name-<hex>` split at the hash.
fn split_hash(stem: &str) -> Option<(&str, &str)> {
    let (name, hash) = stem.rsplit_once('-')?;
    (!name.is_empty() && !hash.is_empty() && hash.chars().all(|c| c.is_ascii_hexdigit()))
        .then_some((name, hash))
}

/// The paths a dep-info file's first rule lists after `target:`, with
/// Make's `\ ` escapes undone.
fn dep_info_sources(path: &Path) -> Vec<PathBuf> {
    let mut text = Vec::new();
    let read =
        std::fs::File::open(path).and_then(|file| file.take(MAX_DEP_INFO).read_to_end(&mut text));
    if read.is_err() {
        return Vec::new();
    }
    let text = String::from_utf8_lossy(&text);
    let Some((_, sources)) = text
        .lines()
        .find(|line| line.contains(": "))
        .and_then(|line| line.split_once(": "))
    else {
        return Vec::new();
    };
    let mut paths = Vec::new();
    let mut current = String::new();
    let mut chars = sources.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\\' if chars.peek() == Some(&' ') => {
                current.push(' ');
                chars.next();
            }
            ' ' | '\t' => {
                if !current.is_empty() {
                    paths.push(PathBuf::from(std::mem::take(&mut current)));
                }
            }
            c => current.push(c),
        }
    }
    if !current.is_empty() {
        paths.push(PathBuf::from(current));
    }
    paths
}

/// An artifact found in `deps/`.
struct Found {
    crate_name: String,
    hash: String,
    ext: Ext,
    bytes: u64,
    modified: Option<SystemTime>,
    file: String,
}

/// Whose an artifact is: a package, or (when no dep-info said which of
/// several) a crate name.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
enum Owner {
    Package(usize),
    Crate(String),
}

/// What an artifact in `deps/` turned out to be.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Claim {
    /// A library, and whose.
    Library(Owner),
    /// One of a package's other targets, checked (`cargo check` and clippy
    /// leave a test's or a binary's `.rmeta`): not shown.
    Other,
    /// No package's.
    Unmatched,
}

/// The bytes under a package's directory, leaving out `target/`, `.git/`
/// and nested packages; `None` when it cannot be read or holds more than
/// [`MAX_SOURCE_FILES`] files.
fn source_size(dir: &Path) -> Option<u64> {
    std::fs::read_dir(dir).ok()?;
    let mut total = 0u64;
    let mut files = 0usize;
    let mut stack = vec![(dir.to_path_buf(), 0usize)];
    while let Some((here, depth)) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&here) else {
            continue;
        };
        for entry in entries.flatten() {
            // A directory entry's metadata does not follow symbolic links.
            let Ok(meta) = entry.metadata() else {
                continue;
            };
            if meta.is_dir() {
                let name = entry.file_name();
                if name == "target" || name == ".git" || depth + 1 > MAX_SOURCE_DEPTH {
                    continue;
                }
                let path = entry.path();
                if path.join("Cargo.toml").is_file() {
                    continue;
                }
                stack.push((path, depth + 1));
            } else if meta.is_file() {
                files += 1;
                if files > MAX_SOURCE_FILES {
                    return None;
                }
                total = total.saturating_add(meta.len());
            }
        }
    }
    Some(total)
}

/// The sizes of a build's crates. See the [module docs](self).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Sizes {
    entries: Vec<SizeEntry>,
    source: SizeSource,
}

impl Sizes {
    /// These entries, largest first.
    pub fn new(entries: Vec<SizeEntry>) -> Self {
        Sizes::with_source(entries, SizeSource::Entries)
    }

    fn with_source(mut entries: Vec<SizeEntry>, source: SizeSource) -> Self {
        entries.sort_by(|a, b| {
            b.bytes
                .cmp(&a.bytes)
                .then_with(|| a.key().cmp(&b.key()))
                .then_with(|| a.version.cmp(&b.version))
        });
        Sizes { entries, source }
    }

    /// The artifacts of one profile in a target directory, matched to the
    /// packages in `metadata` (`cargo metadata --format-version 1` output).
    pub fn from_target_dir(
        target: &Path,
        metadata: &str,
        options: &SizeOptions,
    ) -> Result<Self, DepsError> {
        let value: Value = serde_json::from_str(metadata)
            .map_err(|e| DepsError::new(format!("not cargo metadata JSON: {e}")))?;
        let packages = read_packages(&value)?;
        let (dir, profile) = profile_dir(target, options.profile.as_deref())?;
        let deps = dir.join("deps");
        let entries = std::fs::read_dir(&deps)
            .map_err(|e| DepsError::new(format!("cannot read {}: {e}", shown(&deps))))?;
        let mut found = Vec::new();
        let mut dep_info: HashMap<(String, String), PathBuf> = HashMap::new();
        for (count, entry) in entries.enumerate() {
            if count >= MAX_ENTRIES {
                return Err(DepsError::new(format!(
                    "{} holds more than {MAX_ENTRIES} files",
                    shown(&deps)
                )));
            }
            let Ok(entry) = entry else { continue };
            let Ok(meta) = entry.metadata() else { continue };
            let file_name = entry.file_name();
            let Some(file) = file_name.to_str() else {
                continue;
            };
            if !meta.is_file() {
                continue;
            }
            if let Some(stem) = file.strip_suffix(".d") {
                if let Some((name, hash)) = split_hash(stem) {
                    dep_info.insert((name.to_string(), hash.to_string()), entry.path());
                }
                continue;
            }
            if let Some((crate_name, hash, ext)) = split_artifact(file) {
                found.push(Found {
                    crate_name: crate_name.to_string(),
                    hash: hash.to_string(),
                    ext,
                    bytes: meta.len(),
                    modified: meta.modified().ok(),
                    file: file.to_string(),
                });
            }
        }

        let mut by_crate: HashMap<&str, Vec<usize>> = HashMap::new();
        let mut others: HashSet<&str> = HashSet::new();
        for (index, package) in packages.iter().enumerate() {
            if let Some(lib) = &package.lib {
                by_crate.entry(lib.as_str()).or_default().push(index);
            }
            others.extend(package.others.iter().map(String::as_str));
        }
        // The deepest directory first, so a package inside another's
        // directory claims its own sources.
        let mut dirs: Vec<(&Path, usize)> = packages
            .iter()
            .enumerate()
            .filter_map(|(index, package)| Some((package.dir.as_deref()?, index)))
            .collect();
        dirs.sort_by_key(|(dir, _)| std::cmp::Reverse(dir.components().count()));
        let from_dep_info = |name: &str, hash: &str| -> Option<usize> {
            let path = dep_info.get(&(name.to_string(), hash.to_string()))?;
            dep_info_sources(path).iter().find_map(|source| {
                dirs.iter()
                    .find(|(dir, _)| source.starts_with(dir))
                    .map(|(_, index)| *index)
            })
        };

        // The dep-info file says whose sources an artifact was built from;
        // without one, the crate name says, unless several packages share it.
        let claim = |name: &str, hash: &str| -> Claim {
            if let Some(index) = from_dep_info(name, hash) {
                return if packages[index].lib.as_deref() == Some(name) {
                    Claim::Library(Owner::Package(index))
                } else {
                    Claim::Other
                };
            }
            match by_crate.get(name).map_or(&[][..], Vec::as_slice) {
                [one] => Claim::Library(Owner::Package(*one)),
                [] if others.contains(name) => Claim::Other,
                [] => Claim::Unmatched,
                _ => Claim::Library(Owner::Crate(name.to_string())),
            }
        };
        let mut resolved: HashMap<(String, String), Claim> = HashMap::new();
        let mut unmatched: HashSet<String> = HashSet::new();
        let mut newest: BTreeMap<(Owner, Ext), Found> = BTreeMap::new();
        for artifact in found {
            let key = (artifact.crate_name.clone(), artifact.hash.clone());
            let owner = match resolved
                .entry(key)
                .or_insert_with(|| claim(&artifact.crate_name, &artifact.hash))
            {
                Claim::Library(owner) => owner.clone(),
                Claim::Other => continue,
                Claim::Unmatched => {
                    unmatched.insert(artifact.crate_name.clone());
                    continue;
                }
            };
            let slot = (owner, artifact.ext);
            let newer = newest
                .get(&slot)
                .is_none_or(|kept| artifact.modified > kept.modified);
            if newer {
                newest.insert(slot, artifact);
            }
        }

        let mut entries = Vec::new();
        let mut sources: HashMap<usize, Option<u64>> = HashMap::new();
        let mut measure = |index: usize| -> Option<u64> {
            if !options.sources {
                return None;
            }
            *sources
                .entry(index)
                .or_insert_with(|| packages[index].dir.as_deref().and_then(source_size))
        };
        let owners: Vec<Owner> = {
            let mut owners: Vec<Owner> = newest.keys().map(|(owner, _)| owner.clone()).collect();
            owners.dedup();
            owners
        };
        for owner in owners {
            let package = match &owner {
                Owner::Package(index) => Some(*index),
                Owner::Crate(_) => None,
            };
            let mut push = |found: &Found, kind: ArtifactKind, sources: Option<u64>| {
                let mut entry = SizeEntry::new(found.crate_name.clone(), kind, found.bytes);
                entry.file = Some(found.file.clone());
                if let Some(index) = package {
                    let meta = &packages[index];
                    entry.name = meta.name.clone();
                    entry.version = Some(meta.version.clone());
                    entry.id = Some(meta.id.clone());
                    entry.source_bytes = sources;
                }
                entries.push(entry);
            };
            let sources = package.and_then(&mut measure);
            let lib = newest
                .get(&(owner.clone(), Ext::Rlib))
                .map(|found| (found, ArtifactKind::Lib))
                .or_else(|| {
                    newest
                        .get(&(owner.clone(), Ext::Rmeta))
                        .map(|found| (found, ArtifactKind::Rmeta))
                });
            if let Some((found, kind)) = lib {
                push(found, kind, sources);
            }
            if let Some(found) = newest.get(&(owner.clone(), Ext::Dylib)) {
                let proc_macro = package.and_then(|index| packages[index].proc_macro);
                let kind = if proc_macro == Some(false) {
                    ArtifactKind::Dylib
                } else {
                    ArtifactKind::ProcMacro
                };
                push(found, kind, if lib.is_none() { sources } else { None });
            }
            if let Some(found) = newest.get(&(owner.clone(), Ext::Staticlib)) {
                push(found, ArtifactKind::Staticlib, None);
            }
        }

        // Binaries sit in the profile directory, named after their target.
        let mut bins: HashMap<&str, usize> = HashMap::new();
        for (index, package) in packages.iter().enumerate() {
            for bin in &package.bins {
                bins.entry(bin.as_str()).or_insert(index);
            }
        }
        if !bins.is_empty() {
            let top = std::fs::read_dir(&dir)
                .map_err(|e| DepsError::new(format!("cannot read {}: {e}", shown(&dir))))?;
            for (count, entry) in top.enumerate() {
                if count >= MAX_ENTRIES {
                    break;
                }
                let Ok(entry) = entry else { continue };
                let Ok(meta) = entry.metadata() else { continue };
                let file_name = entry.file_name();
                let Some(file) = file_name.to_str() else {
                    continue;
                };
                let name = file.strip_suffix(".exe").unwrap_or(file);
                let Some(&index) = bins.get(name).filter(|_| meta.is_file()) else {
                    continue;
                };
                let package = &packages[index];
                let mut entry = SizeEntry::new(name, ArtifactKind::Bin, meta.len());
                entry.version = Some(package.version.clone());
                entry.id = Some(package.id.clone());
                entry.file = Some(file.to_string());
                entry.source_bytes = measure(index);
                entries.push(entry);
            }
        }
        check_count(entries.len(), "artifacts")?;
        Ok(Sizes::with_source(
            entries,
            SizeSource::TargetDir {
                dir: dir.display().to_string(),
                profile,
                unmatched: unmatched.len(),
            },
        ))
    }

    /// cargo-bloat's JSON, or a report [`Sizes::to_json`] saved. See the
    /// [module docs](self).
    pub fn parse(input: &str) -> Result<Self, DepsError> {
        check_size(input, "the size report")?;
        let trimmed = input.trim_start_matches('\u{feff}').trim();
        let value: Value = match serde_json::from_str(trimmed) {
            Ok(value) => value,
            // Build output captured with the JSON (`2>&1`): its one line.
            Err(error) => trimmed
                .lines()
                .map(str::trim)
                .find(|line| line.starts_with("{\"file-size\""))
                .and_then(|line| serde_json::from_str(line).ok())
                .ok_or_else(|| {
                    DepsError::new(format!(
                        "not cargo-bloat JSON or a saved size report: {error}"
                    ))
                })?,
        };
        match saved_report(&value) {
            Some("sizes") => Self::from_saved(&value),
            Some(other) => Err(DepsError::new(format!(
                "a saved {} report, not sizes",
                clean(other)
            ))),
            None => Self::from_cargo_bloat(&value),
        }
    }

    /// `cargo bloat --crates --message-format json` output (or the
    /// per-function listing, added up by crate), parsed.
    pub fn from_cargo_bloat(value: &Value) -> Result<Self, DepsError> {
        let not_bloat = || {
            DepsError::new(
                "not cargo-bloat JSON: no `crates` or `functions` \
                 (expected `cargo bloat --crates --message-format json` output)",
            )
        };
        if !value.is_object() {
            return Err(not_bloat());
        }
        let mut sizes: Vec<(String, u64)> = Vec::new();
        let mut index: HashMap<String, usize> = HashMap::new();
        let mut add = |name: &str, size: u64| match index.get(name) {
            Some(&at) => sizes[at].1 = sizes[at].1.saturating_add(size),
            None => {
                index.insert(name.to_string(), sizes.len());
                sizes.push((name.to_string(), size));
            }
        };
        if let Some(crates) = value.get("crates").and_then(Value::as_array) {
            check_count(crates.len(), "crates")?;
            for item in crates {
                let name = item.get("name").and_then(Value::as_str);
                let size = item.get("size").and_then(Value::as_u64);
                let (Some(name), Some(size)) = (name, size) else {
                    return Err(DepsError::new(
                        "cargo-bloat JSON: a crate has no name or size",
                    ));
                };
                add(name, size);
            }
        } else if let Some(functions) = value.get("functions").and_then(Value::as_array) {
            check_count(functions.len(), "functions")?;
            for item in functions {
                let Some(size) = item.get("size").and_then(Value::as_u64) else {
                    return Err(DepsError::new("cargo-bloat JSON: a function has no size"));
                };
                let name = item
                    .get("crate")
                    .and_then(Value::as_str)
                    .unwrap_or("[Unknown]");
                add(name, size);
            }
        } else {
            return Err(not_bloat());
        }
        let entries = sizes
            .into_iter()
            .map(|(name, size)| SizeEntry::new(name, ArtifactKind::Crate, size))
            .collect();
        Ok(Sizes::with_source(
            entries,
            SizeSource::Bloat {
                file_size: value.get("file-size").and_then(Value::as_u64),
                text_size: value.get("text-section-size").and_then(Value::as_u64),
            },
        ))
    }

    fn from_saved(value: &Value) -> Result<Self, DepsError> {
        let version = value.get("version").and_then(Value::as_u64).unwrap_or(0);
        if version != SAVED_VERSION {
            return Err(DepsError::new(format!(
                "a saved size report of version {version}; this reads version {SAVED_VERSION}"
            )));
        }
        let units = value
            .get("units")
            .and_then(Value::as_array)
            .ok_or_else(|| DepsError::new("a saved size report with no `units`"))?;
        check_count(units.len(), "units")?;
        let mut entries = Vec::with_capacity(units.len());
        for unit in units {
            let text = |key: &str| unit.get(key).and_then(Value::as_str).map(str::to_string);
            let (Some(name), Some(kind), Some(bytes)) = (
                text("name"),
                text("kind").and_then(|kind| ArtifactKind::from_name(&kind)),
                unit.get("bytes").and_then(Value::as_u64),
            ) else {
                return Err(DepsError::new(
                    "a saved size report: a unit has no name, kind or bytes",
                ));
            };
            entries.push(SizeEntry {
                name,
                version: text("version"),
                id: text("id"),
                kind,
                bytes,
                source_bytes: unit.get("source_bytes").and_then(Value::as_u64),
                file: text("file"),
            });
        }
        let source = value.get("source").unwrap_or(&Value::Null);
        let text = |key: &str| source.get(key).and_then(Value::as_str);
        let source = match text("kind") {
            Some("target") => SizeSource::TargetDir {
                dir: text("dir").unwrap_or_default().to_string(),
                profile: text("profile").unwrap_or_default().to_string(),
                unmatched: source
                    .get("unmatched")
                    .and_then(Value::as_u64)
                    .map_or(0, |n| usize::try_from(n).unwrap_or(usize::MAX)),
            },
            Some("cargo-bloat") => SizeSource::Bloat {
                file_size: source.get("file_size").and_then(Value::as_u64),
                text_size: source.get("text_size").and_then(Value::as_u64),
            },
            _ => SizeSource::Entries,
        };
        Ok(Sizes::with_source(entries, source))
    }

    /// The report as JSON, to keep (say, as a CI baseline) and read back
    /// with [`Sizes::parse`].
    pub fn to_json(&self) -> String {
        let source = match &self.source {
            SizeSource::TargetDir {
                dir,
                profile,
                unmatched,
            } => json!({"kind": "target", "dir": dir, "profile": profile, "unmatched": unmatched}),
            SizeSource::Bloat {
                file_size,
                text_size,
            } => json!({"kind": "cargo-bloat", "file_size": file_size, "text_size": text_size}),
            SizeSource::Entries => json!({"kind": "entries"}),
        };
        let units: Vec<Value> = self
            .entries
            .iter()
            .map(|entry| {
                json!({
                    "name": entry.name,
                    "version": entry.version,
                    "id": entry.id,
                    "kind": entry.kind.name(),
                    "bytes": entry.bytes,
                    "source_bytes": entry.source_bytes,
                    "file": entry.file,
                })
            })
            .collect();
        let value = json!({
            "format": SAVED_FORMAT,
            "report": "sizes",
            "version": SAVED_VERSION,
            "source": source,
            "units": units,
        });
        let mut out = serde_json::to_string_pretty(&value).unwrap_or_default();
        out.push('\n');
        out
    }

    /// Every entry, largest first.
    pub fn entries(&self) -> &[SizeEntry] {
        &self.entries
    }

    /// Where the sizes came from.
    pub fn source(&self) -> &SizeSource {
        &self.source
    }

    /// Keep only the entries `keep` accepts (say, the packages a
    /// [`DepGraph`](super::DepGraph) reaches).
    #[must_use]
    pub fn retain(mut self, keep: impl FnMut(&SizeEntry) -> bool) -> Self {
        self.entries.retain(keep);
        self
    }

    /// Every entry's bytes, added up.
    pub fn total(&self) -> u64 {
        self.entries
            .iter()
            .fold(0u64, |sum, entry| sum.saturating_add(entry.bytes))
    }

    /// The source bytes of the packages measured, each package once (a
    /// library and a binary share their package's), added up; `None` when
    /// none was measured.
    pub fn source_total(&self) -> Option<u64> {
        let mut seen = HashSet::new();
        let measured: Vec<u64> = self
            .entries
            .iter()
            .filter(|entry| entry.source_bytes.is_some())
            .filter(|entry| entry.id.as_ref().is_none_or(|id| seen.insert(id.clone())))
            .filter_map(|entry| entry.source_bytes)
            .collect();
        (!measured.is_empty()).then(|| measured.iter().fold(0u64, |a, b| a.saturating_add(*b)))
    }
}

fn plural(n: usize, word: &str) -> String {
    format!("{n} {word}{}", if n == 1 { "" } else { "s" })
}

/// The total, the largest entries as bars, and a table of them. See the
/// [module docs](self).
#[derive(Clone, Debug)]
pub struct SizeReport {
    sizes: Sizes,
    limit: usize,
}

impl SizeReport {
    pub fn new(sizes: Sizes) -> Self {
        SizeReport {
            sizes,
            limit: DEFAULT_LIMIT,
        }
    }

    /// Show the `limit` largest entries ([`DEFAULT_LIMIT`] by default); the
    /// rest are counted underneath.
    #[must_use]
    pub fn limit(mut self, limit: usize) -> Self {
        self.limit = limit.max(1);
        self
    }

    pub fn sizes(&self) -> &Sizes {
        &self.sizes
    }

    fn heading(&self, console: &Console) -> Text {
        let count = self.sizes.entries.len();
        let total = format::bytes_binary(self.sizes.total());
        let mut line = match &self.sizes.source {
            SizeSource::TargetDir { dir, .. } => {
                format!("{}: {total} in {}", plural(count, "artifact"), clean(dir))
            }
            SizeSource::Bloat {
                file_size,
                text_size,
            } => {
                let mut line = format!("{}: {total} of code", plural(count, "crate"));
                if let Some(file) = file_size {
                    line.push_str(&format!(" in a {} binary", format::bytes_binary(*file)));
                }
                if let Some(text) = text_size {
                    line.push_str(&format!(" ({} of .text)", format::bytes_binary(*text)));
                }
                line
            }
            SizeSource::Entries => format!("{}: {total}", plural(count, "unit")),
        };
        if let Some(source) = self.sizes.source_total() {
            line.push_str(&format!(", {} of source", format::bytes_binary(source)));
        }
        let mut text = Text::styled(line, theme_style(console, "deps.root"));
        if let SizeSource::TargetDir { unmatched, .. } = &self.sizes.source {
            if *unmatched > 0 {
                text.append(
                    &format!(
                        "\n{} in the directory matched no package (left from an older build?)",
                        plural(*unmatched, "crate")
                    ),
                    Some(theme_style(console, "deps.section").into()),
                );
            }
        }
        text
    }

    /// How an entry is labelled: its name, with the version for a crate
    /// at several and the kind for anything but a library.
    fn label(&self, entry: &SizeEntry) -> String {
        let several = self
            .sizes
            .entries
            .iter()
            .any(|other| other.name == entry.name && other.version != entry.version);
        let mut label = clean(&entry.name).into_owned();
        if let Some(version) = entry.version.as_deref().filter(|_| several) {
            label.push_str(&format!(" v{}", clean(version)));
        }
        if !entry.kind.by_crate() {
            label.push_str(&format!(" ({})", entry.kind.name()));
        }
        label
    }

    /// Bars labelled with the name and size, aligned.
    fn bars(&self, shown: &[&SizeEntry]) -> BarChart {
        let labels: Vec<(String, String)> = shown
            .iter()
            .map(|entry| (self.label(entry), format::bytes_binary(entry.bytes)))
            .collect();
        let name_width = labels.iter().map(|(l, _)| cell_len(l)).max().unwrap_or(0);
        let size_width = labels.iter().map(|(_, s)| cell_len(s)).max().unwrap_or(0);
        BarChart::from_pairs(labels.into_iter().zip(shown).map(|((label, size), entry)| {
            let gap = name_width - cell_len(&label) + 2 + size_width - cell_len(&size);
            (
                format!("{label}{}{size}", " ".repeat(gap)),
                entry.bytes as f64,
            )
        }))
        .show_values(false)
    }

    fn table(&self, shown: &[&SizeEntry], rest: &[&SizeEntry], console: &Console) -> Table {
        let entries = &self.sizes.entries;
        let versions = entries.iter().any(|e| e.version.is_some());
        let kinds = entries.iter().any(|e| e.kind != ArtifactKind::Crate);
        let sources = entries.iter().any(|e| e.source_bytes.is_some());
        let total = self.sizes.total();
        let mut table = Table::new();
        table.add_column("Crate");
        if versions {
            table.add_column("Version");
        }
        if kinds {
            table.add_column("Kind");
        }
        table.add_column_justify("Size", Justify::Right);
        table.add_column_justify("Share", Justify::Right);
        if sources {
            table.add_column_justify("Source", Justify::Right);
        }
        let none = || Text::styled("–", theme_style(console, "deps.off"));
        for entry in shown {
            let mut row = vec![Text::new(clean(&entry.name).into_owned())];
            if versions {
                row.push(entry.version.as_deref().map_or_else(none, |version| {
                    Text::styled(
                        format!("v{}", clean(version)),
                        theme_style(console, "deps.version"),
                    )
                }));
            }
            if kinds {
                row.push(Text::new(entry.kind.name()));
            }
            row.push(Text::new(format::bytes_binary(entry.bytes)));
            row.push(Text::new(if total == 0 {
                "–".to_string()
            } else {
                format::percent(entry.bytes as f64 / total as f64, 1)
            }));
            if sources {
                row.push(
                    entry
                        .source_bytes
                        .map_or_else(none, |bytes| Text::new(format::bytes_binary(bytes))),
                );
            }
            table.add_row_text(row);
        }
        if !rest.is_empty() {
            let bytes = rest
                .iter()
                .fold(0u64, |sum, entry| sum.saturating_add(entry.bytes));
            table = table
                .caption(format!(
                    "and {} more, {} in all",
                    rest.len(),
                    format::bytes_binary(bytes)
                ))
                .caption_justify(Justify::Left);
        }
        table
    }

    fn parts(&self, console: &Console) -> Vec<Box<dyn Renderable>> {
        let all: Vec<&SizeEntry> = self.sizes.entries.iter().collect();
        let (shown, rest) = all.split_at(self.limit.min(all.len()));
        let mut parts: Vec<Box<dyn Renderable>> = vec![Box::new(self.heading(console))];
        if !shown.is_empty() {
            parts.push(Box::new(self.bars(shown)));
            parts.push(Box::new(self.table(shown, rest, console)));
        }
        parts
    }
}

impl Renderable for SizeReport {
    fn rich_render(&self, console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
        let parts = self.parts(console);
        let refs: Vec<&dyn Renderable> = parts.iter().map(|part| part.as_ref()).collect();
        stack(&refs, console, options)
    }

    fn measure(&self, console: &Console, options: &ConsoleOptions) -> rich::measure::Measurement {
        let parts = self.parts(console);
        let refs: Vec<&dyn Renderable> = parts.iter().map(|part| part.as_ref()).collect();
        stack_measure(&refs, console, options)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn artifact_names_split() {
        assert_eq!(
            split_artifact("libsyn-0123abcd.rlib"),
            Some(("syn", "0123abcd", Ext::Rlib))
        );
        assert_eq!(
            split_artifact("libserde_derive-ff00.so"),
            Some(("serde_derive", "ff00", Ext::Dylib))
        );
        assert_eq!(
            split_artifact("serde_derive-ff00.dll"),
            Some(("serde_derive", "ff00", Ext::Dylib))
        );
        assert_eq!(
            split_artifact("liblibrary-ab.rlib"),
            Some(("library", "ab", Ext::Rlib))
        );
        for other in [
            "syn-0123.d",
            "libsyn.rlib",
            "libsyn-xyz.rlib",
            "foo-ab.dll.lib",
            "libfoo-ab.dll.a",
            "app-0123",
            "-ab.rlib",
        ] {
            assert_eq!(split_artifact(other), None, "{other}");
        }
    }

    #[test]
    fn profile_names_map_to_directories() {
        assert_eq!(profile_dir_name("dev"), "debug");
        assert_eq!(profile_dir_name("test"), "debug");
        assert_eq!(profile_dir_name("bench"), "release");
        assert_eq!(profile_dir_name("release"), "release");
        assert_eq!(profile_dir_name("dist"), "dist");
        let dir = std::env::temp_dir();
        assert!(profile_dir(&dir, Some("../x")).is_err());
        assert!(profile_dir(&dir, Some("")).is_err());
    }

    #[test]
    fn cargo_bloat_functions_add_up_by_crate() {
        let json = r#"{"file-size": 100, "text-section-size": 60, "functions": [
            {"crate": "std", "name": "a", "size": 10},
            {"crate": "std", "name": "b", "size": 15},
            {"name": "c", "size": 5}]}"#;
        let sizes = Sizes::parse(json).unwrap();
        let names: Vec<(&str, u64)> = sizes
            .entries()
            .iter()
            .map(|e| (e.name.as_str(), e.bytes))
            .collect();
        assert_eq!(names, [("std", 25), ("[Unknown]", 5)]);
    }

    #[test]
    fn malformed_bloat_is_an_error() {
        for bad in [
            "",
            "[]",
            "{}",
            "{\"crates\": [{\"name\": \"a\"}]}",
            "{\"crates\": [{\"size\": 1}]}",
            "{\"crates\": [{\"name\": \"a\", \"size\": -1}]}",
            "{\"crates\": [",
            "{\"format\": \"rich-deps\", \"report\": \"timings\", \"version\": 1}",
            "{\"format\": \"rich-deps\", \"report\": \"sizes\", \"version\": 9, \"units\": []}",
        ] {
            assert!(Sizes::parse(bad).is_err(), "{bad:?}");
        }
    }
}
