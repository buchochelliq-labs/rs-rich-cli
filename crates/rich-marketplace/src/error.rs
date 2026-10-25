//! The crate's errors: one enum per step, each `#[non_exhaustive]` so a
//! later release can add a reason without a breaking change.

use std::fmt;

use crate::keys::KeyId;

/// Why a minisign signature was refused.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum SignatureError {
    /// Not a minisign signature file.
    Malformed,
    /// The legacy unhashed algorithm (`Ed`), which this client refuses.
    LegacyAlgorithm,
    /// An algorithm minisign does not define.
    UnsupportedAlgorithm,
    /// Made by a key other than the one trusted for it: refused before the
    /// signature is checked.
    WrongKeyId { expected: KeyId, found: KeyId },
    /// The signature does not verify.
    Invalid,
    /// The trusted comment (which the signature covers) is not the one this
    /// document needs, so the signature belongs to another document.
    TrustedComment { expected: String, found: String },
}

impl fmt::Display for SignatureError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SignatureError::Malformed => f.write_str("not a minisign signature"),
            SignatureError::LegacyAlgorithm => f.write_str(
                "the signature uses minisign's legacy unhashed algorithm, which is refused",
            ),
            SignatureError::UnsupportedAlgorithm => f.write_str("an unknown signature algorithm"),
            SignatureError::WrongKeyId { expected, found } => {
                write!(
                    f,
                    "signed by key {found}, but only key {expected} is trusted for it"
                )
            }
            SignatureError::Invalid => f.write_str("the signature does not verify"),
            SignatureError::TrustedComment { expected, found } => {
                write!(f, "the trusted comment is {found:?}, expected {expected:?}")
            }
        }
    }
}

impl std::error::Error for SignatureError {}

/// Why a key or a generation of the key chain was refused.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum KeyChainError {
    /// This release has no compiled-in keys yet.
    NoTrustRoot,
    /// Not a minisign public key.
    BadKey(String),
    /// A `keys-N.json` document is malformed.
    BadKeysFile(String),
    /// A `keys-N.json` format this release does not read.
    UnsupportedSchema(u64),
    /// A rotation's signature failed.
    Signature {
        generation: u64,
        /// `main` or `backup`.
        which: &'static str,
        source: SignatureError,
    },
    /// A generation out of order.
    GenerationOrder { expected: u64, found: u64 },
    /// A generation signed by the backup key alone that does not revoke and
    /// replace the main key.
    MainSignatureRequired { generation: u64 },
    /// A generation signed by the backup key alone that changes the backup
    /// key: that needs the main key's signature too.
    BackupReplacedWithoutMain { generation: u64 },
    /// A generation that names a revoked key as one of its own.
    RevokedKey(KeyId),
}

impl fmt::Display for KeyChainError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            KeyChainError::NoTrustRoot => f.write_str(
                "this release has no marketplace keys compiled in; it can only verify with explicit keys",
            ),
            KeyChainError::BadKey(message) | KeyChainError::BadKeysFile(message) => {
                f.write_str(message)
            }
            KeyChainError::UnsupportedSchema(version) => write!(
                f,
                "keys file schema_version {version} is not supported; update rs-rich"
            ),
            KeyChainError::Signature {
                generation,
                which,
                source,
            } => write!(f, "keys generation {generation}: the {which} signature: {source}"),
            KeyChainError::GenerationOrder { expected, found } => {
                write!(f, "expected keys generation {expected}, found {found}")
            }
            KeyChainError::MainSignatureRequired { generation } => write!(
                f,
                "keys generation {generation} has no main-key signature and does not replace a revoked main key"
            ),
            KeyChainError::BackupReplacedWithoutMain { generation } => write!(
                f,
                "keys generation {generation} replaces the backup key without the main key's signature"
            ),
            KeyChainError::RevokedKey(id) => write!(f, "key {id} is revoked"),
        }
    }
}

impl std::error::Error for KeyChainError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            KeyChainError::Signature { source, .. } => Some(source),
            _ => None,
        }
    }
}

