//! A listing, `packages/<id>.json`: one package and every version of it.
//!
//! [`Listing::parse`] makes the checks of `schemas/marketplace/listing.schema.json`
//! and the rules beyond it that `scripts/validate_marketplace.py` makes, so
//! the client refuses what the marketplace's CI refuses. As the design says,
//! within a schema version it reads, a client ignores keys it does not know.
//! One deliberate difference: a WASM capability kind this host does not know
//! is kept (a newer ABI minor may add kinds), and resolution skips that
//! version rather than refusing the listing.

use std::cmp::Ordering;
use std::fmt;

use semver::Version;
use serde_json::{Map, Value};

use crate::error::ListingError;
use crate::util::{is_lower_hex, parse_date, parse_json};

/// The only listing format this release reads.
pub const LISTING_SCHEMA_VERSION: u64 = 1;

/// Repositories under this prefix may be listed as `official`.
pub const OFFICIAL_PREFIX: &str = "https://github.com/buchochelliq-labs/";

/// What a package is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Kind {
    /// A Cargo crate: listed only, never installed.
    Crate,
    /// A sandboxed WASM module.
    WasmPlugin,
    /// A micro asset pack.
    MicroPack,
    /// An upstream `[styles]` theme file.
    Theme,
}

impl Kind {
    /// The name a listing uses.
    pub const fn as_str(self) -> &'static str {
        match self {
            Kind::Crate => "crate",
            Kind::WasmPlugin => "wasm-plugin",
            Kind::MicroPack => "micro-pack",
            Kind::Theme => "theme",
        }
    }

    /// The kind a listing's name spells.
    pub fn parse(name: &str) -> Option<Self> {
        [Kind::Crate, Kind::WasmPlugin, Kind::MicroPack, Kind::Theme]
            .into_iter()
            .find(|kind| kind.as_str() == name)
    }

    /// Whether it installs from the marketplace (all but a crate).
    pub const fn is_installable(self) -> bool {
        !matches!(self, Kind::Crate)
    }

    /// The largest artifact of this kind, in bytes, if bounded:
    /// `WasmLimits::default().module_bytes`, micro's
    /// `Limits::default().max_archive_bytes` and the CLI's theme file limit.
    pub const fn max_artifact_bytes(self) -> Option<u64> {
        match self {
            Kind::Crate => None,
            Kind::WasmPlugin => Some(16 << 20),
            Kind::MicroPack => Some(8 << 20),
            Kind::Theme => Some(1 << 20),
        }
    }

    /// The `requires` key this kind names.
    pub const fn requires_key(self) -> &'static str {
        match self {
            Kind::Crate => "plugin_api",
            Kind::WasmPlugin => "abi",
            Kind::MicroPack => "micro_schema",
            Kind::Theme => "theme_format",
        }
    }
}

impl fmt::Display for Kind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// How far the marketplace vouches for a package.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Tier {
    /// This repository's own packages.
    Official,
    /// Read and rebuilt by a maintainer.
    Reviewed,
    /// Checked by the marketplace's CI only.
    Community,
}

impl Tier {
    /// The name a listing uses.
    pub const fn as_str(self) -> &'static str {
        match self {
            Tier::Official => "official",
            Tier::Reviewed => "reviewed",
            Tier::Community => "community",
        }
    }

    /// The tier a listing's name spells.
    pub fn parse(name: &str) -> Option<Self> {
        [Tier::Official, Tier::Reviewed, Tier::Community]
            .into_iter()
            .find(|tier| tier.as_str() == name)
    }
}

impl fmt::Display for Tier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// What a version needs from the host: one number, by kind.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Requires {
    /// A WASM plugin's runtime ABI, `MAJOR.MINOR`.
    Abi { major: u32, minor: u32 },
    /// A crate's `PLUGIN_API_VERSION`.
    PluginApi(u64),
    /// A micro pack's `pack.json` schema.
    MicroSchema(u64),
    /// A theme's format.
    ThemeFormat(u64),
}

/// Where a version's file is and what it hashes to.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct Artifact {
    /// An `https://` URL.
    pub url: String,
    /// Lowercase hex SHA-256.
    pub sha256: String,
    /// Bytes.
    pub size: u64,
}

/// One thing a version adds.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct Capability {
    pub kind: String,
    pub name: String,
}

