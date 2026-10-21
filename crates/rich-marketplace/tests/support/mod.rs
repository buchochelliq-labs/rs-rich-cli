//! The fixture marketplace: test keys checked into `tests/fixtures/keys`,
//! and a marketplace built and signed from them in memory.
//!
//! The keys are unencrypted minisign pairs, made by
//! `generate_test_keys` in `tests/marketplace.rs`:
//!
//! ```bash
//! cargo test -p rs-rich-marketplace --test marketplace -- --ignored generate_test_keys
//! ```
//!
//! (`minisign -G -W -p NAME.pub -s NAME.key` makes the same files.) They
//! are trusted only by these tests, through explicit [`KeySet`]s; no build
//! of the crate trusts them by default.

#![allow(dead_code)]

use std::io::{Cursor, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use rich_marketplace::{KeySet, MemoryTransport, TrustedKey};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

/// The pairs in `tests/fixtures/keys`.
pub const KEY_NAMES: [&str; 5] = [
    "gen1-main",
    "gen1-backup",
    "gen2-main",
    "gen2-backup",
    "stranger",
];

pub fn keys_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/keys")
}

/// One test key pair.
pub struct Pair {
    pk: minisign::PublicKey,
    sk: minisign::SecretKey,
    pub trusted: TrustedKey,
}

pub fn pair(name: &str) -> Pair {
    let dir = keys_dir();
    let public = std::fs::read_to_string(dir.join(format!("{name}.pub"))).expect("test public key");
    let secret = std::fs::read_to_string(dir.join(format!("{name}.key"))).expect("test secret key");
    let pk = minisign::PublicKeyBox::from_string(&public)
        .and_then(minisign::PublicKey::from_box)
        .expect("a minisign public key");
    let sk = minisign::SecretKeyBox::from_string(&secret)
        .and_then(minisign::SecretKey::from_unencrypted_box)
        .expect("an unencrypted minisign secret key");
    let trusted = TrustedKey::from_minisign_pub(&public).expect("a trusted key");
    Pair { pk, sk, trusted }
}

impl Pair {
    /// A `.minisig` over `data` with this trusted comment.
    pub fn sign(&self, data: &[u8], comment: &str) -> String {
        minisign::sign(
            Some(&self.pk),
            &self.sk,
            Cursor::new(data),
            Some(comment),
            Some("signature from the rs-rich-marketplace test key"),
        )
        .expect("signing")
        .to_string()
    }
}

/// Generation 1: the test main and backup keys.
pub fn generation_1() -> KeySet {
    KeySet::new(1, pair("gen1-main").trusted, pair("gen1-backup").trusted).unwrap()
}

/// A `keys-N.json` document.
pub fn keys_json(generation: u64, main: &Pair, backup: &Pair, revoked: &[&Pair]) -> Vec<u8> {
    let key =
        |p: &Pair| json!({"id": p.trusted.id().to_string(), "public_key": p.trusted.to_base64()});
    let revoked: Vec<String> = revoked.iter().map(|p| p.trusted.id().to_string()).collect();
    serde_json::to_vec_pretty(&json!({
        "schema_version": 1,
        "generation": generation,
        "main": key(main),
        "backup": key(backup),
        "revoked": revoked,
    }))
    .unwrap()
}

