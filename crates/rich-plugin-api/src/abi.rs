//! The runtime plugin ABI: what a native (`dylib`) or WASM plugin exports,
//! and the one description, [`PluginAbi`], both loaders decode it into.
//!
//! Rust's own ABI is unstable, so nothing Rust-specific crosses the
//! boundary: a runtime plugin exchanges UTF-8 text with its host and nothing
//! else. What it may contribute is deliberately small ([`CapabilityKind`]):
//!
//! | kind | input | output |
//! |---|---|---|
//! | `transform` | plain text | plain text |
//! | `highlighter` | plain text | style spans, one `START END STYLE` per line |
//! | `fence-markup` | a Markdown fence's code | `rich` console markup |
//! | `fence-ansi` | a Markdown fence's code | text with ANSI SGR styling |
//!
//! Every call also receives the width available, in cells. The host sanitizes
//! every output before it reaches a console: terminal controls are made
//! visible, and only `fence-ansi` keeps SGR styling, through the same
//! sanitizer `rich view` uses.
//!
//! **Native plugins** export one C function, [`DYLIB_ENTRY_SYMBOL`], returning
//! a [`PluginDescriptor`]. Write it with [`export_dylib_plugin!`](crate::export_dylib_plugin)
//! rather than by hand. **WASM plugins** export a manifest in the text form
//! of [`PluginAbi`] (see [`PluginAbi::to_manifest`]) and a call function; the
//! exports are listed under [`wasm`].
//!
//! Versioning: [`ABI_MAJOR`] changes with any incompatible change, and a host
//! refuses a plugin built for another major. A newer minor loads when it uses
//! nothing the host lacks (an unknown capability kind is refused).

// The C ABI needs raw pointers, `no_mangle` and `extern "C"`; everything
// unsafe in this crate is here.
#![allow(unsafe_code)]

use std::fmt;
use std::mem::ManuallyDrop;
use std::panic::{catch_unwind, AssertUnwindSafe};

use crate::is_valid_name;

/// The ABI's major version. A host refuses a plugin with another.
pub const ABI_MAJOR: u32 = 1;
/// The ABI's minor version: additions a host of the same major may lack.
pub const ABI_MINOR: u32 = 0;

/// The function a native plugin exports: `extern "C" fn() -> *const PluginDescriptor`.
pub const DYLIB_ENTRY_SYMBOL: &str = "rich_plugin_entry";

/// The most capabilities one runtime plugin may declare.
pub const MAX_CAPABILITIES: usize = 64;
/// The longest version or description string accepted, in bytes.
pub const MAX_FIELD_LEN: usize = 1024;

/// The call succeeded; the output is the result.
pub const STATUS_OK: u32 = 0;
/// The call failed; the output is an error message.
pub const STATUS_ERROR: u32 = 1;

/// What a runtime plugin can contribute. The subset is chosen so that every
/// contribution is text in and text out, which the host can check.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[non_exhaustive]
pub enum CapabilityKind {
    /// Plain text in, plain text out: a named `TextTransform`. Styles on the
    /// input are not passed, and the output is unstyled.
    Transform,
    /// Plain text in, style spans out: a regex-style `Highlighter`. Each
    /// output line is `START END STYLE`: byte offsets into the UTF-8 input,
    /// on character boundaries, with `END` exclusive, and `STYLE` a `rich`
    /// style such as `bold red`. A span the host cannot apply is skipped.
    Highlighter,
    /// A Markdown fence's code in, `rich` markup out (`[bold]x[/]`).
    FenceMarkup,
    /// A Markdown fence's code in, ANSI-styled text out. Only SGR (colour and
    /// attribute) sequences survive the host's sanitizer.
    FenceAnsi,
}

impl CapabilityKind {
    /// Every kind this host understands.
    pub const ALL: [CapabilityKind; 4] = [
        CapabilityKind::Transform,
        CapabilityKind::Highlighter,
        CapabilityKind::FenceMarkup,
        CapabilityKind::FenceAnsi,
    ];