/// One version of a package.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct ListedVersion {
    pub version: Version,
    /// `YYYY-MM-DD`.
    pub published: String,
    pub requires: Requires,
    pub artifact: Artifact,
    pub source_revision: Option<String>,
    pub capabilities: Vec<Capability>,
    pub permissions: Vec<String>,
    /// The reason, when the version is yanked.
    pub yanked: Option<String>,
}

impl ListedVersion {
    /// Whether it is withdrawn.
    pub fn is_yanked(&self) -> bool {
        self.yanked.is_some()
    }
}

/// For a crate: its name on crates.io and the features that enable the plugin.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct CrateInfo {
    pub name: String,
    pub features: Vec<String>,
}

/// A maintainer's pick (the optional `recommended`): the day it was
/// given and why, in at most 140 characters. Like the tier, only a
/// maintainer sets it, and it is signed with the rest of the listing.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct Recommended {
    /// `YYYY-MM-DD`.
    pub since: String,
    pub note: String,
}

/// The longest `recommended.note`, in characters.
pub const MAX_RECOMMENDED_NOTE: usize = 140;

/// One package and every version ever listed, newest first.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct Listing {
    pub id: String,
    pub kind: Kind,
    pub name: String,
    pub description: String,
    pub tier: Tier,
    pub license: String,
    pub authors: Vec<String>,
    pub repository: String,
    pub homepage: Option<String>,
    pub keywords: Vec<String>,
    pub crate_info: Option<CrateInfo>,
    /// A maintainer's pick, when it is one.
    pub recommended: Option<Recommended>,
    pub versions: Vec<ListedVersion>,
}

impl Listing {
    /// Read and check a listing file. To read one through the index (hash
    /// and id checked too), use
    /// [`VerifiedIndex::read_listing`](crate::VerifiedIndex::read_listing).
    pub fn parse(bytes: &[u8]) -> Result<Listing, ListingError> {
        let value = parse_json(bytes).map_err(ListingError::Json)?;
        let map = value
            .as_object()
            .ok_or_else(|| invalid("$", "not a JSON object"))?;
        match map.get("schema_version") {
            Some(v) => match v.as_u64() {
                Some(LISTING_SCHEMA_VERSION) => {}
                Some(other) => return Err(ListingError::UnsupportedSchema(other)),
                None => return Err(invalid("schema_version", "not an integer")),
            },
            None => return Err(invalid("schema_version", "missing")),
        }
        let kind_name = string(map, "kind", "kind")?;
        let kind = Kind::parse(kind_name)
            .ok_or_else(|| ListingError::UnknownKind(kind_name.to_string()))?;
        let id = string(map, "id", "id")?;
        if !rich_plugin_api::is_valid_name(id) {
            return Err(invalid("id", "not a valid package id"));
        }
        if kind == Kind::MicroPack && !is_micro_segment(id) {
            return Err(invalid(
                "id",
                "a micro pack's id is a micro name segment (no '.')",
            ));
        }
        let tier_name = string(map, "tier", "tier")?;
        let tier = Tier::parse(tier_name)
            .ok_or_else(|| invalid("tier", &format!("unknown tier {tier_name:?}")))?;
        let license = string(map, "license", "license")?;
        if license.is_empty() || license.chars().count() > 256 {
            return Err(invalid("license", "must be 1 to 256 characters"));
        }
        let authors = optional_list(map, "authors", 32, |text, at| {
            line(text, at).map(str::to_string)
        })?;
        let keywords = optional_list(map, "keywords", 8, |text, at| {
            if is_keyword(text) {
                Ok(text.to_string())
            } else {
                Err(invalid(at, "not a keyword (a-z, 0-9 and '-', at most 32)"))
            }
        })?;
        if has_duplicates(&keywords) {
            return Err(invalid("keywords", "listed twice"));
        }
        let repository = https_url(string(map, "repository", "repository")?, "repository")?;
        let homepage = match map.get("homepage") {
            None => None,
            Some(Value::String(url)) => Some(https_url(url, "homepage")?.to_string()),
            Some(_) => return Err(invalid("homepage", "not a string")),
        };
        let crate_info = match (kind, map.get("crate")) {
            (Kind::Crate, Some(value)) => Some(crate_info(value)?),
            (Kind::Crate, None) => return Err(invalid("crate", "required for a crate")),
            (_, Some(_)) => return Err(invalid("crate", "only for a crate")),
            (_, None) => None,
        };
        let recommended = match map.get("recommended") {
            None => None,
            Some(value) => Some(recommended(value)?),
        };
        let versions = map
            .get("versions")
            .and_then(Value::as_array)
            .ok_or_else(|| invalid("versions", "missing or not an array"))?;
        if versions.is_empty() || versions.len() > 1000 {
            return Err(invalid("versions", "must list 1 to 1000 versions"));
        }
        let versions = versions
            .iter()
            .enumerate()
            .map(|(i, v)| version(kind, v, &format!("versions[{i}]")))
            .collect::<Result<Vec<_>, _>>()?;
        let listing = Listing {
            id: id.to_string(),
            kind,
            name: line(string(map, "name", "name")?, "name")?.to_string(),
            description: line(string(map, "description", "description")?, "description")?
                .to_string(),
            tier,
            license: license.to_string(),
            authors,
            repository: repository.to_string(),
            homepage,
            keywords,
            crate_info,
            recommended,
            versions,
        };
        listing.check_rules()?;
        Ok(listing)
    }

