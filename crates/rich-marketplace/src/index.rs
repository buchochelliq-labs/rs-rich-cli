//! `index.json` and its signature: every listing's SHA-256, a serial that
//! only grows, and an expiry, signed by the current main key.

use std::collections::BTreeMap;
use std::time::SystemTime;

use serde_json::Value;

use crate::error::{IndexError, ListingError};
use crate::keys::{KeyId, KeySet, ParsedSignature};
use crate::listing::Listing;
use crate::util::{is_lower_hex, parse_json, parse_timestamp, sha256_hex, unix_seconds};

/// The only index format this release reads.
pub const INDEX_SCHEMA_VERSION: u64 = 1;

/// The trusted comment prefix of `index.json.minisig`; the serial follows.
pub const INDEX_COMMENT_PREFIX: &str = "rs-rich-marketplace index serial=";

/// What a client remembers of the last index it accepted, to refuse a
/// rollback: its serial and the SHA-256 of its bytes.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct AcceptedIndex {
    pub serial: u64,
    pub sha256: String,
}

/// One listing the index names.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct IndexEntry {
    /// `packages/<id>.json`, relative to the index.
    pub path: String,
    /// The listing file's SHA-256, lowercase hex.
    pub sha256: String,
}

/// An index whose signature, key, serial, schema and expiry were checked.
#[derive(Clone, Debug)]
pub struct VerifiedIndex {
    serial: u64,
    generated: String,
    expires: String,
    expires_at: i64,
    sha256: String,
    key_id: KeyId,
    listings: BTreeMap<String, IndexEntry>,
}

/// Verify `index.json` (`index_bytes`) and `index.json.minisig`
/// (`signature`) against the current generation's main key.
///
/// In order: the signature's algorithm (the legacy `Ed` is refused) and key
/// id, the signature, the trusted comment's serial, the index's
/// `schema_version` and shape, that the serials agree, the serial against
/// `last` (the last index accepted: an older serial, or the same serial with
/// other bytes, is refused), and the expiry against `now`.
pub fn verify_index(
    keys: &KeySet,
    index_bytes: &[u8],
    signature: &str,
    last: Option<&AcceptedIndex>,
    now: SystemTime,
) -> Result<VerifiedIndex, IndexError> {
    let parsed = ParsedSignature::parse(signature).map_err(IndexError::Signature)?;
    let main = keys.main();
    if keys.revoked().contains(&parsed.key_id()) || parsed.key_id() != main.id() {
        return Err(IndexError::Signature(
            crate::error::SignatureError::WrongKeyId {
                expected: main.id(),
                found: parsed.key_id(),
            },
        ));
    }
    // The comment is checked against itself first, so a valid signature
    // over a wrong comment is still refused (by the serial check below).
    let comment = parsed.trusted_comment().to_string();
    main.verify(index_bytes, &parsed, &comment)
        .map_err(IndexError::Signature)?;
    let signed = comment
        .strip_prefix(INDEX_COMMENT_PREFIX)
        .filter(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()) && !n.starts_with('0'))
        .and_then(|n| n.parse::<u64>().ok())
        .ok_or_else(|| IndexError::TrustedComment(comment.clone()))?;

    let mut index = parse_index(index_bytes)?;
    if index.serial != signed {
        return Err(IndexError::SerialMismatch {
            signed,
            index: index.serial,
        });
    }
    index.sha256 = sha256_hex(index_bytes);
    index.key_id = parsed.key_id();
    if let Some(last) = last {
        if index.serial < last.serial {
            return Err(IndexError::Rollback {
                serial: index.serial,
                last: last.serial,
            });
        }
        if index.serial == last.serial && index.sha256 != last.sha256 {
            return Err(IndexError::SerialReused {
                serial: index.serial,
            });
        }
    }
    index.check_fresh(now)?;
    Ok(index)
}

