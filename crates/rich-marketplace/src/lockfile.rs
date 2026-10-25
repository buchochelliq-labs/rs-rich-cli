//! The lockfile: what is installed, from where, and what it hashes to.
//!
//! It is written only after a verified install, so loading checks each file
//! against it and nothing else: no network, and an expired or unverifiable
//! index never stops an installed package from loading.
//!
//! ```json
//! {
//!   "schema_version": 1,
//!   "packages": {
//!     "shout": {
//!       "kind": "wasm-plugin", "version": "0.2.0", "tier": "community",
//!       "url": "https://…/shout.wasm", "sha256": "…", "size": 2048,
//!       "index_serial": 42, "path": "shout/shout-0.2.0.wasm"
//!     }
//!   }
//! }
//! ```

use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::{Component, Path, PathBuf};

use serde_json::{json, Map, Value};

use crate::error::LockfileError;
use crate::listing::{Kind, Tier};
use crate::util::{is_lower_hex, parse_json, sha256_hex};

/// The only lockfile format this release reads.
pub const LOCKFILE_SCHEMA_VERSION: u64 = 1;

/// The lockfile's name in the install directory.
pub const LOCKFILE_NAME: &str = "marketplace.lock.json";

/// One installed package.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct LockEntry {
    pub id: String,
    pub kind: Kind,
    pub version: String,
    pub tier: Tier,
    pub url: String,
    pub sha256: String,
    pub size: u64,
    /// The serial of the index it was installed from.
    pub index_serial: u64,
    /// The file, relative to the install directory (`<id>/<file>`).
    pub path: String,
}

impl LockEntry {
    /// The installed file's absolute path under `root`.
    pub fn file(&self, root: &Path) -> PathBuf {
        root.join(&self.path)
    }

    /// Read the installed file and check it against this entry. The bytes
    /// returned are the ones checked: load those, not the file again.
    pub fn verify(&self, root: &Path) -> Result<Vec<u8>, LockfileError> {
        let path = self.file(root);
        let bytes = match fs::read(&path) {
            Ok(bytes) => bytes,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Err(LockfileError::Missing {
                    id: self.id.clone(),
                    path: path.display().to_string(),
                })
            }
            Err(e) => return Err(io_error(&path, e)),
        };
        let actual = sha256_hex(&bytes);
        if actual != self.sha256 || bytes.len() as u64 != self.size {
            return Err(LockfileError::Mismatch {
                id: self.id.clone(),
                expected: self.sha256.clone(),
                actual,
            });
        }
        Ok(bytes)
    }
}

/// Every installed package, by id.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Lockfile {
    entries: BTreeMap<String, LockEntry>,
}

fn io_error(path: &Path, error: std::io::Error) -> LockfileError {
    LockfileError::Io {
        path: path.display().to_string(),
        message: error.to_string(),
    }
}

/// `<id>/<file>`: two plain components, the first the id.
fn safe_path(id: &str, path: &str) -> bool {
    let components: Vec<_> = Path::new(path).components().collect();
    components.len() == 2
        && components[0] == Component::Normal(id.as_ref())
        && matches!(components[1], Component::Normal(_))
        && !path.contains('\\')
}

impl Lockfile {
    /// An empty lockfile.
    pub fn new() -> Self {
        Lockfile::default()
    }