    /// The rules beyond the schema (the validator's `semantic_errors`).
    fn check_rules(&self) -> Result<(), ListingError> {
        for (i, entry) in self.versions.iter().enumerate() {
            let at = format!("versions[{i}]");
            for (j, earlier) in self.versions[..i].iter().enumerate() {
                if earlier.version.cmp_precedence(&entry.version) == Ordering::Equal {
                    return Err(invalid(
                        &format!("{at}.version"),
                        &format!("{} repeats versions[{j}]", entry.version),
                    ));
                }
            }
            if i > 0
                && self.versions[i - 1].version.cmp_precedence(&entry.version) != Ordering::Greater
            {
                return Err(invalid(
                    &format!("{at}.version"),
                    &format!(
                        "{} is listed after {}; list versions newest first",
                        entry.version,
                        self.versions[i - 1].version
                    ),
                ));
            }
            for (j, capability) in entry.capabilities.iter().enumerate() {
                if entry.capabilities[..j].contains(capability) {
                    return Err(invalid(
                        &format!("{at}.capabilities[{j}]"),
                        &format!(
                            "{} {:?} is declared twice",
                            capability.kind, capability.name
                        ),
                    ));
                }
            }
            if self.kind == Kind::WasmPlugin {
                let ansi = entry.capabilities.iter().any(|c| c.kind == "fence-ansi");
                let declared = entry.permissions.iter().any(|p| p == "ansi-output");
                if ansi != declared {
                    return Err(invalid(
                        &format!("{at}.permissions"),
                        "'ansi-output' is declared exactly when a fence-ansi capability is",
                    ));
                }
            }
            if let Some(info) = &self.crate_info {
                let name = &info.name;
                let expected = format!(
                    "https://static.crates.io/crates/{name}/{name}-{}.crate",
                    entry.version
                );
                if entry.artifact.url != expected {
                    return Err(invalid(
                        &format!("{at}.artifact.url"),
                        &format!("must be {expected}"),
                    ));
                }
            }
        }
        if self.tier == Tier::Official && !self.repository.starts_with(OFFICIAL_PREFIX) {
            return Err(invalid(
                "tier",
                &format!("'official' is only for packages under {OFFICIAL_PREFIX}"),
            ));
        }
        Ok(())
    }

    /// The listed version equal to `version` by SemVer precedence.
    pub fn version(&self, version: &Version) -> Option<&ListedVersion> {
        self.versions
            .iter()
            .find(|v| v.version.cmp_precedence(version) == Ordering::Equal)
    }
}

fn invalid(at: &str, message: &str) -> ListingError {
    ListingError::Invalid {
        at: at.to_string(),
        message: message.to_string(),
    }
}

fn string<'a>(map: &'a Map<String, Value>, key: &str, at: &str) -> Result<&'a str, ListingError> {
    match map.get(key) {
        Some(Value::String(text)) => Ok(text),
        Some(_) => Err(invalid(at, "not a string")),
        None => Err(invalid(at, "missing")),
    }
}

