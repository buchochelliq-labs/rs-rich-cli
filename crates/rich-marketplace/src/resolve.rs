//! Choosing a version for the running host
//! ([design: Resolving a version](https://github.com/buchochelliq-labs/rs-rich-cli/blob/main/docs/design/marketplace.md#resolving-a-version)).

use rich_plugin_api::abi::{self, CapabilityKind};

use crate::error::{Incompatibility, ResolveError};
use crate::listing::{Kind, ListedVersion, Listing, Requires};

/// `rich_micro::package::SCHEMA_VERSION`, the `pack.json` schema the host
/// reads. Restated here so this crate does not build rs-rich-micro (and its
/// image decoders); rs-rich-cli's tests check that the two agree.
pub const MICRO_SCHEMA_VERSION: u64 = 1;

/// The theme format the host reads: `1`, upstream `rich`'s `[styles]` INI
/// file, as `--theme-file` reads it.
pub const THEME_FORMAT: u64 = 1;

/// What the running host loads. [`Host::current`] is this build's; a test
/// changes a field to play a host of another version.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct Host {
    /// `rich_plugin_api::PLUGIN_API_VERSION`.
    pub plugin_api: u32,
    /// `rich_plugin_api::abi::ABI_MAJOR`.
    pub abi_major: u32,
    /// `rich_plugin_api::abi::ABI_MINOR`.
    pub abi_minor: u32,
    /// [`MICRO_SCHEMA_VERSION`].
    pub micro_schema: u64,
    /// [`THEME_FORMAT`].
    pub theme_format: u64,
}

impl Host {
    /// This build: the constants in rs-rich-plugin-api and above.
    pub const fn current() -> Self {
        Host {
            plugin_api: rich_plugin_api::PLUGIN_API_VERSION,
            abi_major: abi::ABI_MAJOR,
            abi_minor: abi::ABI_MINOR,
            micro_schema: MICRO_SCHEMA_VERSION,
            theme_format: THEME_FORMAT,
        }
    }

    /// Whether this host can use `version` of a package of `kind`: the
    /// loaders' own checks (`abi::is_supported`,
    /// `is_supported_api_version`), and for a WASM plugin, that every
    /// capability kind is one the host knows.
    pub fn check(&self, kind: Kind, version: &ListedVersion) -> Result<(), Incompatibility> {
        match version.requires {
            Requires::Abi { major, minor } => {
                // The host's own check, when this host is the build's.
                let supported = if *self == Host::current() {
                    abi::is_supported(major, minor)
                } else {
                    major == self.abi_major
                };
                if !supported {
                    return Err(Incompatibility::Abi {
                        required: (major, minor),
                        host: (self.abi_major, self.abi_minor),
                    });
                }
                if kind == Kind::WasmPlugin {
                    if let Some(unknown) = version
                        .capabilities
                        .iter()
                        .find(|c| CapabilityKind::parse(&c.kind).is_none())
                    {
                        return Err(Incompatibility::UnknownCapability {
                            kind: unknown.kind.clone(),
                        });
                    }
                }
            }
            Requires::PluginApi(required) => {
                let supported = if *self == Host::current() {
                    u32::try_from(required).is_ok_and(rich_plugin_api::is_supported_api_version)
                } else {
                    required == u64::from(self.plugin_api)
                };
                if !supported {
                    return Err(Incompatibility::PluginApi {
                        required,
                        host: self.plugin_api,
                    });
                }
            }
            Requires::MicroSchema(required) if required != self.micro_schema => {
                return Err(Incompatibility::MicroSchema {
                    required,
                    host: self.micro_schema,
                })
            }
            Requires::ThemeFormat(required) if required != self.theme_format => {
                return Err(Incompatibility::ThemeFormat {
                    required,
                    host: self.theme_format,
                })
            }
            _ => {}
        }
        Ok(())
    }
}

impl Default for Host {
    fn default() -> Self {
        Host::current()
    }
}

/// Which version to resolve.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Request {
    /// The newest candidate: not yanked, not a pre-release, and loadable.
    Newest,
    /// Exactly this version (the only way to get a pre-release). A yanked
    /// or unloadable one is refused.
    Exact(semver::Version),
}

/// Choose the version of `listing` to install on `host`.
pub fn resolve<'a>(
    listing: &'a Listing,
    host: &Host,
    request: &Request,
) -> Result<&'a ListedVersion, ResolveError> {
    match request {
        Request::Exact(wanted) => {
            let version = listing
                .version(wanted)
                .ok_or_else(|| ResolveError::NoSuchVersion {
                    id: listing.id.clone(),
                    version: wanted.to_string(),
                })?;
            if let Some(reason) = &version.yanked {
                return Err(ResolveError::Yanked {
                    id: listing.id.clone(),
                    version: version.version.to_string(),
                    reason: reason.clone(),
                });
            }
            host.check(listing.kind, version)
                .map_err(|reason| ResolveError::Incompatible {
                    id: listing.id.clone(),
                    version: version.version.to_string(),
                    reason,
                })?;
            Ok(version)
        }
        Request::Newest => {
            let mut newer_host_needed = None;
            for version in &listing.versions {
                if version.is_yanked() || !version.version.pre.is_empty() {
                    continue;
                }
                match host.check(listing.kind, version) {
                    Ok(()) => return Ok(version),
                    Err(reason) if reason.needs_newer_host() && newer_host_needed.is_none() => {
                        newer_host_needed = Some(version.version.to_string());
                    }
                    Err(_) => {}
                }
            }
            Err(ResolveError::NoCandidate {
                id: listing.id.clone(),
                newer_host_needed,
            })
        }
    }
}