    /// The number used in [`AbiCapability::kind`].
    pub const fn code(self) -> u32 {
        match self {
            CapabilityKind::Transform => 1,
            CapabilityKind::Highlighter => 2,
            CapabilityKind::FenceMarkup => 3,
            CapabilityKind::FenceAnsi => 4,
        }
    }

    /// The kind for a [`code`](Self::code), if this host knows it.
    pub fn from_code(code: u32) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| kind.code() == code)
    }

    /// The name used in a manifest (`fence-markup`).
    pub const fn as_str(self) -> &'static str {
        match self {
            CapabilityKind::Transform => "transform",
            CapabilityKind::Highlighter => "highlighter",
            CapabilityKind::FenceMarkup => "fence-markup",
            CapabilityKind::FenceAnsi => "fence-ansi",
        }
    }

    /// The kind a manifest name spells, if this host knows it.
    pub fn parse(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| kind.as_str() == name)
    }

    /// Whether the output may keep ANSI styling (after sanitizing).
    pub const fn produces_ansi(self) -> bool {
        matches!(self, CapabilityKind::FenceAnsi)
    }
}

impl fmt::Display for CapabilityKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// One capability a runtime plugin declares.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AbiCapability {
    pub kind: CapabilityKind,
    /// The capability's name; for a fence, the language (`mermaid`).
    pub name: String,
}

/// A runtime plugin's self-description, decoded from a native plugin's
/// [`PluginDescriptor`] or a WASM plugin's manifest. Both loaders produce this
/// one type, and one host adapter turns it into registry capabilities.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PluginAbi {
    pub abi_major: u32,
    pub abi_minor: u32,
    /// The plugin id: lowercase letters, digits, `-`, `_` and `.`.
    pub name: String,
    pub version: String,
    pub description: String,
    /// In declaration order; a call names a capability by its index here.
    pub capabilities: Vec<AbiCapability>,
}

/// Why a runtime plugin's description was refused.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AbiError {
    /// Built for another major version of this ABI.
    Incompatible { major: u32, minor: u32 },
    /// The description is malformed; the message says how.
    Invalid(String),
}