/// One line: 1 to 1024 characters, no control characters.
fn recommended(value: &Value) -> Result<Recommended, ListingError> {
    let map = value
        .as_object()
        .ok_or_else(|| invalid("recommended", "not an object"))?;
    let since = string(map, "since", "recommended.since")?;
    if parse_date(since).is_none() {
        return Err(invalid("recommended.since", "not a real day YYYY-MM-DD"));
    }
    let note = line(string(map, "note", "recommended.note")?, "recommended.note")?;
    if note.chars().count() > MAX_RECOMMENDED_NOTE {
        return Err(invalid(
            "recommended.note",
            "must be at most 140 characters",
        ));
    }
    Ok(Recommended {
        since: since.to_string(),
        note: note.to_string(),
    })
}

fn line<'a>(text: &'a str, at: &str) -> Result<&'a str, ListingError> {
    let count = text.chars().count();
    if count == 0 || count > 1024 {
        return Err(invalid(at, "must be 1 to 1024 characters"));
    }
    if text.chars().any(|c| c <= '\u{1f}' || c == '\u{7f}') {
        return Err(invalid(at, "has a control character"));
    }
    Ok(text)
}

fn https_url<'a>(text: &'a str, at: &str) -> Result<&'a str, ListingError> {
    match text.strip_prefix("https://") {
        Some(rest)
            if !rest.is_empty() && text.len() <= 2048 && !text.chars().any(char::is_whitespace) =>
        {
            Ok(text)
        }
        _ => Err(invalid(at, "not an https:// URL")),
    }
}

fn optional_list<T>(
    map: &Map<String, Value>,
    key: &str,
    max: usize,
    item: impl Fn(&str, &str) -> Result<T, ListingError>,
) -> Result<Vec<T>, ListingError> {
    let Some(value) = map.get(key) else {
        return Ok(Vec::new());
    };
    let list = value
        .as_array()
        .ok_or_else(|| invalid(key, "not an array"))?;
    if list.len() > max {
        return Err(invalid(key, &format!("at most {max} entries")));
    }
    list.iter()
        .enumerate()
        .map(|(i, v)| {
            let at = format!("{key}[{i}]");
            item(v.as_str().ok_or_else(|| invalid(&at, "not a string"))?, &at)
        })
        .collect()
}

fn has_duplicates<T: PartialEq>(items: &[T]) -> bool {
    items
        .iter()
        .enumerate()
        .any(|(i, x)| items[..i].contains(x))
}

fn is_keyword(text: &str) -> bool {
    let bytes = text.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 32
        && (bytes[0].is_ascii_lowercase() || bytes[0].is_ascii_digit())
        && bytes
            .iter()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || *b == b'-')
}

fn is_micro_segment(text: &str) -> bool {
    !text.is_empty()
        && text.len() <= 64
        && text
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'-')
}

fn is_micro_name(text: &str) -> bool {
    text.len() <= 128 && text.split('/').all(is_micro_segment)
}

fn crate_info(value: &Value) -> Result<CrateInfo, ListingError> {
    let map = value
        .as_object()
        .ok_or_else(|| invalid("crate", "not an object"))?;
    let name = string(map, "name", "crate.name")?;
    let bytes = name.as_bytes();
    let valid = !bytes.is_empty()
        && bytes.len() <= 64
        && bytes[0].is_ascii_alphabetic()
        && bytes
            .iter()
            .all(|b| b.is_ascii_alphanumeric() || *b == b'_' || *b == b'-');
    if !valid {
        return Err(invalid("crate.name", "not a crate name"));
    }
    let features = optional_list(map, "features", usize::MAX, |text, at| {
        let bytes = text.as_bytes();
        let valid = !bytes.is_empty()
            && bytes.len() <= 64
            && (bytes[0].is_ascii_alphanumeric() || bytes[0] == b'_')
            && bytes
                .iter()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'+' | b'.' | b'-'));
        if valid {
            Ok(text.to_string())
        } else {
            Err(invalid(at, "not a Cargo feature name"))
        }
    })?;
    if has_duplicates(&features) {
        return Err(invalid("crate.features", "listed twice"));
    }
    Ok(CrateInfo {
        name: name.to_string(),
        features,
    })
}

