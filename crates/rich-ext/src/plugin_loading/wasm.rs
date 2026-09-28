//! Sandboxed WASM plugins, run by `wasmi` (a pure-Rust interpreter).
//!
//! The host gives a module nothing: it may not import a single function, so
//! it has no WASI, no file system, no network, no clock and no randomness. It
//! sees only the text passed in, through its own linear memory. Every call
//! runs in a fresh instance with a fuel budget (roughly, an instruction
//! count) and a memory cap, so a plugin that loops forever or allocates
//! without bound is stopped, and no state leaks from one call to the next.
//!
//! The exports it must provide are listed in [`rich_plugin_api::abi::wasm`].

use std::path::Path;
use std::sync::Arc;

use rich_plugin_api::abi::{wasm as names, PluginAbi};
use wasmi::{
    Config, EnforcedLimits, Engine, ExternType, Instance, Linker, Memory, Module, Store,
    StoreLimits, StoreLimitsBuilder, TrapCode,
};

use super::{AbiBackend, LoadError, RuntimeKind, RuntimePlugin, WasmLimits, MAX_OUTPUT_BYTES};

/// Bytes in a WASM page.
const PAGE: u64 = 64 * 1024;

struct WasmBackend {
    engine: Engine,
    module: Module,
    limits: WasmLimits,
}

/// Why a call failed, before it is turned into text.
enum CallError {
    OutOfFuel,
    Memory(String),
    Other(String),
}

impl CallError {
    fn message(self, limits: &WasmLimits) -> String {
        match self {
            CallError::OutOfFuel => format!(
                "the plugin ran out of fuel (the limit is {} units per call)",
                limits.fuel
            ),
            CallError::Memory(message) | CallError::Other(message) => message,
        }
    }
}

fn classify(error: wasmi::Error) -> CallError {
    match error.as_trap_code() {
        Some(TrapCode::OutOfFuel) => CallError::OutOfFuel,
        Some(TrapCode::GrowthOperationLimited) => CallError::Memory(
            "the plugin tried to use more memory than the limit allows".to_string(),
        ),
        _ => CallError::Other(format!("the plugin failed: {error}")),
    }
}

impl WasmBackend {
    /// A fresh store and instance with this call's fuel and memory limits.
    fn instantiate(&self) -> Result<(Store<StoreLimits>, Instance, Memory), CallError> {
        let limits = StoreLimitsBuilder::new()
            .memory_size(self.limits.memory_bytes)
            .memories(1)
            .tables(1)
            .instances(1)
            .trap_on_grow_failure(true)
            .build();
        let mut store = Store::new(&self.engine, limits);
        store.limiter(|limits| limits);
        store
            .set_fuel(self.limits.fuel)
            .map_err(|e| CallError::Other(e.to_string()))?;
        let linker = Linker::<StoreLimits>::new(&self.engine);
        let instance = linker
            .instantiate_and_start(&mut store, &self.module)
            .map_err(classify)?;
        let memory = instance
            .get_memory(&store, names::MEMORY)
            .ok_or_else(|| CallError::Other(format!("it does not export `{}`", names::MEMORY)))?;
        Ok((store, instance, memory))
    }

    /// Read `len` bytes at `ptr` from a packed `ptr << 32 | len` result.
    fn read_packed(
        store: &Store<StoreLimits>,
        memory: &Memory,
        packed: u64,
    ) -> Result<String, CallError> {
        let ptr = ((packed >> 32) & 0x7fff_ffff) as usize;
        let len = (packed & 0xffff_ffff) as usize;
        if len > MAX_OUTPUT_BYTES {
            return Err(CallError::Other(format!(
                "the plugin returned {len} bytes; at most {MAX_OUTPUT_BYTES} are accepted"
            )));
        }
        let bytes = memory
            .data(store)
            .get(ptr..ptr.saturating_add(len))
            .ok_or_else(|| {
                CallError::Other("the plugin returned a range outside its memory".into())
            })?;
        String::from_utf8(bytes.to_vec())
            .map_err(|_| CallError::Other("the plugin returned non-UTF-8".into()))
    }

    fn manifest(&self) -> Result<String, CallError> {
        let (mut store, instance, memory) = self.instantiate()?;
        let manifest = instance
            .get_typed_func::<(), i64>(&store, names::MANIFEST)
            .map_err(|_| missing(names::MANIFEST, "() -> i64"))?;
        let packed = manifest.call(&mut store, ()).map_err(classify)? as u64;
        Self::read_packed(&store, &memory, packed & !names::ERROR_BIT)
    }

