//! The plugin ABI's manifest, as a WASM or native plugin returns it:
//! parsed and validated, and a manifest that parses reads back the same
//! after `to_manifest`.
#![no_main]

use libfuzzer_sys::fuzz_target;
use rich_fuzz::{text, MAX_PARSE_INPUT};
use rich_plugin_api::abi::PluginAbi;

fuzz_target!(|data: &[u8]| {
    if let Ok(abi) = PluginAbi::parse_manifest(&text(data, MAX_PARSE_INPUT)) {
        let again = PluginAbi::parse_manifest(&abi.to_manifest());
        assert_eq!(again.as_ref(), Ok(&abi), "a parsed manifest round-trips");
    }
});