fn positive_integer(value: Option<&Value>, at: &str) -> Result<u64, ListingError> {
    value
        .and_then(Value::as_u64)
        .filter(|&n| n >= 1)
        .ok_or_else(|| invalid(at, "missing or not a positive integer"))
}

fn abi_number(text: &str) -> Option<u32> {
    let valid = !text.is_empty()
        && text.len() <= 9
        && text.bytes().all(|b| b.is_ascii_digit())
        && (text == "0" || !text.starts_with('0'));
    if valid {
        text.parse().ok()
    } else {
        None
    }
}

fn requires(kind: Kind, value: Option<&Value>, at: &str) -> Result<Requires, ListingError> {
    let at = format!("{at}.requires");
    let map = value
        .and_then(Value::as_object)
        .ok_or_else(|| invalid(&at, "missing or not an object"))?;
    let own = kind.requires_key();
    for other in ["abi", "plugin_api", "micro_schema", "theme_format"] {
        if other != own && map.contains_key(other) {
            return Err(invalid(&at, &format!("a {kind} does not name {other}")));
        }
    }
    let key_at = format!("{at}.{own}");
    Ok(match kind {
        Kind::WasmPlugin => {
            let text = map
                .get("abi")
                .and_then(Value::as_str)
                .ok_or_else(|| invalid(&key_at, "missing or not a string"))?;
            let (major, minor) = text
                .split_once('.')
                .and_then(|(a, b)| Some((abi_number(a)?, abi_number(b)?)))
                .ok_or_else(|| invalid(&key_at, "not MAJOR.MINOR"))?;
            Requires::Abi { major, minor }
        }
        Kind::Crate => Requires::PluginApi(positive_integer(map.get(own), &key_at)?),
        Kind::MicroPack => Requires::MicroSchema(positive_integer(map.get(own), &key_at)?),
        Kind::Theme => Requires::ThemeFormat(positive_integer(map.get(own), &key_at)?),
    })
}

fn artifact(kind: Kind, value: Option<&Value>, at: &str) -> Result<Artifact, ListingError> {
    let at = format!("{at}.artifact");
    let map = value
        .and_then(Value::as_object)
        .ok_or_else(|| invalid(&at, "missing or not an object"))?;
    let url_at = format!("{at}.url");
    let url = https_url(string(map, "url", &url_at)?, &url_at)?;
    let shape = match kind {
        Kind::Crate => url.starts_with("https://static.crates.io/crates/"),
        Kind::WasmPlugin => url.ends_with(".wasm"),
        Kind::MicroPack => url.ends_with(".zip") || url.ends_with(".richmicro"),
        Kind::Theme => url.ends_with(".ini") || url.ends_with(".theme"),
    };
    if !shape {
        return Err(invalid(
            &url_at,
            &format!("not the URL of a {kind} artifact"),
        ));
    }
    let sha_at = format!("{at}.sha256");
    let sha256 = string(map, "sha256", &sha_at)?;
    if !is_lower_hex(sha256, 64) {
        return Err(invalid(&sha_at, "not 64 lowercase hex digits"));
    }
    let size_at = format!("{at}.size");
    let size = positive_integer(map.get("size"), &size_at)?;
    if let Some(max) = kind.max_artifact_bytes() {
        if size > max {
            return Err(invalid(
                &size_at,
                &format!("larger than the {max} bytes a {kind} may be"),
            ));
        }
    }
    Ok(Artifact {
        url: url.to_string(),
        sha256: sha256.to_string(),
        size,
    })
}

const PERMISSIONS: [&str; 8] = [
    "native-code",
    "filesystem-read",
    "filesystem-write",
    "network",
    "process",
    "environment",
    "clipboard",
    "ansi-output",
];

const CRATE_CAPABILITIES: [&str; 9] = [
    "highlighter",
    "code-highlighter",
    "theme",
    "box-style",
    "renderer",
    "fence-renderer",
    "transform",
    "action",
    "component",
];

