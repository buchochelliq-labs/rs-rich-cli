//! The client side of the rs-rich plugin marketplace.
//!
//! The marketplace (`buchochelliq-labs/rs-rich-marketplace`) stores where each
//! artifact is and what it hashes to, never the artifact. This crate reads it
//! and installs from it, following the contract in
//! [docs/design/marketplace.md](https://github.com/buchochelliq-labs/rs-rich-cli/blob/main/docs/design/marketplace.md):
//!
//! 1. **Keys** ([`KeySet`]): the trusted generation of the main and backup
//!    keys, and [`KeySet::walk`] to accept later generations one at a time.
//! 2. **The index** ([`verify_index`]): `index.json.minisig` by the main key
//!    (minisign's hashed `ED` algorithm only, the key id checked first), the
//!    trusted comment's serial, the `schema_version`, a rollback against the
//!    last accepted serial, and the expiry.
//! 3. **Listings** ([`VerifiedIndex::read_listing`]): each file's SHA-256
//!    against the index, then the format and the rules beyond the schema.
//! 4. **Resolution** ([`resolve`]): the newest version that is not yanked and
//!    that this host loads, decided with rs-rich-plugin-api's own checks.
//! 5. **Install** ([`Installer`]): an HTTPS download within the declared
//!    size through a [`Transport`], the SHA-256, a check by kind, an atomic
//!    move into place and the [`Lockfile`].
//! 6. **Load** ([`Lockfile::verify`]): each run reads an installed file only
//!    after checking it against the lockfile.
//!
//! There is no terminal code here: `rich plugin` in rs-rich-cli prompts,
//! prints and decides. Time is passed in (`now`), so callers and tests
//! control expiry.
//!
//! ```no_run
//! use std::time::SystemTime;
//! use rich_marketplace::{resolve, verify_index, Host, Installer, KeySet, Request, Transport};
//! # fn run(transport: impl Transport, index: &[u8], sig: &str, listing: &[u8]) -> Result<(), Box<dyn std::error::Error>> {
//! let keys = KeySet::official()?;
//! let index = verify_index(&keys, index, sig, None, SystemTime::now())?;
//! let listing = index.read_listing("shout", listing)?;
//! let version = resolve(&listing, &Host::current(), &Request::Newest)?;
//! let installer = Installer::new("/home/me/.local/share/rich/plugins", transport);
//! installer.install(&index, &listing, version, SystemTime::now())?;
//! let bytes = installer.load("shout")?; // checked against the lockfile
//! # let _ = bytes; Ok(()) }
//! ```

mod check;
mod error;
mod index;
mod install;
mod keys;
mod listing;
mod lockfile;
#[cfg(feature = "http")]
pub mod redirect;
mod resolve;
#[cfg(feature = "http")]
pub mod tls;
mod transport;
mod util;

pub use check::{
    check_pack_json, check_theme, check_wasm_header, compare_wasm_manifest, ArtifactChecker,
    BuiltinChecks, WasmExpectation,
};
pub use error::{
    CheckError, FetchError, Incompatibility, IndexError, InstallError, KeyChainError, ListingError,
    LockfileError, ResolveError, SignatureError,
};
pub use index::{
    verify_index, AcceptedIndex, IndexEntry, VerifiedIndex, INDEX_COMMENT_PREFIX,
    INDEX_SCHEMA_VERSION,
};
pub use install::Installer;
pub use keys::{
    KeyId, KeySet, Rotation, RotationKind, TrustedKey, KEYS_COMMENT_PREFIX, KEYS_SCHEMA_VERSION,
};
pub use listing::{
    Artifact, Capability, CrateInfo, Kind, ListedVersion, Listing, Recommended, Requires, Tier,
    LISTING_SCHEMA_VERSION, MAX_RECOMMENDED_NOTE, OFFICIAL_PREFIX,
};
pub use lockfile::{LockEntry, Lockfile, LOCKFILE_NAME, LOCKFILE_SCHEMA_VERSION};
pub use resolve::{resolve, Host, Request, MICRO_SCHEMA_VERSION, THEME_FORMAT};
#[cfg(feature = "http")]
pub use transport::HttpsTransport;
pub use transport::{require_https, MemoryTransport, Transport};

/// Re-exported so callers name versions with the same type.
pub use semver;
