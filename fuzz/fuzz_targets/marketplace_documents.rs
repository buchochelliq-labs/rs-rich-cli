//! rs-rich-marketplace's documents read as they arrive: a listing (then
//! version resolution against this host), a lockfile, a `keys-N.json`
//! file, a `minisign.pub` file and a key id.
#![no_main]

use libfuzzer_sys::fuzz_target;
use rich_fuzz::{text, MAX_PARSE_INPUT};
use rich_marketplace::{resolve, Host, KeyId, KeySet, Listing, Lockfile, Request, TrustedKey};

fuzz_target!(|data: &[u8]| {
    let data = &data[..data.len().min(MAX_PARSE_INPUT)];
    if let Ok(listing) = Listing::parse(data) {
        let _ = resolve(&listing, &Host::current(), &Request::Newest);
    }
    let _ = Lockfile::parse(data);
    let _ = KeySet::from_json(data);
    let source = text(data, MAX_PARSE_INPUT);
    let _ = TrustedKey::from_minisign_pub(&source);
    let _ = TrustedKey::from_base64(&source);
    let _ = KeyId::parse(&source);
});