fn parse_index(bytes: &[u8]) -> Result<VerifiedIndex, IndexError> {
    let invalid = |message: String| IndexError::Invalid(message);
    let value = parse_json(bytes).map_err(invalid)?;
    let map = value
        .as_object()
        .ok_or_else(|| invalid("not a JSON object".into()))?;
    match map.get("schema_version") {
        Some(v) if v.as_u64() == Some(INDEX_SCHEMA_VERSION) => {}
        Some(v) => match v.as_u64() {
            Some(other) => return Err(IndexError::UnsupportedSchema(other)),
            None => return Err(invalid("schema_version: not an integer".into())),
        },
        None => return Err(invalid("schema_version: missing".into())),
    }
    let serial = map
        .get("serial")
        .and_then(Value::as_u64)
        .filter(|&n| n >= 1)
        .ok_or_else(|| invalid("serial: missing or not a positive integer".into()))?;
    let time = |key: &str| -> Result<(String, i64), IndexError> {
        let text = map
            .get(key)
            .and_then(Value::as_str)
            .ok_or_else(|| invalid(format!("{key}: missing or not a string")))?;
        let seconds = parse_timestamp(text).ok_or_else(|| {
            invalid(format!(
                "{key}: not a UTC time YYYY-MM-DDTHH:MM:SSZ: {text:?}"
            ))
        })?;
        Ok((text.to_string(), seconds))
    };
    let (generated, generated_at) = time("generated")?;
    let (expires, expires_at) = time("expires")?;
    if expires_at <= generated_at {
        return Err(invalid("expires is not after generated".into()));
    }
    let entries = map
        .get("listings")
        .and_then(Value::as_object)
        .ok_or_else(|| invalid("listings: missing or not an object".into()))?;
    let mut listings = BTreeMap::new();
    for (id, entry) in entries {
        let at = format!("listings.{id}");
        if !rich_plugin_api::is_valid_name(id) {
            return Err(invalid(format!("{at}: not a valid package id")));
        }
        let entry = entry
            .as_object()
            .ok_or_else(|| invalid(format!("{at}: not an object")))?;
        let path = entry
            .get("path")
            .and_then(Value::as_str)
            .ok_or_else(|| invalid(format!("{at}.path: missing or not a string")))?;
        if path != format!("packages/{id}.json") {
            return Err(invalid(format!("{at}.path: must be packages/{id}.json")));
        }
        let sha256 = entry
            .get("sha256")
            .and_then(Value::as_str)
            .filter(|s| is_lower_hex(s, 64))
            .ok_or_else(|| invalid(format!("{at}.sha256: not 64 lowercase hex digits")))?;
        listings.insert(
            id.clone(),
            IndexEntry {
                path: path.to_string(),
                sha256: sha256.to_string(),
            },
        );
    }
    Ok(VerifiedIndex {
        serial,
        generated,
        expires,
        expires_at,
        sha256: String::new(),
        key_id: KeyId::from_bytes([0; 8]),
        listings,
    })
}

impl VerifiedIndex {
    /// The serial.
    pub fn serial(&self) -> u64 {
        self.serial
    }

    /// When it was generated, as written (`2026-10-20T12:00:00Z`).
    pub fn generated(&self) -> &str {
        &self.generated
    }

    /// When it expires, as written.
    pub fn expires(&self) -> &str {
        &self.expires
    }

    /// The SHA-256 of the index's bytes.
    pub fn sha256(&self) -> &str {
        &self.sha256
    }

    /// The key that signed it.
    pub fn key_id(&self) -> KeyId {
        self.key_id
    }

    /// What to remember of it, to refuse a rollback next time.
    pub fn accepted(&self) -> AcceptedIndex {
        AcceptedIndex {
            serial: self.serial,
            sha256: self.sha256.clone(),
        }
    }

    /// Every listing, by id.
    pub fn listings(&self) -> &BTreeMap<String, IndexEntry> {
        &self.listings
    }

    /// One listing's entry.
    pub fn entry(&self, id: &str) -> Option<&IndexEntry> {
        self.listings.get(id)
    }

    /// Refuse an index past its expiry, for anything that resolves or
    /// installs. Loading what is installed never needs this.
    pub fn check_fresh(&self, now: SystemTime) -> Result<(), IndexError> {
        if unix_seconds(now) >= self.expires_at {
            return Err(IndexError::Expired {
                expires: self.expires.clone(),
            });
        }
        Ok(())
    }

    /// Read the listing filed as `id` from its file's bytes: its SHA-256
    /// against the index, then the format and the rules beyond the schema,
    /// then that its `id` is `id`.
    pub fn read_listing(&self, id: &str, bytes: &[u8]) -> Result<Listing, ListingError> {
        let entry = self
            .entry(id)
            .ok_or_else(|| ListingError::NotInIndex(id.to_string()))?;
        let actual = sha256_hex(bytes);
        if actual != entry.sha256 {
            return Err(ListingError::HashMismatch {
                id: id.to_string(),
                expected: entry.sha256.clone(),
                actual,
            });
        }
        let listing = Listing::parse(bytes)?;
        if listing.id != id {
            return Err(ListingError::IdMismatch {
                filed: id.to_string(),
                found: listing.id,
            });
        }
        Ok(listing)
    }
}