impl fmt::Display for AbiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AbiError::Incompatible { major, minor } => write!(
                f,
                "built for plugin ABI {major}.{minor}, but this host supports ABI \
                 {ABI_MAJOR}.x; rebuild it against rs-rich-plugin-api with ABI {ABI_MAJOR}"
            ),
            AbiError::Invalid(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for AbiError {}

/// The first line of a manifest: `rich-plugin-abi MAJOR.MINOR`.
const MANIFEST_MAGIC: &str = "rich-plugin-abi";

impl PluginAbi {
    /// A description for this ABI version, with no capabilities.
    pub fn new(name: impl Into<String>, version: impl Into<String>) -> Self {
        PluginAbi {
            abi_major: ABI_MAJOR,
            abi_minor: ABI_MINOR,
            name: name.into(),
            version: version.into(),
            description: String::new(),
            capabilities: Vec::new(),
        }
    }

    /// Check the version and every field a host relies on.
    pub fn validate(&self) -> Result<(), AbiError> {
        if self.abi_major != ABI_MAJOR {
            return Err(AbiError::Incompatible {
                major: self.abi_major,
                minor: self.abi_minor,
            });
        }
        if !is_valid_name(&self.name) {
            return Err(AbiError::Invalid(format!(
                "invalid plugin name {:?}: use lowercase letters, digits, '-', '_' and '.'",
                self.name
            )));
        }
        for (field, value) in [
            ("version", &self.version),
            ("description", &self.description),
        ] {
            if value.len() > MAX_FIELD_LEN || value.chars().any(char::is_control) {
                return Err(AbiError::Invalid(format!(
                    "the {field} must be one line of at most {MAX_FIELD_LEN} bytes"
                )));
            }
        }
        if self.version.is_empty() {
            return Err(AbiError::Invalid("the version is empty".into()));
        }
        if self.capabilities.len() > MAX_CAPABILITIES {
            return Err(AbiError::Invalid(format!(
                "{} capabilities declared; at most {MAX_CAPABILITIES} are allowed",
                self.capabilities.len()
            )));
        }
        for (i, capability) in self.capabilities.iter().enumerate() {
            if !is_valid_name(&capability.name) {
                return Err(AbiError::Invalid(format!(
                    "invalid {} name {:?}",
                    capability.kind, capability.name
                )));
            }
            if self.capabilities[..i].contains(capability) {
                return Err(AbiError::Invalid(format!(
                    "{} {:?} is declared twice",
                    capability.kind, capability.name
                )));
            }
        }
        Ok(())
    }

    /// The manifest text form, which a WASM plugin returns from
    /// [`wasm::MANIFEST`]:
    ///
    /// ```text
    /// rich-plugin-abi 1.0
    /// name shout
    /// version 0.1.0
    /// description Upper-cases text
    /// capability transform upper
    /// capability fence-markup shout
    /// ```
    pub fn to_manifest(&self) -> String {
        let mut out = format!(
            "{MANIFEST_MAGIC} {}.{}\nname {}\nversion {}\n",
            self.abi_major, self.abi_minor, self.name, self.version
        );
        if !self.description.is_empty() {
            out.push_str(&format!("description {}\n", self.description));
        }
        for capability in &self.capabilities {
            out.push_str(&format!(
                "capability {} {}\n",
                capability.kind, capability.name
            ));
        }
        out
    }

    /// Parse and [validate](Self::validate) a manifest. Unknown keys are
    /// ignored (a newer minor may add some); an unknown capability kind is
    /// refused, because the host could not honour it.
    pub fn parse_manifest(text: &str) -> Result<PluginAbi, AbiError> {
        let mut lines = text.lines();
        let first = lines.next().unwrap_or_default();
        let version = first
            .strip_prefix(MANIFEST_MAGIC)
            .and_then(|rest| rest.strip_prefix(' '))
            .ok_or_else(|| {
                AbiError::Invalid(format!(
                    "the manifest must start with `{MANIFEST_MAGIC} MAJOR.MINOR`"
                ))
            })?;
        let (major, minor) = version
            .split_once('.')
            .and_then(|(major, minor)| Some((major.parse().ok()?, minor.parse().ok()?)))
            .ok_or_else(|| AbiError::Invalid(format!("invalid ABI version {version:?}")))?;
        if major != ABI_MAJOR {
            return Err(AbiError::Incompatible { major, minor });
        }
        let mut abi = PluginAbi {
            abi_major: major,
            abi_minor: minor,
            name: String::new(),
            version: String::new(),
            description: String::new(),
            capabilities: Vec::new(),
        };
        for line in lines {
            if line.trim().is_empty() {
                continue;
            }
            let (key, value) = line.split_once(' ').unwrap_or((line, ""));
            match key {
                "name" => abi.name = value.to_string(),
                "version" => abi.version = value.to_string(),
                "description" => abi.description = value.to_string(),
                "capability" => {
                    let (kind, name) = value.split_once(' ').ok_or_else(|| {
                        AbiError::Invalid(format!("expected `capability KIND NAME`: {line:?}"))
                    })?;
                    let kind = CapabilityKind::parse(kind).ok_or_else(|| {
                        AbiError::Invalid(format!(
                            "unknown capability kind {kind:?}; this host knows {}",
                            CapabilityKind::ALL.map(CapabilityKind::as_str).join(", ")
                        ))
                    })?;
                    abi.capabilities.push(AbiCapability {
                        kind,
                        name: name.to_string(),
                    });
                }
                _ => {}
            }
        }
        abi.validate()?;
        Ok(abi)
    }
}

/// The export names of a WASM plugin. A module must export all of them and
/// import nothing:
///
/// - `memory`: its linear memory;
/// - `rich_plugin_alloc(len: i32) -> i32`: room for `len` bytes of input;
/// - `rich_plugin_manifest() -> i64`: where its manifest is, packed as
///   `ptr << 32 | len`;
/// - `rich_plugin_call(capability: i32, ptr: i32, len: i32, width: i32) -> i64`:
///   run capability number `capability` (its index in the manifest) on the
///   input at `ptr`, returning the output packed like the manifest, with bit
///   63 set when the output is an error message instead.
pub mod wasm {
    pub const MEMORY: &str = "memory";
    pub const ALLOC: &str = "rich_plugin_alloc";
    pub const MANIFEST: &str = "rich_plugin_manifest";
    pub const CALL: &str = "rich_plugin_call";
    /// Set in a call's result when the output is an error message.
    pub const ERROR_BIT: u64 = 1 << 63;
}

// ---------------------------------------------------------------------------
// The native (C) ABI.
// ---------------------------------------------------------------------------

/// A borrowed UTF-8 string: `len` bytes at `ptr`.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct AbiStr {
    pub ptr: *const u8,
    pub len: usize,
}