pub fn sha256(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// 2026-10-21T00:00:00Z, the fixture's "now".
pub fn now() -> SystemTime {
    UNIX_EPOCH + Duration::from_secs(1_792_540_800)
}

/// 2026-11-02T00:00:00Z, after the fixture index expires.
pub fn after_expiry() -> SystemTime {
    UNIX_EPOCH + Duration::from_secs(1_793_577_600)
}

/// A fresh, empty directory under Cargo's per-test temporary directory.
pub fn temp_dir(name: &str) -> PathBuf {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!(
        "marketplace-{name}-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

pub const BASE: &str = "https://example.test/releases";

/// The `shout` module: a WASM header and a custom section, enough for the
/// built-in check.
pub fn shout_wasm(version: &str) -> Vec<u8> {
    let mut bytes = b"\0asm\x01\0\0\0".to_vec();
    let name = b"shout";
    let payload_len = 1 + name.len() + version.len();
    bytes.push(0); // custom section
    bytes.push(payload_len as u8);
    bytes.push(name.len() as u8);
    bytes.extend_from_slice(name);
    bytes.extend_from_slice(version.as_bytes());
    bytes
}

/// A micro pack archive whose `pack.json` names `name` and `version`.
pub fn pack_zip(name: &str, version: &str) -> Vec<u8> {
    let mut out = Cursor::new(Vec::new());
    {
        let mut zip = zip::ZipWriter::new(&mut out);
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);
        zip.start_file("pack.json", options).unwrap();
        let pack = json!({"schema_version": 1, "name": name, "version": version, "packages": []});
        zip.write_all(pack.to_string().as_bytes()).unwrap();
        zip.finish().unwrap();
    }
    out.into_inner()
}

pub const THEME: &[u8] = b"[styles]\nwarning = bold red\ninfo = dim cyan\n";

fn version(
    number: &str,
    requires: Value,
    url: String,
    body: &[u8],
    capabilities: Value,
    permissions: Value,
) -> Value {
    json!({
        "version": number,
        "published": "2026-10-15",
        "requires": requires,
        "artifact": {"url": url, "sha256": sha256(body), "size": body.len()},
        "capabilities": capabilities,
        "permissions": permissions,
    })
}

fn wasm_version(number: &str, abi: &str, capabilities: Value, body: &[u8]) -> Value {
    let mut v = version(
        number,
        json!({"abi": abi}),
        format!("{BASE}/shout-{number}.wasm"),
        body,
        capabilities,
        json!([]),
    );
    v["source_revision"] = json!("e26c737c5e9741d94abfea61e9d2065c6b59f2e5");
    v
}

/// The fixture marketplace's files, by path, and the artifacts it points to.
pub struct Marketplace {
    pub files: Vec<(String, Vec<u8>)>,
    pub transport: MemoryTransport,
}

impl Marketplace {
    /// `shout` (a WASM plugin with an unknown-capability 0.4.0, an ABI 2
    /// 0.3.0, a yanked 0.2.1 and a good 0.2.0), `status` (a micro pack),
    /// `solarized` (a theme) and `mermaid` (a crate, listed only).
    pub fn build() -> Self {
        let mut transport = MemoryTransport::new();
        let upper = json!([{"kind": "transform", "name": "upper"}]);
        let shout_versions: Vec<Value> = [
            ("0.4.0", "1.1", json!([{"kind": "transform", "name": "upper"}, {"kind": "fence-svg", "name": "shout"}])),
            ("0.3.0", "2.0", upper.clone()),
            ("0.2.1", "1.0", upper.clone()),
            ("0.2.0", "1.0", upper.clone()),
        ]
        .into_iter()
        .map(|(number, abi, capabilities)| {
            let body = shout_wasm(number);
            transport.insert(format!("{BASE}/shout-{number}.wasm"), body.clone());
            let mut v = wasm_version(number, abi, capabilities, &body);
            if number == "0.2.1" {
                v["yanked"] = json!(true);
                v["yank_reason"] = json!("Drops combining marks; use 0.2.0.");
            }
            v
        })
        .collect();
        let shout = json!({
            "schema_version": 1, "id": "shout", "kind": "wasm-plugin", "name": "Shout",
            "description": "Upper-cases text.", "tier": "community", "license": "MIT",
            "repository": "https://github.com/example/rich-shout",
            "versions": shout_versions,
        });

        let pack = pack_zip("status", "1.0.0");
        transport.insert(format!("{BASE}/status-1.0.0.zip"), pack.clone());
        let status = json!({
            "schema_version": 1, "id": "status", "kind": "micro-pack", "name": "Status icons",
            "description": "Success and warning icons.", "tier": "reviewed", "license": "CC0-1.0",
            "repository": "https://github.com/example/status-icons",
            "versions": [version("1.0.0", json!({"micro_schema": 1}), format!("{BASE}/status-1.0.0.zip"),
                &pack, json!([{"kind": "asset", "name": "status/success"}]), json!([]))],
        });

        transport.insert(format!("{BASE}/solarized.ini"), THEME.to_vec());
        let solarized = json!({
            "schema_version": 1, "id": "solarized", "kind": "theme", "name": "Solarized",
            "description": "Solarized styles.", "tier": "community", "license": "MIT",
            "repository": "https://github.com/example/rich-solarized",
            "versions": [version("1.2.0", json!({"theme_format": 1}), format!("{BASE}/solarized.ini"),
                THEME, json!([{"kind": "theme", "name": "solarized"}]), json!([]))],
        });

        let mermaid = json!({
            "schema_version": 1, "id": "mermaid", "kind": "crate", "name": "Mermaid diagrams",
            "description": "Mermaid in the terminal.", "tier": "official", "license": "MIT",
            "repository": "https://github.com/buchochelliq-labs/rs-rich-cli",
            "crate": {"name": "rs-rich-mermaid"},
            "versions": [{
                "version": "0.0.7", "published": "2026-10-20", "requires": {"plugin_api": 1},
                "artifact": {"url": "https://static.crates.io/crates/rs-rich-mermaid/rs-rich-mermaid-0.0.7.crate",
                             "sha256": "6ca5b3f974d2ead0191b462b81164c750a3e03c6629f9a5783fb819d43a0e9ac", "size": 98304},
                "capabilities": [{"kind": "fence-renderer", "name": "mermaid"}],
                "permissions": ["native-code"]
            }],
        });

        let files = [
            ("shout", shout),
            ("status", status),
            ("solarized", solarized),
            ("mermaid", mermaid),
        ]
        .into_iter()
        .map(|(id, listing)| {
            (
                format!("packages/{id}.json"),
                serde_json::to_vec_pretty(&listing).unwrap(),
            )
        })
        .collect();
        Marketplace { files, transport }
    }

    pub fn listing(&self, id: &str) -> &[u8] {
        let path = format!("packages/{id}.json");
        &self.files.iter().find(|(p, _)| *p == path).unwrap().1
    }

    /// `index.json` for these listings, with `serial`.
    pub fn index(&self, serial: u64) -> Vec<u8> {
        let mut listings = serde_json::Map::new();
        for (path, bytes) in &self.files {
            let id = path
                .trim_start_matches("packages/")
                .trim_end_matches(".json");
            listings.insert(
                id.to_string(),
                json!({"path": path, "sha256": sha256(bytes)}),
            );
        }
        serde_json::to_vec_pretty(&json!({
            "schema_version": 1,
            "serial": serial,
            "generated": "2026-10-01T00:00:00Z",
            "expires": "2026-11-01T00:00:00Z",
            "listings": listings,
        }))
        .unwrap()
    }
}

/// `index.json.minisig` for `index` at `serial`, by `key`.
pub fn sign_index(key: &Pair, index: &[u8], serial: u64) -> String {
    key.sign(index, &format!("rs-rich-marketplace index serial={serial}"))
}
