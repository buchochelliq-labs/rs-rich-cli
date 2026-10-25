//! The trust root: minisign keys, their ids, the key chain and signature
//! checks ([design: Signing](https://github.com/buchochelliq-labs/rs-rich-cli/blob/main/docs/design/marketplace.md#signing)).
//!
//! A generation is a main key, which signs indexes, and a backup key, which
//! signs nothing but the next generation. `keys/keys-N.json` in the
//! marketplace repository describes generation `N`:
//!
//! ```json
//! {
//!   "schema_version": 1,
//!   "generation": 2,
//!   "main": {"id": "8C6B1E0F2A9D4C31", "public_key": "RWQxTJ0qDx5rjK…"},
//!   "backup": {"id": "1D0A7F3E5B2C9E48", "public_key": "RWRInixbPn8KHf…"},
//!   "revoked": ["0123456789ABCDEF"]
//! }
//! ```
//!
//! An id is minisign's key number, printed as minisign prints it (16
//! upper-case hex digits of the little-endian number). `revoked` is
//! optional. Generation `N + 1` is accepted against generation `N` by
//! [`KeySet::rotate`]: signed by both of `N`'s keys (a planned rotation), or
//! by `N`'s backup key alone when it revokes `N`'s main key (a lost or leaked
//! main key). Each signature's trusted comment is
//! `rs-rich-marketplace keys generation=N+1`.

use std::fmt;

use minisign_verify::{PublicKey, Signature};
use serde_json::Value;

use crate::error::{KeyChainError, SignatureError};
use crate::util::{base64_decode, parse_json};

/// The only keys-file format this release reads.
pub const KEYS_SCHEMA_VERSION: u64 = 1;

/// The trusted comment prefix of a key rotation's signatures.
pub const KEYS_COMMENT_PREFIX: &str = "rs-rich-marketplace keys generation=";

/// The official generation compiled into this release: the public halves of
/// the maintainer's main and backup keys, as a `keys-N.json` document.
///
/// `None` until the maintainer has made the pairs with `minisign -G` and
/// recorded them (0.0.22 plan, decision 1). Until then
/// [`KeySet::official`] refuses, and only a client built with explicit keys
/// (a test) verifies anything.
const OFFICIAL_KEYS: Option<&str> = None;

/// A minisign key id: the 8-byte key number in every key and signature.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct KeyId([u8; 8]);

impl KeyId {
    /// The id from its raw bytes, as stored in a key or signature.
    pub const fn from_bytes(bytes: [u8; 8]) -> Self {
        KeyId(bytes)
    }

    /// The raw bytes.
    pub const fn as_bytes(&self) -> &[u8; 8] {
        &self.0
    }

    /// Parse the 16 hex digits minisign prints (either case).
    pub fn parse(text: &str) -> Option<Self> {
        if text.len() != 16 || !text.bytes().all(|b| b.is_ascii_hexdigit()) {
            return None;
        }
        u64::from_str_radix(text, 16)
            .ok()
            .map(|n| KeyId(n.to_le_bytes()))
    }
}

impl fmt::Display for KeyId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:016X}", u64::from_le_bytes(self.0))
    }
}

impl fmt::Debug for KeyId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "KeyId({self})")
    }
}

/// One minisign public key and its id.
#[derive(Clone, PartialEq, Eq)]
pub struct TrustedKey {
    id: KeyId,
    encoded: String,
    key: PublicKey,
}

impl TrustedKey {
    /// A key from the base64 line of a `minisign.pub` file (`RW…`).
    pub fn from_base64(encoded: &str) -> Result<Self, KeyChainError> {
        let encoded = encoded.trim();
        let bad = || KeyChainError::BadKey(format!("not a minisign public key: {encoded:?}"));
        let raw = base64_decode(encoded).ok_or_else(bad)?;
        if raw.len() != 42 || &raw[..2] != b"Ed" {
            return Err(bad());
        }
        let key = PublicKey::from_base64(encoded).map_err(|_| bad())?;
        let mut id = [0u8; 8];
        id.copy_from_slice(&raw[2..10]);
        Ok(TrustedKey {
            id: KeyId(id),
            encoded: encoded.to_string(),
            key,
        })
    }

    /// A key from a whole `minisign.pub` file (comment line, then the key).
    pub fn from_minisign_pub(text: &str) -> Result<Self, KeyChainError> {
        let line = text
            .lines()
            .map(str::trim)
            .find(|line| !line.is_empty() && !line.starts_with("untrusted comment:"))
            .ok_or_else(|| KeyChainError::BadKey("an empty minisign public key file".into()))?;
        TrustedKey::from_base64(line)
    }