fn capabilities(
    kind: Kind,
    value: Option<&Value>,
    at: &str,
) -> Result<Vec<Capability>, ListingError> {
    let at = format!("{at}.capabilities");
    let list = value
        .and_then(Value::as_array)
        .ok_or_else(|| invalid(&at, "missing or not an array"))?;
    if list.len() > 64 {
        return Err(invalid(&at, "at most 64 capabilities"));
    }
    if list.is_empty() && matches!(kind, Kind::WasmPlugin | Kind::Theme) {
        return Err(invalid(&at, &format!("a {kind} lists at least one")));
    }
    list.iter()
        .enumerate()
        .map(|(i, item)| {
            let at = format!("{at}[{i}]");
            let map = item
                .as_object()
                .ok_or_else(|| invalid(&at, "not an object"))?;
            let ckind = string(map, "kind", &format!("{at}.kind"))?;
            let name = string(map, "name", &format!("{at}.name"))?;
            let kind_ok = match kind {
                Kind::Crate => CRATE_CAPABILITIES.contains(&ckind),
                // Kept even when unknown here: resolution skips the version.
                Kind::WasmPlugin => is_keyword(ckind),
                Kind::MicroPack => ckind == "asset",
                Kind::Theme => ckind == "theme",
            };
            if !kind_ok {
                return Err(invalid(
                    &format!("{at}.kind"),
                    &format!("not a {kind} capability"),
                ));
            }
            let name_ok = match kind {
                Kind::MicroPack => is_micro_name(name),
                _ => rich_plugin_api::is_valid_name(name),
            };
            if !name_ok {
                return Err(invalid(
                    &format!("{at}.name"),
                    "not a valid capability name",
                ));
            }
            Ok(Capability {
                kind: ckind.to_string(),
                name: name.to_string(),
            })
        })
        .collect()
}

fn permissions(kind: Kind, value: Option<&Value>, at: &str) -> Result<Vec<String>, ListingError> {
    let at = format!("{at}.permissions");
    let list = value
        .and_then(Value::as_array)
        .ok_or_else(|| invalid(&at, "missing or not an array"))?;
    let mut out: Vec<String> = Vec::new();
    for (i, item) in list.iter().enumerate() {
        let item_at = format!("{at}[{i}]");
        let name = item
            .as_str()
            .filter(|p| PERMISSIONS.contains(p))
            .ok_or_else(|| invalid(&item_at, "not a known permission"))?;
        if out.iter().any(|p| p == name) {
            return Err(invalid(&item_at, "listed twice"));
        }
        let allowed = match kind {
            Kind::Crate => name != "ansi-output",
            Kind::WasmPlugin => name == "ansi-output",
            Kind::MicroPack | Kind::Theme => false,
        };
        if !allowed {
            return Err(invalid(&item_at, &format!("a {kind} cannot have {name:?}")));
        }
        out.push(name.to_string());
    }
    if kind == Kind::Crate && !out.iter().any(|p| p == "native-code") {
        return Err(invalid(&at, "a crate always has 'native-code'"));
    }
    Ok(out)
}