impl AbiStr {
    /// Borrow `text` for the duration of a call.
    pub fn new(text: &str) -> Self {
        AbiStr {
            ptr: text.as_ptr(),
            len: text.len(),
        }
    }

    /// The string, if it is valid UTF-8.
    ///
    /// # Safety
    ///
    /// `ptr` must be null with `len == 0`, or point to `len` readable bytes
    /// that outlive `'a`.
    pub unsafe fn as_str<'a>(self) -> Result<&'a str, AbiError> {
        if self.ptr.is_null() {
            return if self.len == 0 {
                Ok("")
            } else {
                Err(AbiError::Invalid("a null string with a length".into()))
            };
        }
        // SAFETY: the caller promises `len` readable bytes at `ptr`.
        let bytes = unsafe { std::slice::from_raw_parts(self.ptr, self.len) };
        std::str::from_utf8(bytes).map_err(|_| AbiError::Invalid("a string is not UTF-8".into()))
    }
}

/// One capability in a [`PluginDescriptor`]: a [`CapabilityKind::code`] and a
/// name.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct AbiCapabilityEntry {
    pub kind: u32,
    pub name: AbiStr,
}

/// Bytes a plugin allocated and hands to the host, which gives them back
/// through [`PluginVTable::free`] (the plugin's allocator frees them).
#[repr(C)]
#[derive(Debug)]
pub struct AbiOutput {
    pub ptr: *mut u8,
    pub len: usize,
    pub cap: usize,
}

impl AbiOutput {
    /// An empty output, for the host to pass to a call.
    pub const fn empty() -> Self {
        AbiOutput {
            ptr: std::ptr::null_mut(),
            len: 0,
            cap: 0,
        }
    }

    /// Hand `text` over; free it with [`free_output`].
    pub fn from_string(text: String) -> Self {
        let mut bytes = ManuallyDrop::new(text.into_bytes());
        AbiOutput {
            ptr: bytes.as_mut_ptr(),
            len: bytes.len(),
            cap: bytes.capacity(),
        }
    }
}

/// Run capability `capability` (an index into the descriptor's capabilities)
/// on `input`, with `width` cells available, writing the output to `output`
/// and returning [`STATUS_OK`] or [`STATUS_ERROR`].
pub type AbiCallFn = unsafe extern "C" fn(
    capability: usize,
    input: AbiStr,
    width: u32,
    output: *mut AbiOutput,
) -> u32;

/// Free an output a call returned.
pub type AbiFreeFn = unsafe extern "C" fn(output: AbiOutput);

/// The functions a native plugin provides.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct PluginVTable {
    pub call: AbiCallFn,
    pub free: AbiFreeFn,
}

/// What [`DYLIB_ENTRY_SYMBOL`] returns. `abi_major` and `abi_minor` come first
/// and stay first in every version, so a host can read them before trusting
/// the rest of the layout.
#[repr(C)]
#[derive(Debug)]
pub struct PluginDescriptor {
    pub abi_major: u32,
    pub abi_minor: u32,
    pub name: AbiStr,
    pub version: AbiStr,
    pub description: AbiStr,
    pub capabilities: *const AbiCapabilityEntry,
    pub capability_count: usize,
    pub vtable: PluginVTable,
}