    /// The key's id.
    pub fn id(&self) -> KeyId {
        self.id
    }

    /// The base64 public key, as `minisign.pub` holds it.
    pub fn to_base64(&self) -> &str {
        &self.encoded
    }

    /// Check `signature` over `data` by this key, and that its trusted
    /// comment is exactly `comment`.
    pub(crate) fn verify(
        &self,
        data: &[u8],
        signature: &ParsedSignature,
        comment: &str,
    ) -> Result<(), SignatureError> {
        if signature.key_id != self.id {
            return Err(SignatureError::WrongKeyId {
                expected: self.id,
                found: signature.key_id,
            });
        }
        self.key
            .verify(data, &signature.signature, false)
            .map_err(|_| SignatureError::Invalid)?;
        if signature.trusted_comment() != comment {
            return Err(SignatureError::TrustedComment {
                expected: comment.to_string(),
                found: signature.trusted_comment().to_string(),
            });
        }
        Ok(())
    }
}

impl fmt::Debug for TrustedKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TrustedKey")
            .field("id", &self.id)
            .field("key", &self.encoded)
            .finish()
    }
}

/// A `.minisig` file, read but not yet verified.
#[derive(Clone)]
pub(crate) struct ParsedSignature {
    key_id: KeyId,
    signature: Signature,
}

impl ParsedSignature {
    /// Read a `.minisig` file's text. The legacy unhashed algorithm (`Ed`)
    /// is refused here, before any key is tried.
    pub(crate) fn parse(text: &str) -> Result<Self, SignatureError> {
        let line = text.lines().nth(1).ok_or(SignatureError::Malformed)?;
        let raw = base64_decode(line).ok_or(SignatureError::Malformed)?;
        if raw.len() != 74 {
            return Err(SignatureError::Malformed);
        }
        match &raw[..2] {
            b"ED" => {}
            b"Ed" => return Err(SignatureError::LegacyAlgorithm),
            _ => return Err(SignatureError::UnsupportedAlgorithm),
        }
        let signature = Signature::decode(text).map_err(|_| SignatureError::Malformed)?;
        let mut id = [0u8; 8];
        id.copy_from_slice(&raw[2..10]);
        Ok(ParsedSignature {
            key_id: KeyId(id),
            signature,
        })
    }

    pub(crate) fn key_id(&self) -> KeyId {
        self.key_id
    }

    /// The trusted comment, without minisign's `trusted comment: ` prefix.
    /// Unverified until a key has checked the signature.
    pub(crate) fn trusted_comment(&self) -> &str {
        self.signature.trusted_comment()
    }
}

/// How a generation was accepted.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum RotationKind {
    /// Signed by both keys of the generation before: a planned rotation.
    Planned,
    /// Signed by the backup key alone, revoking the old main key: the main
    /// key was lost or leaked and has been replaced. A client says so.
    MainKeyReplaced {
        /// The main key the new generation revokes.
        revoked: KeyId,
    },
}

/// One generation after the current one, as fetched from the marketplace:
/// `keys/keys-N.json` and its signatures.
#[derive(Clone, Copy, Debug)]
pub struct Rotation<'a> {
    /// `keys-N.json`'s bytes.
    pub keys_json: &'a [u8],
    /// `keys-N.json.main.minisig`, when there is one.
    pub main_signature: Option<&'a str>,
    /// `keys-N.json.backup.minisig`: always required.
    pub backup_signature: &'a str,
}

/// One generation of the marketplace's keys.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KeySet {
    generation: u64,
    main: TrustedKey,
    backup: TrustedKey,
    revoked: Vec<KeyId>,
}

impl KeySet {
    /// A generation from its keys. The two must differ.
    pub fn new(
        generation: u64,
        main: TrustedKey,
        backup: TrustedKey,
    ) -> Result<Self, KeyChainError> {
        if generation == 0 {
            return Err(KeyChainError::BadKeysFile("generations start at 1".into()));
        }
        if main.id == backup.id {
            return Err(KeyChainError::BadKeysFile(
                "the main and backup keys are the same key".into(),
            ));
        }
        Ok(KeySet {
            generation,
            main,
            backup,
            revoked: Vec::new(),
        })
    }

