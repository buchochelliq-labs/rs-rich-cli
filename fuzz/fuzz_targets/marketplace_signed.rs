//! A fuzzed `index.json` and `keys-N.json`, each signed with the test keys,
//! so the parsers behind the signature check are reached: the index (then
//! each listing it names, read through it) and a key rotation.
#![no_main]

use std::sync::OnceLock;

use libfuzzer_sys::fuzz_target;
use rich_fuzz::{now, take_byte, test_keys, MAX_PARSE_INPUT};
use rich_marketplace::{
    verify_index, AcceptedIndex, KeySet, Rotation, INDEX_COMMENT_PREFIX, KEYS_COMMENT_PREFIX,
};

fn keys() -> &'static KeySet {
    static KEYS: OnceLock<KeySet> = OnceLock::new();
    KEYS.get_or_init(test_keys::generation_1)
}

fuzz_target!(|data: &[u8]| {
    // The first byte is the serial the signature claims, and whether an
    // earlier index was accepted.
    let (serial, rest) = take_byte(data);
    let rest = &rest[..rest.len().min(MAX_PARSE_INPUT)];
    let serial = u64::from(serial % 4) + 1;
    let signature = test_keys::main().sign(rest, &format!("{INDEX_COMMENT_PREFIX}{serial}"));
    let last = AcceptedIndex {
        serial: 2,
        sha256: String::new(),
    };
    let last = (data.first().copied().unwrap_or(0) & 0x80 != 0).then_some(&last);
    if let Ok(index) = verify_index(keys(), rest, &signature, last, now()) {
        for id in index.listings().keys() {
            let _ = index.read_listing(id, rest);
        }
    }

    let comment = format!("{KEYS_COMMENT_PREFIX}2");
    let main = test_keys::main().sign(rest, &comment);
    let backup = test_keys::backup().sign(rest, &comment);
    let _ = keys().rotate(Rotation {
        keys_json: rest,
        main_signature: Some(&main),
        backup_signature: &backup,
    });
});