/// Decode a descriptor a native plugin's entry point returned.
///
/// Reads the version first and refuses another major before looking at the
/// rest; then checks every string and capability ([`PluginAbi::validate`]).
///
/// # Safety
///
/// `descriptor` must be null or point to a [`PluginDescriptor`] (or, for
/// another major version, at least two `u32`s) whose strings and capability
/// array stay valid while the plugin is loaded.
pub unsafe fn read_descriptor(
    descriptor: *const PluginDescriptor,
) -> Result<(PluginAbi, PluginVTable), AbiError> {
    if descriptor.is_null() {
        return Err(AbiError::Invalid(format!(
            "{DYLIB_ENTRY_SYMBOL} returned no descriptor (it failed to initialise)"
        )));
    }
    // SAFETY: every version starts with two u32s (see `PluginDescriptor`).
    let (major, minor) = unsafe {
        let version = descriptor.cast::<u32>();
        (version.read(), version.add(1).read())
    };
    if major != ABI_MAJOR {
        return Err(AbiError::Incompatible { major, minor });
    }
    // SAFETY: same major, so the layout is this one.
    let descriptor = unsafe { &*descriptor };
    if descriptor.capability_count > MAX_CAPABILITIES {
        return Err(AbiError::Invalid(format!(
            "{} capabilities declared; at most {MAX_CAPABILITIES} are allowed",
            descriptor.capability_count
        )));
    }
    let entries: &[AbiCapabilityEntry] = if descriptor.capability_count == 0 {
        &[]
    } else if descriptor.capabilities.is_null() {
        return Err(AbiError::Invalid("a null capability list".into()));
    } else {
        // SAFETY: the caller promises the array is valid.
        unsafe { std::slice::from_raw_parts(descriptor.capabilities, descriptor.capability_count) }
    };
    let text = |s: AbiStr, what: &str| -> Result<String, AbiError> {
        if s.len > MAX_FIELD_LEN {
            return Err(AbiError::Invalid(format!("the {what} is too long")));
        }
        // SAFETY: the caller promises the strings are valid.
        unsafe { s.as_str() }.map(str::to_string)
    };
    let mut capabilities = Vec::with_capacity(entries.len());
    for entry in entries {
        let kind = CapabilityKind::from_code(entry.kind).ok_or_else(|| {
            AbiError::Invalid(format!(
                "unknown capability kind {}; this host knows {}",
                entry.kind,
                CapabilityKind::ALL.map(CapabilityKind::as_str).join(", ")
            ))
        })?;
        capabilities.push(AbiCapability {
            kind,
            name: text(entry.name, "capability name")?,
        });
    }
    let abi = PluginAbi {
        abi_major: major,
        abi_minor: minor,
        name: text(descriptor.name, "name")?,
        version: text(descriptor.version, "version")?,
        description: text(descriptor.description, "description")?,
        capabilities,
    };
    abi.validate()?;
    Ok((abi, descriptor.vtable))
}

// ---------------------------------------------------------------------------
// The plugin side: build a native plugin without writing unsafe code.
// ---------------------------------------------------------------------------

/// One capability's implementation: the input and the width in cells, to the
/// output or an error message.
pub type ExportFn = fn(input: &str, width: u32) -> Result<String, String>;

/// A native plugin's description and functions, for
/// [`export_dylib_plugin!`](crate::export_dylib_plugin).
pub struct Exports {
    abi: PluginAbi,
    functions: Vec<ExportFn>,
}

impl Exports {
    /// A plugin called `name` (its id) at `version`.
    pub fn new(name: &str, version: &str) -> Self {
        Exports {
            abi: PluginAbi::new(name, version),
            functions: Vec::new(),
        }
    }

    /// One line on what it does.
    pub fn description(mut self, description: &str) -> Self {
        self.abi.description = description.to_string();
        self
    }

