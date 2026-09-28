//! Native plugins through the C ABI in [`rich_plugin_api::abi`].
//!
//! Opening a library runs its initialisers, and every call runs its code in
//! this process: nothing here sandboxes it. What this module guarantees is
//! that only `repr(C)` data crosses the boundary, that a plugin built for
//! another ABI major is refused before its descriptor is read, and that the
//! library stays loaded as long as anything registered from it exists.

// Loading a library and calling through its function pointers is unsafe by
// nature; the checks are in `rich_plugin_api::abi::read_descriptor`.
#![allow(unsafe_code)]

use std::path::Path;
use std::sync::Arc;

use rich_plugin_api::abi::{
    read_descriptor, AbiOutput, AbiStr, PluginDescriptor, PluginVTable, DYLIB_ENTRY_SYMBOL,
    STATUS_OK,
};

use super::{AbiBackend, LoadError, RuntimeKind, RuntimePlugin, MAX_OUTPUT_BYTES};

/// A loaded library and its functions. The vtable's pointers are only valid
/// while `_library` is loaded, which is why they live together.
struct NativeBackend {
    vtable: PluginVTable,
    _library: libloading::Library,
}

impl AbiBackend for NativeBackend {
    fn call(&self, capability: usize, input: &str, width: u32) -> Result<String, String> {
        let mut output = AbiOutput::empty();
        // SAFETY: the vtable came from a descriptor whose ABI version matched,
        // the library is loaded, and `input` outlives the call.
        let status =
            unsafe { (self.vtable.call)(capability, AbiStr::new(input), width, &mut output) };
        let text = if output.ptr.is_null() {
            Err("the plugin returned no output".to_string())
        } else if output.len > MAX_OUTPUT_BYTES {
            Err(format!(
                "the plugin returned {} bytes; at most {MAX_OUTPUT_BYTES} are accepted",
                output.len
            ))
        } else {
            // SAFETY: the plugin hands over `len` initialised bytes at `ptr`.
            let bytes = unsafe { std::slice::from_raw_parts(output.ptr, output.len) };
            String::from_utf8(bytes.to_vec()).map_err(|_| "the plugin returned non-UTF-8".into())
        };
        if !output.ptr.is_null() {
            // SAFETY: the output came from this plugin's call; freed once.
            unsafe { (self.vtable.free)(output) };
        }
        match (status, text) {
            (STATUS_OK, text) => text,
            (_, Ok(message)) => Err(message),
            (_, Err(error)) => Err(error),
        }
    }
}

/// Load a native plugin (`.so`, `.dylib`, `.dll`).
///
/// **This runs the library's code.** Only load a path the user chose.
pub fn load_native(path: &Path) -> Result<RuntimePlugin, LoadError> {
    // Resolve the path first: a bare name would make the loader search the
    // system library paths instead of opening this file.
    let path = std::fs::canonicalize(path).map_err(|error| LoadError::Io {
        path: path.to_path_buf(),
        message: error.to_string(),
    })?;
    // SAFETY: loading runs the library's initialisers. The caller chose this
    // file; that is the trust decision (see docs/PLUGINS.md).
    let library = unsafe { libloading::Library::new(&path) }.map_err(|error| LoadError::Io {
        path: path.clone(),
        message: error.to_string(),
    })?;
    let descriptor = {
        // SAFETY: the symbol's type is the ABI's entry point type.
        let entry: libloading::Symbol<unsafe extern "C" fn() -> *const PluginDescriptor> = unsafe {
            library.get(DYLIB_ENTRY_SYMBOL.as_bytes())
        }
        .map_err(|_| LoadError::NotAPlugin {
            path: path.clone(),
            message: format!("it does not export {DYLIB_ENTRY_SYMBOL}"),
        })?;
        // SAFETY: the entry point takes nothing and returns a descriptor.
        unsafe { entry() }
    };
    // SAFETY: the descriptor points into the library, which is still loaded.
    let (abi, vtable) = unsafe { read_descriptor(descriptor) }.map_err(|error| LoadError::Abi {
        path: path.clone(),
        error,
    })?;
    let backend = NativeBackend {
        vtable,
        _library: library,
    };
    RuntimePlugin::new(abi, RuntimeKind::Native, path, Arc::new(backend))
}
