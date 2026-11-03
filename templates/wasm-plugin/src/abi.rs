//! The four exports a WASM plugin gives its host (`rich_plugin_api::abi::wasm`).
//! Nothing here needs changing: the plugin is `MANIFEST` and `call` in
//! `lib.rs`.
//!
//! - `memory`: the module's linear memory (Rust exports it);
//! - `rich_plugin_alloc(len) -> ptr`: room for the host to write an input;
//! - `rich_plugin_manifest() -> ptr << 32 | len`: where `MANIFEST` is;
//! - `rich_plugin_call(capability, ptr, len, width) -> ptr << 32 | len`: run
//!   a capability, with bit 63 set when the output is an error message.
//!
//! Every call runs in a fresh instance, so memory handed to the host is never
//! freed: the instance is dropped after the call.

// Natively (`cargo test`) only the tests use these.
#![cfg_attr(not(target_family = "wasm"), allow(dead_code))]

/// Set in a call's result when the output is an error message.
const ERROR_BIT: u64 = 1 << 63;

/// A `ptr << 32 | len` result. On wasm32 both fit in 32 bits.
fn pack(ptr: usize, len: usize) -> u64 {
    ((ptr as u64) << 32) | len as u64
}

/// Where `text` is, packed.
fn locate(text: &str) -> u64 {
    pack(text.as_ptr() as usize, text.len())
}

/// Hand `text` to the host: it stays in memory until the instance ends.
fn leak(text: String) -> u64 {
    locate(Box::leak(text.into_boxed_str()))
}

#[cfg(target_family = "wasm")]
mod exports {
    use super::{ERROR_BIT, leak, locate};

    #[unsafe(no_mangle)]
    extern "C" fn rich_plugin_alloc(len: u32) -> u32 {
        let buffer: &mut [u8] = Box::leak(vec![0; len as usize].into_boxed_slice());
        buffer.as_mut_ptr() as usize as u32
    }

    #[unsafe(no_mangle)]
    extern "C" fn rich_plugin_manifest() -> u64 {
        locate(crate::MANIFEST)
    }

    #[unsafe(no_mangle)]
    extern "C" fn rich_plugin_call(capability: u32, ptr: u32, len: u32, width: u32) -> u64 {
        // SAFETY: the host wrote `len` bytes at `ptr`, which
        // `rich_plugin_alloc(len)` returned in this instance and nothing
        // else uses.
        let bytes = unsafe { std::slice::from_raw_parts(ptr as usize as *const u8, len as usize) };
        let result = std::str::from_utf8(bytes)
            .map_err(|_| "the input is not UTF-8".to_string())
            .and_then(|input| crate::call(capability, input, width));
        match result {
            Ok(output) => leak(output),
            Err(message) => leak(message) | ERROR_BIT,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_result_packs_its_pointer_and_length() {
        assert_eq!(pack(0x1000, 5), 0x0000_1000_0000_0005);
        assert_eq!(pack(0x1000, 5) & ERROR_BIT, 0);
    }
}