    /// The generation compiled into this release.
    ///
    /// Until the maintainer's public keys are recorded, this refuses with
    /// [`KeyChainError::NoTrustRoot`]; a test builds its own with
    /// [`KeySet::new`] or [`KeySet::from_json`].
    pub fn official() -> Result<Self, KeyChainError> {
        match OFFICIAL_KEYS {
            Some(json) => KeySet::from_json(json.as_bytes()),
            None => Err(KeyChainError::NoTrustRoot),
        }
    }

    /// Read a `keys-N.json` document. This trusts it: use it for a
    /// generation compiled in or held locally, and [`KeySet::rotate`] for one
    /// fetched from the marketplace.
    pub fn from_json(bytes: &[u8]) -> Result<Self, KeyChainError> {
        let bad = |message: String| KeyChainError::BadKeysFile(message);
        let value = parse_json(bytes).map_err(bad)?;
        let map = value
            .as_object()
            .ok_or_else(|| bad("not a JSON object".into()))?;
        match map.get("schema_version").and_then(Value::as_u64) {
            Some(KEYS_SCHEMA_VERSION) => {}
            Some(other) => return Err(KeyChainError::UnsupportedSchema(other)),
            None => return Err(bad("schema_version: missing or not an integer".into())),
        }
        let generation = map
            .get("generation")
            .and_then(Value::as_u64)
            .ok_or_else(|| bad("generation: missing or not an integer".into()))?;
        let key = |which: &str| -> Result<TrustedKey, KeyChainError> {
            let entry = map
                .get(which)
                .and_then(Value::as_object)
                .ok_or_else(|| bad(format!("{which}: missing or not an object")))?;
            let encoded = entry
                .get("public_key")
                .and_then(Value::as_str)
                .ok_or_else(|| bad(format!("{which}.public_key: missing or not a string")))?;
            let declared = entry
                .get("id")
                .and_then(Value::as_str)
                .and_then(KeyId::parse)
                .ok_or_else(|| bad(format!("{which}.id: missing or not 16 hex digits")))?;
            let key = TrustedKey::from_base64(encoded)?;
            if key.id != declared {
                return Err(bad(format!(
                    "{which}.id is {declared}, but the key's own id is {}",
                    key.id
                )));
            }
            Ok(key)
        };
        let mut set = KeySet::new(generation, key("main")?, key("backup")?)?;
        if let Some(revoked) = map.get("revoked") {
            let list = revoked
                .as_array()
                .ok_or_else(|| bad("revoked: not an array".into()))?;
            for item in list {
                let id = item
                    .as_str()
                    .and_then(KeyId::parse)
                    .ok_or_else(|| bad("revoked: an entry is not 16 hex digits".into()))?;
                if id == set.main.id || id == set.backup.id {
                    return Err(bad(format!(
                        "revoked: {id} is one of this generation's keys"
                    )));
                }
                set.revoked.push(id);
            }
        }
        Ok(set)
    }

    /// This set as a `keys-N.json` document, so a client can store the
    /// newest generation it has verified.
    pub fn to_json(&self) -> String {
        let key =
            |k: &TrustedKey| serde_json::json!({"id": k.id.to_string(), "public_key": k.encoded});
        let value = serde_json::json!({
            "schema_version": KEYS_SCHEMA_VERSION,
            "generation": self.generation,
            "main": key(&self.main),
            "backup": key(&self.backup),
            "revoked": self.revoked.iter().map(KeyId::to_string).collect::<Vec<_>>(),
        });
        let mut text = serde_json::to_string_pretty(&value).unwrap_or_default();
        text.push('\n');
        text
    }

    /// Mark keys as revoked (they are refused wherever they sign).
    pub fn with_revoked(mut self, revoked: impl IntoIterator<Item = KeyId>) -> Self {
        self.revoked.extend(revoked);
        self
    }

    /// The generation number.
    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// The key that signs indexes.
    pub fn main(&self) -> &TrustedKey {
        &self.main
    }

    /// The key that signs only rotations.
    pub fn backup(&self) -> &TrustedKey {
        &self.backup
    }

    /// Keys this generation (or one before it) revoked.
    pub fn revoked(&self) -> &[KeyId] {
        &self.revoked
    }

