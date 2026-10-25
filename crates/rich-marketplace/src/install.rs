//! Installing a version: download, check, move into place, lock.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use crate::check::{ArtifactChecker, BuiltinChecks, WasmExpectation};
use crate::error::{InstallError, ListingError, LockfileError, ResolveError};
use crate::index::VerifiedIndex;
use crate::listing::{Kind, ListedVersion, Listing, Requires};
use crate::lockfile::{write_atomically, LockEntry, Lockfile, LOCKFILE_NAME};
use crate::resolve::Host;
use crate::transport::{require_https, Transport};
use crate::util::sha256_hex;

/// Installs packages into one directory: each at `<root>/<id>/<file>`, with
/// the lockfile at `<root>/`[`LOCKFILE_NAME`].
#[derive(Debug)]
pub struct Installer<T, C = BuiltinChecks> {
    root: PathBuf,
    transport: T,
    checker: C,
    host: Host,
}

impl<T: Transport> Installer<T, BuiltinChecks> {
    /// An installer into `root` that downloads with `transport` and checks
    /// with the built-in checks, for this host.
    pub fn new(root: impl Into<PathBuf>, transport: T) -> Self {
        Installer {
            root: root.into(),
            transport,
            checker: BuiltinChecks,
            host: Host::current(),
        }
    }
}

impl<T: Transport, C: ArtifactChecker> Installer<T, C> {
    /// Check artifacts with `checker` instead (the WASM sandbox, say).
    pub fn with_checker<D: ArtifactChecker>(self, checker: D) -> Installer<T, D> {
        Installer {
            root: self.root,
            transport: self.transport,
            checker,
            host: self.host,
        }
    }

    /// Resolve against `host` instead of this build.
    pub fn with_host(mut self, host: Host) -> Self {
        self.host = host;
        self
    }

    /// The install directory.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The lockfile's path.
    pub fn lockfile_path(&self) -> PathBuf {
        self.root.join(LOCKFILE_NAME)
    }

    /// Read the lockfile.
    pub fn lockfile(&self) -> Result<Lockfile, LockfileError> {
        Lockfile::load(&self.lockfile_path())
    }

    /// Install `version` of `listing`, read through `index`.
    ///
    /// Nothing is written until every check has passed: the kind installs,
    /// the index has not expired and names the listing, the version is not
    /// yanked and suits the host, the URL is HTTPS, the download is at most
    /// and exactly the declared size and has the listed SHA-256, and the
    /// artifact passes its check by kind. Then the file is moved into place
    /// atomically, the lockfile is written, and an older version's file is
    /// removed.
    pub fn install(
        &self,
        index: &VerifiedIndex,
        listing: &Listing,
        version: &ListedVersion,
        now: SystemTime,
    ) -> Result<LockEntry, InstallError> {
        if !listing.kind.is_installable() {
            return Err(InstallError::NotInstallable {
                id: listing.id.clone(),
                kind: listing.kind.as_str(),
            });
        }
        index.check_fresh(now)?;
        if index.entry(&listing.id).is_none() {
            return Err(ListingError::NotInIndex(listing.id.clone()).into());
        }
        if let Some(reason) = &version.yanked {
            return Err(ResolveError::Yanked {
                id: listing.id.clone(),
                version: version.version.to_string(),
                reason: reason.clone(),
            }
            .into());
        }
        self.host
            .check(listing.kind, version)
            .map_err(|reason| ResolveError::Incompatible {
                id: listing.id.clone(),
                version: version.version.to_string(),
                reason,
            })?;

        let artifact = &version.artifact;
        require_https(&artifact.url)?;
        let bytes = self.transport.get(&artifact.url, artifact.size)?;
        if bytes.len() as u64 != artifact.size {
            return Err(InstallError::SizeMismatch {
                expected: artifact.size,
                actual: bytes.len() as u64,
            });
        }
        let actual = sha256_hex(&bytes);
        if actual != artifact.sha256 {
            return Err(InstallError::HashMismatch {
                expected: artifact.sha256.clone(),
                actual,
            });
        }
        let number = version.version.to_string();
        match (listing.kind, version.requires) {
            (Kind::WasmPlugin, Requires::Abi { major, minor }) => {
                let expected = WasmExpectation {
                    id: &listing.id,
                    version: &number,
                    abi: (major, minor),
                    capabilities: &version.capabilities,
                };
                self.checker.check_wasm(&bytes, &expected)?;
            }
            (Kind::MicroPack, _) => self
                .checker
                .check_micro_pack(&bytes, &listing.id, &number)?,
            (Kind::Theme, _) => self.checker.check_theme(&bytes)?,
            _ => {}
        }

        let relative = format!(
            "{}/{}-{}.{}",
            listing.id,
            listing.id,
            number,
            extension(&artifact.url)
        );
        // Read the lockfile first, so a broken one stops the install
        // before anything is written.
        let mut lockfile = self.lockfile()?;
        let destination = self.root.join(&relative);
        write_atomically(&destination, &bytes).map_err(|e| LockfileError::Io {
            path: destination.display().to_string(),
            message: e.to_string(),
        })?;
        let entry = LockEntry {
            id: listing.id.clone(),
            kind: listing.kind,
            version: number,
            tier: listing.tier,
            url: artifact.url.clone(),
            sha256: artifact.sha256.clone(),
            size: artifact.size,
            index_serial: index.serial(),
            path: relative,
        };
        let previous = lockfile.insert(entry.clone());
        lockfile.save(&self.lockfile_path())?;
        if let Some(previous) = previous {
            if previous.path != entry.path {
                let _ = fs::remove_file(previous.file(&self.root));
            }
        }
        Ok(entry)
    }

    /// Remove an installed package: its lockfile entry, then its file.
    pub fn remove(&self, id: &str) -> Result<LockEntry, LockfileError> {
        let mut lockfile = self.lockfile()?;
        let entry = lockfile
            .remove(id)
            .ok_or_else(|| LockfileError::NotInstalled(id.to_string()))?;
        lockfile.save(&self.lockfile_path())?;
        let _ = fs::remove_file(entry.file(&self.root));
        let _ = fs::remove_dir(self.root.join(id));
        Ok(entry)
    }

    /// Read package `id`'s file, checked against the lockfile: what a host
    /// does on every load. No network, and no index needed.
    pub fn load(&self, id: &str) -> Result<Vec<u8>, LockfileError> {
        self.lockfile()?.verify(&self.root, id)
    }
}

/// The artifact URL's file extension, which the schema fixes per kind.
fn extension(url: &str) -> &str {
    let path = url.split(['?', '#']).next().unwrap_or(url);
    path.rsplit_once('.')
        .map(|(_, ext)| ext)
        .filter(|ext| !ext.is_empty() && ext.bytes().all(|b| b.is_ascii_alphanumeric()))
        .unwrap_or("bin")
}