fn version(kind: Kind, value: &Value, at: &str) -> Result<ListedVersion, ListingError> {
    let map = value
        .as_object()
        .ok_or_else(|| invalid(at, "not an object"))?;
    let version_at = format!("{at}.version");
    let text = string(map, "version", &version_at)?;
    if text.len() > 256 {
        return Err(invalid(&version_at, "longer than 256 bytes"));
    }
    let number = Version::parse(text).map_err(|e| invalid(&version_at, &e.to_string()))?;
    let published_at = format!("{at}.published");
    let published = string(map, "published", &published_at)?;
    if parse_date(published).is_none() {
        return Err(invalid(&published_at, "not a real day YYYY-MM-DD"));
    }
    let source_revision = match map.get("source_revision") {
        None => None,
        Some(Value::String(rev)) if is_lower_hex(rev, 40) || is_lower_hex(rev, 64) => {
            Some(rev.clone())
        }
        Some(_) => {
            return Err(invalid(
                &format!("{at}.source_revision"),
                "not a full Git commit (40 or 64 lowercase hex digits)",
            ))
        }
    };
    if kind == Kind::WasmPlugin && source_revision.is_none() {
        return Err(invalid(
            &format!("{at}.source_revision"),
            "required for a WASM plugin",
        ));
    }
    let yanked = match (map.get("yanked"), map.get("yank_reason")) {
        (Some(Value::Bool(true)), Some(Value::String(reason))) => {
            Some(line(reason, &format!("{at}.yank_reason"))?.to_string())
        }
        (Some(Value::Bool(true)), _) => {
            return Err(invalid(
                &format!("{at}.yank_reason"),
                "required when yanked",
            ))
        }
        (Some(Value::Bool(false)) | None, None) => None,
        (Some(Value::Bool(false)) | None, Some(_)) => {
            return Err(invalid(
                &format!("{at}.yank_reason"),
                "only for a yanked version",
            ))
        }
        (Some(_), _) => return Err(invalid(&format!("{at}.yanked"), "not a boolean")),
    };
    Ok(ListedVersion {
        version: number,
        published: published.to_string(),
        requires: requires(kind, map.get("requires"), at)?,
        artifact: artifact(kind, map.get("artifact"), at)?,
        source_revision,
        capabilities: capabilities(kind, map.get("capabilities"), at)?,
        permissions: permissions(kind, map.get("permissions"), at)?,
        yanked,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn examples(which: &str) -> Vec<PathBuf> {
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../schemas/marketplace/examples")
            .join(which);
        let mut files: Vec<_> = std::fs::read_dir(&dir)
            .unwrap_or_else(|e| panic!("{}: {e}", dir.display()))
            .map(|e| e.unwrap().path())
            .filter(|p| p.extension().is_some_and(|x| x == "json"))
            .collect();
        files.sort();
        files
    }

    #[test]
    fn the_valid_examples_parse() {
        let files = examples("valid");
        assert!(files.len() >= 4);
        for path in files {
            let bytes = std::fs::read(&path).unwrap();
            if let Err(e) = Listing::parse(&bytes) {
                panic!("{}: {e}", path.display());
            }
        }
    }

    #[test]
    fn the_invalid_examples_are_refused() {
        let files = examples("invalid");
        assert!(files.len() >= 13);
        for path in files {
            let bytes = std::fs::read(&path).unwrap();
            assert!(
                Listing::parse(&bytes).is_err(),
                "{} was accepted",
                path.display()
            );
        }
    }

    #[test]
    fn the_shout_example_reads_as_written() {
        let path = examples("valid")
            .into_iter()
            .find(|p| p.ends_with("shout.json"))
            .unwrap();
        let listing = Listing::parse(&std::fs::read(path).unwrap()).unwrap();
        assert_eq!(listing.kind, Kind::WasmPlugin);
        assert_eq!(listing.tier, Tier::Community);
        let newest = &listing.versions[0];
        assert_eq!(newest.requires, Requires::Abi { major: 1, minor: 0 });
        assert!(!newest.is_yanked());
        assert!(listing.versions.iter().any(ListedVersion::is_yanked));
    }

    #[test]
    fn recommended_reads_as_written() {
        let path = examples("valid")
            .into_iter()
            .find(|p| p.ends_with("status-icons.json"))
            .unwrap();
        let text = std::fs::read_to_string(path).unwrap();
        let listing = Listing::parse(text.as_bytes()).unwrap();
        let recommended = listing.recommended.unwrap();
        assert_eq!(recommended.since, "2026-10-26");
        assert!(recommended.note.starts_with("Clear status badges"));
        let long = text.replacen(&recommended.note, &"x".repeat(MAX_RECOMMENDED_NOTE + 1), 1);
        assert!(Listing::parse(long.as_bytes()).is_err());
        let bad_day = text.replacen("\"since\": \"2026-10-26\"", "\"since\": \"2026-02-30\"", 1);
        assert!(Listing::parse(bad_day.as_bytes()).is_err());
        let without = examples("valid")
            .into_iter()
            .find(|p| p.ends_with("shout.json"))
            .unwrap();
        let listing = Listing::parse(&std::fs::read(without).unwrap()).unwrap();
        assert_eq!(listing.recommended, None);
    }

    #[test]
    fn unknown_keys_are_ignored_but_unknown_schema_versions_are_not() {
        let path = examples("valid")
            .into_iter()
            .find(|p| p.ends_with("solarized.json"))
            .unwrap();
        let text = std::fs::read_to_string(path).unwrap();
        let extra = text.replacen('{', "{\"future_key\": true,", 1);
        assert!(Listing::parse(extra.as_bytes()).is_ok());
        let v2 = text.replacen("\"schema_version\": 1", "\"schema_version\": 2", 1);
        assert_eq!(
            Listing::parse(v2.as_bytes()),
            Err(ListingError::UnsupportedSchema(2))
        );
    }
}