/// Why an index was refused.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum IndexError {
    /// `index.json.minisig` failed.
    Signature(SignatureError),
    /// The trusted comment is not `rs-rich-marketplace index serial=N`.
    TrustedComment(String),
    /// `index.json` is not a valid index.
    Invalid(String),
    /// An index format this release does not read.
    UnsupportedSchema(u64),
    /// The signature names one serial and the index another.
    SerialMismatch { signed: u64, index: u64 },
    /// Older than the last index this client accepted.
    Rollback { serial: u64, last: u64 },
    /// The same serial as the last accepted index, but different bytes.
    SerialReused { serial: u64 },
    /// Past its `expires`.
    Expired { expires: String },
}

impl fmt::Display for IndexError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            IndexError::Signature(source) => write!(f, "index.json.minisig: {source}"),
            IndexError::TrustedComment(found) => {
                write!(f, "index.json.minisig: unexpected trusted comment {found:?}")
            }
            IndexError::Invalid(message) => write!(f, "index.json: {message}"),
            IndexError::UnsupportedSchema(version) => write!(
                f,
                "index.json: schema_version {version} is not supported; update rs-rich"
            ),
            IndexError::SerialMismatch { signed, index } => write!(
                f,
                "index.json has serial {index}, but its signature is for serial {signed}"
            ),
            IndexError::Rollback { serial, last } => write!(
                f,
                "index.json has serial {serial}, older than serial {last} already accepted (a rollback)"
            ),
            IndexError::SerialReused { serial } => write!(
                f,
                "index.json has serial {serial} again, with different contents"
            ),
            IndexError::Expired { expires } => {
                write!(f, "index.json expired at {expires}; refresh the marketplace")
            }
        }
    }
}

impl std::error::Error for IndexError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            IndexError::Signature(source) => Some(source),
            _ => None,
        }
    }
}

/// Why a listing was refused.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum ListingError {
    /// Not strict JSON.
    Json(String),
    /// A listing format this release does not read.
    UnsupportedSchema(u64),
    /// A kind this release does not know.
    UnknownKind(String),
    /// Breaks the schema or a rule beyond it; `at` is the JSON path.
    Invalid { at: String, message: String },
    /// The index has no listing with this id.
    NotInIndex(String),
    /// The file's SHA-256 is not the one the index records.
    HashMismatch {
        id: String,
        expected: String,
        actual: String,
    },
    /// The listing's `id` is not the key the index files it under.
    IdMismatch { filed: String, found: String },
}

impl fmt::Display for ListingError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ListingError::Json(message) => write!(f, "listing: {message}"),
            ListingError::UnsupportedSchema(version) => write!(
                f,
                "listing schema_version {version} is not supported; update rs-rich"
            ),
            ListingError::UnknownKind(kind) => {
                write!(
                    f,
                    "listing kind {kind:?} is not known to this release; update rs-rich"
                )
            }
            ListingError::Invalid { at, message } => write!(f, "listing {at}: {message}"),
            ListingError::NotInIndex(id) => write!(f, "no listing {id:?} in the index"),
            ListingError::HashMismatch {
                id,
                expected,
                actual,
            } => write!(
                f,
                "listing {id}: SHA-256 is {actual}, but the index records {expected}"
            ),
            ListingError::IdMismatch { filed, found } => {
                write!(f, "listing filed as {filed:?} has id {found:?}")
            }
        }
    }
}

impl std::error::Error for ListingError {}

/// Why a host cannot use a version.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Incompatibility {
    /// A WASM plugin built for another ABI major.
    Abi {
        required: (u32, u32),
        host: (u32, u32),
    },
    /// A WASM capability kind this host does not know (a newer ABI minor).
    UnknownCapability { kind: String },
    /// A crate built against another plugin API.
    PluginApi { required: u64, host: u32 },
    /// A micro pack of another pack schema.
    MicroSchema { required: u64, host: u64 },
    /// A theme of another theme format.
    ThemeFormat { required: u64, host: u64 },
}

impl Incompatibility {
    /// Whether a newer host would load it (rather than an older one).
    pub fn needs_newer_host(&self) -> bool {
        match self {
            Incompatibility::Abi { required, host } => required.0 > host.0,
            Incompatibility::UnknownCapability { .. } => true,
            Incompatibility::PluginApi { required, host } => *required > u64::from(*host),
            Incompatibility::MicroSchema { required, host }
            | Incompatibility::ThemeFormat { required, host } => required > host,
        }
    }
}