    /// Add a capability of any kind.
    pub fn capability(mut self, kind: CapabilityKind, name: &str, function: ExportFn) -> Self {
        self.abi.capabilities.push(AbiCapability {
            kind,
            name: name.to_string(),
        });
        self.functions.push(function);
        self
    }

    /// A named text transform: plain text in and out.
    pub fn transform(self, name: &str, function: ExportFn) -> Self {
        self.capability(CapabilityKind::Transform, name, function)
    }

    /// A highlighter: plain text in, `START END STYLE` lines out.
    pub fn highlighter(self, name: &str, function: ExportFn) -> Self {
        self.capability(CapabilityKind::Highlighter, name, function)
    }

    /// A fence renderer for `language` returning `rich` markup.
    pub fn fence_markup(self, language: &str, function: ExportFn) -> Self {
        self.capability(CapabilityKind::FenceMarkup, language, function)
    }

    /// A fence renderer for `language` returning ANSI-styled text.
    pub fn fence_ansi(self, language: &str, function: ExportFn) -> Self {
        self.capability(CapabilityKind::FenceAnsi, language, function)
    }

    /// The description, as a host will decode it.
    pub fn abi(&self) -> &PluginAbi {
        &self.abi
    }
}

/// An [`Exports`] laid out for the C ABI. [`export_dylib_plugin!`](crate::export_dylib_plugin)
/// keeps one in a `static`.
#[doc(hidden)]
pub struct Exported {
    exports: Exports,
    _entries: Vec<AbiCapabilityEntry>,
    descriptor: PluginDescriptor,
}

// SAFETY: the raw pointers in `descriptor` and `_entries` point into
// `exports`' strings and `_entries`' buffer, which are never mutated after
// construction and live as long as the `Exported`.
unsafe impl Send for Exported {}
// SAFETY: as above; shared access is read-only.
unsafe impl Sync for Exported {}

impl Exported {
    pub fn new(exports: Exports, call: AbiCallFn) -> Self {
        let entries: Vec<AbiCapabilityEntry> = exports
            .abi
            .capabilities
            .iter()
            .map(|capability| AbiCapabilityEntry {
                kind: capability.kind.code(),
                name: AbiStr::new(&capability.name),
            })
            .collect();
        let descriptor = PluginDescriptor {
            abi_major: exports.abi.abi_major,
            abi_minor: exports.abi.abi_minor,
            name: AbiStr::new(&exports.abi.name),
            version: AbiStr::new(&exports.abi.version),
            description: AbiStr::new(&exports.abi.description),
            capabilities: entries.as_ptr(),
            capability_count: entries.len(),
            vtable: PluginVTable {
                call,
                free: free_output,
            },
        };
        Exported {
            exports,
            _entries: entries,
            descriptor,
        }
    }

    pub fn descriptor(&'static self) -> *const PluginDescriptor {
        &self.descriptor
    }

    /// The body of the generated `call` function. Never unwinds: a panic in
    /// a capability becomes an error result.
    ///
    /// # Safety
    ///
    /// `input` must be valid for the call, and `output` null or writable.
    pub unsafe fn call(
        this: Option<&Exported>,
        capability: usize,
        input: AbiStr,
        width: u32,
        output: *mut AbiOutput,
    ) -> u32 {
        let result = catch_unwind(AssertUnwindSafe(|| {
            let this = this.ok_or("the plugin was called before it was initialised")?;
            let function = this
                .exports
                .functions
                .get(capability)
                .ok_or_else(|| format!("no capability number {capability}"))?;
            // SAFETY: the caller promises `input` is valid for this call.
            let input = unsafe { input.as_str() }.map_err(|e| e.to_string())?;
            function(input, width)
        }));
        let (status, text) = match result {
            Ok(Ok(text)) => (STATUS_OK, text),
            Ok(Err(message)) => (STATUS_ERROR, message),
            Err(_) => (STATUS_ERROR, "the plugin panicked".to_string()),
        };
        if output.is_null() {
            return STATUS_ERROR;
        }
        // SAFETY: the caller promises `output` is writable.
        unsafe { output.write(AbiOutput::from_string(text)) };
        status
    }

