//! A fuzzed `.minisig` file against a real index and a real key rotation:
//! the signature parser and every check before the signed bytes are read.
#![no_main]

use std::sync::OnceLock;

use libfuzzer_sys::fuzz_target;
use rich_fuzz::{MAX_PARSE_INPUT, now, test_keys, text};
use rich_marketplace::{KeySet, Rotation, verify_index};

/// A valid generation-1 index, serial 1.
const INDEX: &[u8] = br#"{"schema_version":1,"serial":1,"generated":"2026-10-26T00:00:00Z","expires":"2099-01-01T00:00:00Z","listings":{}}"#;

fn keys() -> &'static KeySet {
    static KEYS: OnceLock<KeySet> = OnceLock::new();
    KEYS.get_or_init(test_keys::generation_1)
}

fuzz_target!(|data: &[u8]| {
    let signature = text(data, MAX_PARSE_INPUT);
    let _ = verify_index(keys(), INDEX, &signature, None, now());
    let _ = keys().rotate(Rotation {
        keys_json: INDEX,
        main_signature: Some(&signature),
        backup_signature: &signature,
    });
});
