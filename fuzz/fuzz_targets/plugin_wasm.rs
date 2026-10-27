//! A WASM plugin module loaded by rs-rich-ext with the default limits
//! (wasmi, fuel and memory caps, no imports), and, when it loads, each of
//! its capabilities called: the ABI's inputs are what the module returns.
#![no_main]

use std::path::Path;

use libfuzzer_sys::fuzz_target;
use rich_ext::plugin_loading::{load_wasm_bytes, WasmLimits};

/// Smaller than the default 50 million so a looping module is stopped in
/// well under a second; every other limit is the default.
const FUEL: u64 = 5_000_000;

fuzz_target!(|data: &[u8]| {
    let limits = WasmLimits {
        fuel: FUEL,
        ..WasmLimits::default()
    };
    if let Ok(plugin) = load_wasm_bytes(data, Path::new("fuzz.wasm"), &limits) {
        for capability in 0..plugin.abi().capabilities.len() {
            let _ = plugin.call(capability, "fuzz input", 40);
        }
    }
});