    /// The body of the generated entry point: `f` makes the exports, and a
    /// panic in it is a null descriptor instead of an abort.
    pub fn entry(
        slot: &'static std::sync::OnceLock<Exported>,
        make: fn() -> Exports,
        call: AbiCallFn,
    ) -> *const PluginDescriptor {
        match catch_unwind(AssertUnwindSafe(|| {
            slot.get_or_init(|| Exported::new(make(), call))
        })) {
            Ok(exported) => exported.descriptor(),
            Err(_) => std::ptr::null(),
        }
    }
}

/// Free an [`AbiOutput`] made by [`AbiOutput::from_string`].
///
/// # Safety
///
/// `output` must come from [`AbiOutput::from_string`] in this same binary,
/// and be freed once.
pub unsafe extern "C" fn free_output(output: AbiOutput) {
    if !output.ptr.is_null() {
        // SAFETY: made by `from_string` from a `Vec<u8>` with these parts.
        drop(unsafe { Vec::from_raw_parts(output.ptr, output.len, output.cap) });
    }
}

/// Export a native plugin from a `cdylib` crate.
///
/// The argument is a function (or non-capturing closure) returning
/// [`Exports`](crate::abi::Exports). The macro defines the
/// [`rich_plugin_entry`](crate::abi::DYLIB_ENTRY_SYMBOL) symbol; the crate
/// needs `crate-type = ["cdylib"]` and no unsafe code of its own.
///
/// ```
/// use rich_plugin_api::abi::Exports;
///
/// fn upper(input: &str, _width: u32) -> Result<String, String> {
///     Ok(input.to_uppercase())
/// }
///
/// rich_plugin_api::export_dylib_plugin!(|| {
///     Exports::new("shout", "0.1.0")
///         .description("Upper-cases text")
///         .transform("upper", upper)
/// });
/// # fn main() {}
/// ```
#[macro_export]
macro_rules! export_dylib_plugin {
    ($exports:expr) => {
        #[doc(hidden)]
        static __RICH_PLUGIN_EXPORTED: ::std::sync::OnceLock<$crate::abi::Exported> =
            ::std::sync::OnceLock::new();

        #[doc(hidden)]
        #[allow(unsafe_code)]
        unsafe extern "C" fn __rich_plugin_call(
            capability: usize,
            input: $crate::abi::AbiStr,
            width: u32,
            output: *mut $crate::abi::AbiOutput,
        ) -> u32 {
            // SAFETY: the host passes a valid input and a writable output.
            unsafe {
                $crate::abi::Exported::call(
                    __RICH_PLUGIN_EXPORTED.get(),
                    capability,
                    input,
                    width,
                    output,
                )
            }
        }

        /// The plugin's entry point, for the `rich` host.
        #[allow(unsafe_code)]
        #[unsafe(no_mangle)]
        pub extern "C" fn rich_plugin_entry() -> *const $crate::abi::PluginDescriptor {
            $crate::abi::Exported::entry(&__RICH_PLUGIN_EXPORTED, $exports, __rich_plugin_call)
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    fn upper(input: &str, _: u32) -> Result<String, String> {
        Ok(input.to_uppercase())
    }

    fn fails(_: &str, _: u32) -> Result<String, String> {
        Err("no".into())
    }

    fn panics(_: &str, _: u32) -> Result<String, String> {
        panic!("boom")
    }

    fn exports() -> Exports {
        Exports::new("shout", "0.1.0")
            .description("Upper-cases text")
            .transform("upper", upper)
            .fence_markup("shout", fails)
            .highlighter("boom", panics)
    }

    crate::export_dylib_plugin!(exports);

    unsafe fn call(descriptor: &PluginVTable, capability: usize, input: &str) -> (u32, String) {
        let mut output = AbiOutput::empty();
        let status = unsafe { (descriptor.call)(capability, AbiStr::new(input), 80, &mut output) };
        let text = unsafe { std::slice::from_raw_parts(output.ptr, output.len) };
        let text = String::from_utf8(text.to_vec()).unwrap();
        unsafe { (descriptor.free)(output) };
        (status, text)
    }

    #[test]
    fn a_descriptor_round_trips_through_the_c_abi() {
        let (abi, vtable) = unsafe { read_descriptor(rich_plugin_entry()) }.unwrap();
        assert_eq!(abi, *exports().abi());
        assert_eq!(abi.capabilities[1].kind, CapabilityKind::FenceMarkup);
        assert_eq!(
            unsafe { call(&vtable, 0, "hi") },
            (STATUS_OK, "HI".to_string())
        );
        assert_eq!(
            unsafe { call(&vtable, 1, "hi") },
            (STATUS_ERROR, "no".to_string())
        );
        let (status, message) = unsafe { call(&vtable, 2, "hi") };
        assert_eq!(status, STATUS_ERROR);
        assert!(message.contains("panicked"), "{message}");
        let (status, message) = unsafe { call(&vtable, 9, "hi") };
        assert_eq!(status, STATUS_ERROR);
        assert!(message.contains("no capability"), "{message}");
    }

    #[test]
    fn another_major_is_refused_before_the_rest_is_read() {
        // Only the two version words are valid: reading further would be a
        // bug the version check must prevent.
        let words: [u32; 2] = [ABI_MAJOR + 1, 0];
        let error = unsafe { read_descriptor(words.as_ptr().cast()) }.unwrap_err();
        assert_eq!(
            error,
            AbiError::Incompatible {
                major: ABI_MAJOR + 1,
                minor: 0
            }
        );
        assert!(error.to_string().contains("rebuild"));
        assert!(unsafe { read_descriptor(std::ptr::null()) }.is_err());
    }

    #[test]
    fn a_newer_minor_loads_but_an_unknown_kind_does_not() {
        let mut exported = Exported::new(exports(), __rich_plugin_call);
        exported.descriptor.abi_minor = ABI_MINOR + 3;
        let (abi, _) = unsafe { read_descriptor(&exported.descriptor) }.unwrap();
        assert_eq!(abi.abi_minor, ABI_MINOR + 3);
        let entries = [AbiCapabilityEntry {
            kind: 99,
            name: AbiStr::new("x"),
        }];
        exported.descriptor.capabilities = entries.as_ptr();
        exported.descriptor.capability_count = 1;
        let error = unsafe { read_descriptor(&exported.descriptor) }.unwrap_err();
        assert!(error.to_string().contains("unknown capability kind 99"));
    }

    #[test]
    fn manifests_round_trip_and_are_checked() {
        let abi = exports().abi().clone();
        let manifest = abi.to_manifest();
        assert!(manifest.starts_with("rich-plugin-abi 1.0\nname shout\n"));
        assert_eq!(PluginAbi::parse_manifest(&manifest).unwrap(), abi);
        // Unknown keys are ignored; unknown kinds and other majors are not.
        let extra = manifest.replace("name shout", "name shout\nhomepage x");
        assert_eq!(PluginAbi::parse_manifest(&extra).unwrap(), abi);
        let other = manifest.replace("rich-plugin-abi 1.0", "rich-plugin-abi 2.0");
        assert_eq!(
            PluginAbi::parse_manifest(&other),
            Err(AbiError::Incompatible { major: 2, minor: 0 })
        );
        for bad in [
            "",
            "hello",
            "rich-plugin-abi one",
            "rich-plugin-abi 1.0\nname Bad\nversion 1",
            "rich-plugin-abi 1.0\nname ok\n",
            "rich-plugin-abi 1.0\nname ok\nversion 1\ncapability sing x",
            "rich-plugin-abi 1.0\nname ok\nversion 1\ncapability transform X",
            "rich-plugin-abi 1.0\nname ok\nversion 1\ncapability transform x\ncapability transform x",
        ] {
            assert!(PluginAbi::parse_manifest(bad).is_err(), "{bad:?}");
        }
    }
}