impl fmt::Display for Incompatibility {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Incompatibility::Abi { required, host } => write!(
                f,
                "needs plugin ABI {}.{}, this host loads ABI {}.x",
                required.0, required.1, host.0
            ),
            Incompatibility::UnknownCapability { kind } => {
                write!(
                    f,
                    "declares capability kind {kind:?}, which this host does not know"
                )
            }
            Incompatibility::PluginApi { required, host } => {
                write!(f, "needs plugin API {required}, this host has {host}")
            }
            Incompatibility::MicroSchema { required, host } => {
                write!(
                    f,
                    "needs micro pack schema {required}, this host reads {host}"
                )
            }
            Incompatibility::ThemeFormat { required, host } => {
                write!(f, "needs theme format {required}, this host reads {host}")
            }
        }
    }
}

impl std::error::Error for Incompatibility {}

/// Why no version was chosen.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum ResolveError {
    /// The exact version asked for is not listed.
    NoSuchVersion { id: String, version: String },
    /// The exact version asked for is yanked.
    Yanked {
        id: String,
        version: String,
        reason: String,
    },
    /// The exact version asked for does not suit this host.
    Incompatible {
        id: String,
        version: String,
        reason: Incompatibility,
    },
    /// No version suits this host. `newer_host_needed` names the newest one
    /// skipped because it needs a newer host, if any.
    NoCandidate {
        id: String,
        newer_host_needed: Option<String>,
    },
}

impl fmt::Display for ResolveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ResolveError::NoSuchVersion { id, version } => {
                write!(f, "{id} has no version {version}")
            }
            ResolveError::Yanked {
                id,
                version,
                reason,
            } => write!(f, "{id} {version} is yanked: {reason}"),
            ResolveError::Incompatible {
                id,
                version,
                reason,
            } => write!(f, "{id} {version} {reason}"),
            ResolveError::NoCandidate {
                id,
                newer_host_needed: Some(version),
            } => write!(
                f,
                "no version of {id} suits this host; {version} and newer need a newer rs-rich"
            ),
            ResolveError::NoCandidate { id, .. } => {
                write!(f, "no version of {id} suits this host")
            }
        }
    }
}

impl std::error::Error for ResolveError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            ResolveError::Incompatible { reason, .. } => Some(reason),
            _ => None,
        }
    }
}

/// Why a download failed.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum FetchError {
    /// Not an `https://` URL (or a redirect away from HTTPS).
    NotHttps { url: String },
    /// Longer than the most it may be.
    TooLarge { url: String, limit: u64 },
    /// The server answered with an error status.
    Status { url: String, status: u16 },
    /// Nothing at that URL (an in-memory transport).
    NotFound { url: String },
    /// The connection or read failed.
    Transport { url: String, message: String },
}

impl fmt::Display for FetchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FetchError::NotHttps { url } => write!(f, "{url}: only https:// downloads are allowed"),
            FetchError::TooLarge { url, limit } => {
                write!(f, "{url}: larger than the {limit} bytes declared")
            }
            FetchError::Status { url, status } => write!(f, "{url}: HTTP status {status}"),
            FetchError::NotFound { url } => write!(f, "{url}: not found"),
            FetchError::Transport { url, message } => write!(f, "{url}: {message}"),
        }
    }
}

impl std::error::Error for FetchError {}

/// Why an artifact failed its check by kind.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum CheckError {
    /// Not a WebAssembly module.
    NotWasm,
    /// The module's manifest differs from the listing.
    ManifestMismatch {
        field: &'static str,
        expected: String,
        found: String,
    },
    /// The micro pack's archive or `pack.json` is wrong.
    Pack(String),
    /// The theme file does not parse.
    Theme(String),
    /// A check supplied by the host failed (the WASM sandbox, say).
    Host(String),
}