    /// Read the lockfile at `path`; a missing file is an empty lockfile.
    pub fn load(path: &Path) -> Result<Self, LockfileError> {
        match fs::read(path) {
            Ok(bytes) => Lockfile::parse(&bytes),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Lockfile::new()),
            Err(e) => Err(io_error(path, e)),
        }
    }

    /// Read a lockfile's bytes.
    pub fn parse(bytes: &[u8]) -> Result<Self, LockfileError> {
        let invalid = |message: String| LockfileError::Invalid(message);
        let value = parse_json(bytes).map_err(invalid)?;
        match value.get("schema_version").and_then(Value::as_u64) {
            Some(LOCKFILE_SCHEMA_VERSION) => {}
            Some(other) => return Err(LockfileError::UnsupportedSchema(other)),
            None => return Err(invalid("schema_version: missing".into())),
        }
        let packages = value
            .get("packages")
            .and_then(Value::as_object)
            .ok_or_else(|| invalid("packages: missing or not an object".into()))?;
        let mut entries = BTreeMap::new();
        for (id, entry) in packages {
            let text = |key: &str| {
                entry
                    .get(key)
                    .and_then(Value::as_str)
                    .ok_or_else(|| invalid(format!("packages.{id}.{key}: missing or not a string")))
            };
            let number = |key: &str| {
                entry.get(key).and_then(Value::as_u64).ok_or_else(|| {
                    invalid(format!("packages.{id}.{key}: missing or not an integer"))
                })
            };
            if !rich_plugin_api::is_valid_name(id) {
                return Err(invalid(format!("packages.{id}: not a valid package id")));
            }
            let kind = Kind::parse(text("kind")?)
                .ok_or_else(|| invalid(format!("packages.{id}.kind: unknown")))?;
            let tier = Tier::parse(text("tier")?)
                .ok_or_else(|| invalid(format!("packages.{id}.tier: unknown")))?;
            let sha256 = text("sha256")?;
            if !is_lower_hex(sha256, 64) {
                return Err(invalid(format!("packages.{id}.sha256: not a SHA-256")));
            }
            let path = text("path")?;
            if !safe_path(id, path) {
                return Err(invalid(format!("packages.{id}.path: must be {id}/<file>")));
            }
            entries.insert(
                id.clone(),
                LockEntry {
                    id: id.clone(),
                    kind,
                    version: text("version")?.to_string(),
                    tier,
                    url: text("url")?.to_string(),
                    sha256: sha256.to_string(),
                    size: number("size")?,
                    index_serial: number("index_serial")?,
                    path: path.to_string(),
                },
            );
        }
        Ok(Lockfile { entries })
    }

    /// The lockfile as JSON.
    pub fn to_json(&self) -> String {
        let mut packages = Map::new();
        for (id, e) in &self.entries {
            packages.insert(
                id.clone(),
                json!({
                    "kind": e.kind.as_str(),
                    "version": e.version,
                    "tier": e.tier.as_str(),
                    "url": e.url,
                    "sha256": e.sha256,
                    "size": e.size,
                    "index_serial": e.index_serial,
                    "path": e.path,
                }),
            );
        }
        let value = json!({"schema_version": LOCKFILE_SCHEMA_VERSION, "packages": packages});
        let mut text = serde_json::to_string_pretty(&value).unwrap_or_default();
        text.push('\n');
        text
    }

    /// Write the lockfile to `path` atomically (a temporary file beside it,
    /// then a rename).
    pub fn save(&self, path: &Path) -> Result<(), LockfileError> {
        write_atomically(path, self.to_json().as_bytes()).map_err(|e| io_error(path, e))
    }

    /// One package.
    pub fn get(&self, id: &str) -> Option<&LockEntry> {
        self.entries.get(id)
    }

    /// Every package, by id.
    pub fn entries(&self) -> impl Iterator<Item = &LockEntry> {
        self.entries.values()
    }

    /// Record a package, replacing any earlier entry for its id.
    pub fn insert(&mut self, entry: LockEntry) -> Option<LockEntry> {
        self.entries.insert(entry.id.clone(), entry)
    }

    /// Forget a package.
    pub fn remove(&mut self, id: &str) -> Option<LockEntry> {
        self.entries.remove(id)
    }

    /// Read package `id`'s installed file under `root`, checked against its
    /// entry: what a host does on every load.
    pub fn verify(&self, root: &Path, id: &str) -> Result<Vec<u8>, LockfileError> {
        self.get(id)
            .ok_or_else(|| LockfileError::NotInstalled(id.to_string()))?
            .verify(root)
    }

    /// Check every installed file under `root`.
    pub fn verify_all(&self, root: &Path) -> Vec<(String, Result<(), LockfileError>)> {
        self.entries()
            .map(|e| (e.id.clone(), e.verify(root).map(|_| ())))
            .collect()
    }
}

/// Write `bytes` to `path` through a temporary file in the same directory,
/// synced, then renamed over it.
pub(crate) fn write_atomically(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let dir = path.parent().unwrap_or(Path::new("."));
    fs::create_dir_all(dir)?;
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let temporary = dir.join(format!(".{name}.{}.partial", std::process::id()));
    let result = (|| {
        let mut file = fs::File::create(&temporary)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        fs::rename(&temporary, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_stay_inside_their_package_folder() {
        assert!(safe_path("shout", "shout/shout-0.2.0.wasm"));
        assert!(!safe_path("shout", "other/shout-0.2.0.wasm"));
        assert!(!safe_path("shout", "shout/../x"));
        assert!(!safe_path("shout", "/etc/passwd"));
        assert!(!safe_path("shout", "shout/a/b"));
        assert!(!safe_path("shout", "shout\\..\\x"));
    }

    #[test]
    fn round_trip() {
        let mut lock = Lockfile::new();
        lock.insert(LockEntry {
            id: "shout".into(),
            kind: Kind::WasmPlugin,
            version: "0.2.0".into(),
            tier: Tier::Community,
            url: "https://example.test/shout.wasm".into(),
            sha256: "0".repeat(64),
            size: 8,
            index_serial: 7,
            path: "shout/shout-0.2.0.wasm".into(),
        });
        assert_eq!(Lockfile::parse(lock.to_json().as_bytes()).unwrap(), lock);
        let v2 = lock
            .to_json()
            .replace("\"schema_version\": 1", "\"schema_version\": 2");
        assert_eq!(
            Lockfile::parse(v2.as_bytes()),
            Err(LockfileError::UnsupportedSchema(2))
        );
    }
}
