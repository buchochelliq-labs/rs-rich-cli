//! A downloaded artifact's checks in rs-rich-marketplace: a micro pack's
//! zip archive and its `pack.json`, a theme file, and a WASM header.
#![no_main]

use libfuzzer_sys::fuzz_target;
use rich_fuzz::MAX_PARSE_INPUT;
use rich_marketplace::{check_pack_json, check_theme, check_wasm_header};

fuzz_target!(|data: &[u8]| {
    let data = &data[..data.len().min(MAX_PARSE_INPUT)];
    let _ = check_pack_json(data, "status", "1.0.0");
    let _ = check_theme(data);
    let _ = check_wasm_header(data);
});