impl fmt::Display for CheckError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CheckError::NotWasm => f.write_str("not a WebAssembly module"),
            CheckError::ManifestMismatch {
                field,
                expected,
                found,
            } => write!(
                f,
                "the manifest's {field} is {found}, the listing says {expected}"
            ),
            CheckError::Pack(message) => write!(f, "micro pack: {message}"),
            CheckError::Theme(message) => write!(f, "theme: {message}"),
            CheckError::Host(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for CheckError {}

/// Why the lockfile, or a file it records, was refused.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum LockfileError {
    /// Reading or writing failed.
    Io { path: String, message: String },
    /// The lockfile is malformed.
    Invalid(String),
    /// A lockfile format this release does not read.
    UnsupportedSchema(u64),
    /// Nothing with this id is installed.
    NotInstalled(String),
    /// The installed file is missing.
    Missing { id: String, path: String },
    /// The installed file changed on disk: it is not loaded.
    Mismatch {
        id: String,
        expected: String,
        actual: String,
    },
}

impl fmt::Display for LockfileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LockfileError::Io { path, message } => write!(f, "{path}: {message}"),
            LockfileError::Invalid(message) => write!(f, "lockfile: {message}"),
            LockfileError::UnsupportedSchema(version) => write!(
                f,
                "lockfile schema_version {version} is not supported; update rs-rich"
            ),
            LockfileError::NotInstalled(id) => write!(f, "{id} is not installed"),
            LockfileError::Missing { id, path } => {
                write!(f, "{id}: the installed file {path} is missing")
            }
            LockfileError::Mismatch {
                id,
                expected,
                actual,
            } => write!(
                f,
                "{id}: the installed file's SHA-256 is {actual}, the lockfile records {expected}; it changed on disk and is not loaded"
            ),
        }
    }
}

impl std::error::Error for LockfileError {}

/// Why an install failed. Nothing is written unless every check passed.
#[derive(Debug)]
#[non_exhaustive]
pub enum InstallError {
    /// A crate, which is listed only.
    NotInstallable { id: String, kind: &'static str },
    /// The index is not usable (expired, say).
    Index(IndexError),
    /// The listing is not the one the index names.
    Listing(ListingError),
    /// The version is yanked or does not suit this host.
    Resolve(ResolveError),
    /// The download failed.
    Fetch(FetchError),
    /// The download's size is not the declared size.
    SizeMismatch { expected: u64, actual: u64 },
    /// The download's SHA-256 is not the listed one.
    HashMismatch { expected: String, actual: String },
    /// The artifact failed its check by kind.
    Check(CheckError),
    /// Writing the file or the lockfile failed.
    Lockfile(LockfileError),
}

impl fmt::Display for InstallError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            InstallError::NotInstallable { id, kind } => {
                write!(
                    f,
                    "{id} is a {kind}, which is listed only and never installed"
                )
            }
            InstallError::Index(e) => e.fmt(f),
            InstallError::Listing(e) => e.fmt(f),
            InstallError::Resolve(e) => e.fmt(f),
            InstallError::Fetch(e) => e.fmt(f),
            InstallError::SizeMismatch { expected, actual } => write!(
                f,
                "the download is {actual} bytes, the listing declares {expected}"
            ),
            InstallError::HashMismatch { expected, actual } => write!(
                f,
                "the download's SHA-256 is {actual}, the listing records {expected}"
            ),
            InstallError::Check(e) => e.fmt(f),
            InstallError::Lockfile(e) => e.fmt(f),
        }
    }
}

impl std::error::Error for InstallError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            InstallError::Index(e) => Some(e),
            InstallError::Listing(e) => Some(e),
            InstallError::Resolve(e) => Some(e),
            InstallError::Fetch(e) => Some(e),
            InstallError::Check(e) => Some(e),
            InstallError::Lockfile(e) => Some(e),
            _ => None,
        }
    }
}

impl From<IndexError> for InstallError {
    fn from(e: IndexError) -> Self {
        InstallError::Index(e)
    }
}

impl From<ListingError> for InstallError {
    fn from(e: ListingError) -> Self {
        InstallError::Listing(e)
    }
}

impl From<ResolveError> for InstallError {
    fn from(e: ResolveError) -> Self {
        InstallError::Resolve(e)
    }
}

impl From<FetchError> for InstallError {
    fn from(e: FetchError) -> Self {
        InstallError::Fetch(e)
    }
}

impl From<CheckError> for InstallError {
    fn from(e: CheckError) -> Self {
        InstallError::Check(e)
    }
}

impl From<LockfileError> for InstallError {
    fn from(e: LockfileError) -> Self {
        InstallError::Lockfile(e)
    }
}