    fn run(&self, capability: usize, input: &str, width: u32) -> Result<String, CallError> {
        let (mut store, instance, memory) = self.instantiate()?;
        let alloc = instance
            .get_typed_func::<i32, i32>(&store, names::ALLOC)
            .map_err(|_| missing(names::ALLOC, "(i32) -> i32"))?;
        let call = instance
            .get_typed_func::<(i32, i32, i32, i32), i64>(&store, names::CALL)
            .map_err(|_| missing(names::CALL, "(i32, i32, i32, i32) -> i64"))?;
        let too_big = || CallError::Memory("the input is larger than the plugin can accept".into());
        let len = i32::try_from(input.len()).map_err(|_| too_big())?;
        let ptr = alloc.call(&mut store, len).map_err(classify)?;
        let start = usize::try_from(ptr).map_err(|_| too_big())?;
        memory
            .write(&mut store, start, input.as_bytes())
            .map_err(|_| {
                CallError::Other("the plugin's allocation is outside its memory".into())
            })?;
        let capability = i32::try_from(capability).unwrap_or(i32::MAX);
        let width = i32::try_from(width).unwrap_or(i32::MAX);
        let packed = call
            .call(&mut store, (capability, ptr, len, width))
            .map_err(classify)? as u64;
        let text = Self::read_packed(&store, &memory, packed & !names::ERROR_BIT)?;
        if packed & names::ERROR_BIT != 0 {
            Err(CallError::Other(text))
        } else {
            Ok(text)
        }
    }
}

fn missing(name: &str, signature: &str) -> CallError {
    CallError::Other(format!("it does not export `{name}` as {signature}"))
}

impl AbiBackend for WasmBackend {
    fn call(&self, capability: usize, input: &str, width: u32) -> Result<String, String> {
        self.run(capability, input, width)
            .map_err(|error| error.message(&self.limits))
    }
}

/// Load a WASM plugin with these limits.
///
/// Refused when the file is too large or not valid WASM, when the module
/// imports anything, when its memory starts above the cap, and when its
/// manifest cannot be read within the limits or is not a valid
/// [`PluginAbi`].
pub fn load_wasm(path: &Path, limits: &WasmLimits) -> Result<RuntimePlugin, LoadError> {
    let io = |message: String| LoadError::Io {
        path: path.to_path_buf(),
        message,
    };
    let not_a_plugin = |message: String| LoadError::NotAPlugin {
        path: path.to_path_buf(),
        message,
    };
    let limit = |message: String| LoadError::Limit {
        path: path.to_path_buf(),
        message,
    };
    let size = std::fs::metadata(path)
        .map_err(|e| io(e.to_string()))?
        .len();
    if size > limits.module_bytes {
        return Err(limit(format!(
            "the module is {size} bytes; at most {} are accepted",
            limits.module_bytes
        )));
    }
    let bytes = std::fs::read(path).map_err(|e| io(e.to_string()))?;

    let mut config = Config::default();
    config
        .consume_fuel(true)
        .enforced_limits(EnforcedLimits::strict());
    let engine = Engine::new(&config);
    let module =
        Module::new(&engine, &bytes).map_err(|e| not_a_plugin(format!("invalid WASM: {e}")))?;
    if let Some(import) = module.imports().next() {
        return Err(not_a_plugin(format!(
            "it imports `{}.{}`, but the host provides nothing to import (no WASI, no host \
             functions)",
            import.module(),
            import.name()
        )));
    }
    for export in module.exports() {
        if let ExternType::Memory(memory) = export.ty() {
            let bytes = memory.minimum().saturating_mul(PAGE);
            if bytes > limits.memory_bytes as u64 {
                return Err(limit(format!(
                    "its memory starts at {bytes} bytes, over the limit of {} bytes",
                    limits.memory_bytes
                )));
            }
        }
    }

    let backend = WasmBackend {
        engine,
        module,
        limits: *limits,
    };
    let manifest = backend.manifest().map_err(|error| match error {
        CallError::OutOfFuel | CallError::Memory(_) => limit(error.message(limits)),
        CallError::Other(message) => not_a_plugin(message),
    })?;
    let abi = PluginAbi::parse_manifest(&manifest).map_err(|error| LoadError::Abi {
        path: path.to_path_buf(),
        error,
    })?;
    RuntimePlugin::new(abi, RuntimeKind::Wasm, path, Arc::new(backend))
}