    /// Accept the next generation against this one.
    ///
    /// The backup signature must be by this generation's backup key. With a
    /// main signature by this generation's main key it is a planned rotation
    /// (either key may change). Without one, the next generation must revoke
    /// this main key, replace it, and keep the backup key: the backup key
    /// cannot be replaced without the main key.
    pub fn rotate(&self, next: Rotation<'_>) -> Result<(KeySet, RotationKind), KeyChainError> {
        let generation = self.generation + 1;
        let comment = format!("{KEYS_COMMENT_PREFIX}{generation}");
        let signed = |which: &'static str, key: &TrustedKey, text: &str| {
            ParsedSignature::parse(text)
                .and_then(|signature| key.verify(next.keys_json, &signature, &comment))
                .map_err(|source| KeyChainError::Signature {
                    generation,
                    which,
                    source,
                })
        };
        signed("backup", &self.backup, next.backup_signature)?;
        if let Some(main) = next.main_signature {
            signed("main", &self.main, main)?;
        }
        let mut set = KeySet::from_json(next.keys_json)?;
        if set.generation != generation {
            return Err(KeyChainError::GenerationOrder {
                expected: generation,
                found: set.generation,
            });
        }
        // Revocations carry forward, and a revoked key never comes back.
        for id in &self.revoked {
            if !set.revoked.contains(id) {
                set.revoked.push(*id);
            }
        }
        for key in [&set.main, &set.backup] {
            if set.revoked.contains(&key.id) {
                return Err(KeyChainError::RevokedKey(key.id));
            }
        }
        let kind = if next.main_signature.is_some() {
            RotationKind::Planned
        } else {
            if !set.revoked.contains(&self.main.id) || set.main.id == self.main.id {
                return Err(KeyChainError::MainSignatureRequired { generation });
            }
            if set.backup.id != self.backup.id {
                return Err(KeyChainError::BackupReplacedWithoutMain { generation });
            }
            RotationKind::MainKeyReplaced {
                revoked: self.main.id,
            }
        };
        Ok((set, kind))
    }

    /// Accept each generation after this one in turn, and return the newest
    /// with how each was accepted.
    pub fn walk<'a>(
        &self,
        rotations: impl IntoIterator<Item = Rotation<'a>>,
    ) -> Result<(KeySet, Vec<RotationKind>), KeyChainError> {
        let mut current = self.clone();
        let mut kinds = Vec::new();
        for rotation in rotations {
            let (next, kind) = current.rotate(rotation)?;
            current = next;
            kinds.push(kind);
        }
        Ok((current, kinds))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_ids_print_as_minisign_does() {
        let id = KeyId::parse("8C6B1E0F2A9D4C31").unwrap();
        assert_eq!(id.to_string(), "8C6B1E0F2A9D4C31");
        assert_eq!(id.as_bytes()[0], 0x31);
        assert_eq!(KeyId::parse("8c6b1e0f2a9d4c31"), Some(id));
        assert!(KeyId::parse("8C6B1E0F2A9D4C3").is_none());
        assert!(KeyId::parse("+C6B1E0F2A9D4C31").is_none());
    }

    #[test]
    fn no_official_keys_until_recorded() {
        assert!(matches!(
            KeySet::official(),
            Err(KeyChainError::NoTrustRoot)
        ));
    }

    #[test]
    fn signatures_with_the_legacy_algorithm_are_refused() {
        // `Ed` (0x45 0x64) then a key id and 64 zero bytes.
        let mut raw = b"Ed".to_vec();
        raw.extend([0u8; 72]);
        let line = base64_encode(&raw);
        let text = format!(
            "untrusted comment: x\n{line}\ntrusted comment: y\n{}\n",
            base64_encode(&[0; 64])
        );
        assert!(matches!(
            ParsedSignature::parse(&text),
            Err(SignatureError::LegacyAlgorithm)
        ));
        let text = text.replacen(
            &line,
            &base64_encode(&[b"XX".as_slice(), &[0; 72]].concat()),
            1,
        );
        assert!(matches!(
            ParsedSignature::parse(&text),
            Err(SignatureError::UnsupportedAlgorithm)
        ));
    }

    fn base64_encode(bytes: &[u8]) -> String {
        const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut out = String::new();
        for chunk in bytes.chunks(3) {
            let n = chunk
                .iter()
                .enumerate()
                .fold(0u32, |n, (i, &b)| n | u32::from(b) << (16 - 8 * i));
            for i in 0..4 {
                if i <= chunk.len() {
                    out.push(ALPHABET[(n >> (18 - 6 * i) & 63) as usize] as char);
                } else {
                    out.push('=');
                }
            }
        }
        out
    }
}
