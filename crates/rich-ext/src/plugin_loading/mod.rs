//! Runtime plugins: native libraries and WASM modules loaded from a path.
//!
//! Both loaders decode a plugin's self-description into one
//! [`PluginAbi`](rich_plugin_api::abi::PluginAbi) and wrap it in a
//! [`RuntimePlugin`], an ordinary [`Plugin`] that
//! [`ExtensionRegistry::add_plugin`](crate::ExtensionRegistry::add_plugin)
//! accepts like any other: the same name rules, the same conflict checks.
//!
//! A runtime plugin exchanges text only (see [`rich_plugin_api::abi`]), and
//! everything it returns is sanitized here before a console sees it:
//! terminal controls are made visible, and only a `fence-ansi` capability
//! keeps SGR styling, through [`sanitize_ansi_for_decoder`].
//!
//! Loading is behind Cargo features, both off by default:
//! `dylib-plugins` ([`load_native`]) and `wasm-plugins` ([`load_wasm`]).
//! A native plugin runs arbitrary code in this process, so load one only from
//! a path the user chose. See docs/design/plugin-loading.md and the threat
//! model in docs/PLUGINS.md.
//!
//! [`sanitize_ansi_for_decoder`]: crate::sanitize::sanitize_ansi_for_decoder

use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use rich::console::ConsoleOptions;
use rich::segment::Segment;
use rich::{Console, FenceRenderer, Highlighter, Renderable, Style, Text};
use rich_plugin_api::abi::{AbiError, CapabilityKind, PluginAbi};
use rich_plugin_api::{Plugin, PluginError, PluginMetadata, PluginRegistrar, TextTransform};

use crate::sanitize::{sanitize_ansi_for_decoder, sanitize_terminal_controls};

#[cfg(feature = "dylib-plugins")]
mod native;
#[cfg(feature = "wasm-plugins")]
mod wasm;

#[cfg(feature = "dylib-plugins")]
pub use native::load_native;
#[cfg(feature = "wasm-plugins")]
pub use wasm::load_wasm;

/// The most bytes one call may return; more is an error.
pub const MAX_OUTPUT_BYTES: usize = 4 * 1024 * 1024;
/// The most spans one highlighter call may return; the rest are dropped.
pub const MAX_SPANS: usize = 10_000;

/// How a runtime plugin was loaded.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum RuntimeKind {
    /// A native library through the C ABI (`dylib-plugins`).
    Native,
    /// A WASM module in the sandbox (`wasm-plugins`).
    Wasm,
}

impl RuntimeKind {
    /// `native` or `wasm`.
    pub fn as_str(self) -> &'static str {
        match self {
            RuntimeKind::Native => "native",
            RuntimeKind::Wasm => "wasm",
        }
    }

    /// The kind a path's extension names: `.wasm`, or a platform library
    /// (`.so`, `.dylib`, `.dll`).
    pub fn for_path(path: &Path) -> Option<RuntimeKind> {
        match path.extension()?.to_str()?.to_ascii_lowercase().as_str() {
            "wasm" => Some(RuntimeKind::Wasm),
            "so" | "dylib" | "dll" => Some(RuntimeKind::Native),
            _ => None,
        }
    }

    /// The Cargo feature that loads this kind.
    pub fn feature(self) -> &'static str {
        match self {
            RuntimeKind::Native => "dylib-plugins",
            RuntimeKind::Wasm => "wasm-plugins",
        }
    }

    /// Whether this build can load this kind.
    pub fn supported(self) -> bool {
        match self {
            RuntimeKind::Native => cfg!(feature = "dylib-plugins"),
            RuntimeKind::Wasm => cfg!(feature = "wasm-plugins"),
        }
    }
}

impl fmt::Display for RuntimeKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Limits for a WASM plugin. Every call runs in a fresh instance.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WasmLimits {
    /// Fuel for one call (roughly, instructions), instantiation included.
    pub fuel: u64,
    /// The most linear memory an instance may have, in bytes.
    pub memory_bytes: usize,
    /// The largest module file accepted, in bytes.
    pub module_bytes: u64,
}

impl Default for WasmLimits {
    fn default() -> Self {
        WasmLimits {
            fuel: 50_000_000,
            memory_bytes: 64 * 1024 * 1024,
            module_bytes: 16 * 1024 * 1024,
        }
    }
}

/// Why a runtime plugin could not be loaded. Every variant names the path.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum LoadError {
    /// The file could not be read or opened.
    Io { path: PathBuf, message: String },
    /// Not a `.wasm`, `.so`, `.dylib` or `.dll` file.
    UnknownKind { path: PathBuf },
    /// This build cannot load that kind: it needs `feature`.
    Unsupported {
        path: PathBuf,
        feature: &'static str,
    },
    /// The file is not a plugin: an export or symbol is missing or wrong.
    NotAPlugin { path: PathBuf, message: String },
    /// The plugin's description was refused (ABI version, names, kinds).
    Abi { path: PathBuf, error: AbiError },
    /// The plugin exceeded a limit (fuel, memory, size) while loading.
    Limit { path: PathBuf, message: String },
}

impl LoadError {
    /// The path that failed.
    pub fn path(&self) -> &Path {
        match self {
            LoadError::Io { path, .. }
            | LoadError::UnknownKind { path }
            | LoadError::Unsupported { path, .. }
            | LoadError::NotAPlugin { path, .. }
            | LoadError::Abi { path, .. }
            | LoadError::Limit { path, .. } => path,
        }
    }
}

impl fmt::Display for LoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let path = self.path().display();
        match self {
            LoadError::Io { message, .. } => write!(f, "plugin {path}: {message}"),
            LoadError::UnknownKind { .. } => write!(
                f,
                "plugin {path}: not a plugin file; expected .wasm, or a native library \
                 (.so, .dylib, .dll)"
            ),
            LoadError::Unsupported { feature, .. } => write!(
                f,
                "plugin {path}: this build cannot load it; it needs the {feature} feature"
            ),
            LoadError::NotAPlugin { message, .. } => {
                write!(f, "plugin {path}: not a rich plugin: {message}")
            }
            LoadError::Abi { error, .. } => write!(f, "plugin {path}: {error}"),
            LoadError::Limit { message, .. } => write!(f, "plugin {path}: {message}"),
        }
    }
}

impl std::error::Error for LoadError {}

/// Options for [`load`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LoadOptions {
    pub wasm: WasmLimits,
}

/// Load the plugin at `path`, choosing the loader by its extension.
///
/// Refused, never panicking, when the kind is unknown or not compiled in, or
/// when the loader refuses it.
pub fn load(path: &Path, options: &LoadOptions) -> Result<RuntimePlugin, LoadError> {
    let kind = RuntimeKind::for_path(path).ok_or_else(|| LoadError::UnknownKind {
        path: path.to_path_buf(),
    })?;
    if !kind.supported() {
        return Err(LoadError::Unsupported {
            path: path.to_path_buf(),
            feature: kind.feature(),
        });
    }
    let _ = options;
    match kind {
        #[cfg(feature = "dylib-plugins")]
        RuntimeKind::Native => load_native(path),
        #[cfg(feature = "wasm-plugins")]
        RuntimeKind::Wasm => load_wasm(path, &options.wasm),
        #[allow(unreachable_patterns)]
        _ => unreachable!("checked by RuntimeKind::supported"),
    }
}

/// Runs a runtime plugin's capabilities. The native and WASM loaders each
/// implement it; tests can too.
pub trait AbiBackend: Send + Sync {
    /// Run capability number `capability` (its index in
    /// [`PluginAbi::capabilities`]) on `input` with `width` cells available
    /// (0 when unknown). `Err` is the plugin's error message, or the host's.
    fn call(&self, capability: usize, input: &str, width: u32) -> Result<String, String>;
}

/// A loaded runtime plugin: its description, where it came from, and the
/// backend that runs it. Keeping any of its capabilities keeps the backend
/// (and a native library) loaded.
#[derive(Clone)]
pub struct RuntimePlugin {
    abi: PluginAbi,
    kind: RuntimeKind,
    path: PathBuf,
    backend: Arc<dyn AbiBackend>,
}

impl fmt::Debug for RuntimePlugin {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RuntimePlugin")
            .field("abi", &self.abi)
            .field("kind", &self.kind)
            .field("path", &self.path)
            .finish_non_exhaustive()
    }
}

impl RuntimePlugin {
    /// Wrap a backend. The description is [validated](PluginAbi::validate)
    /// again, so a backend cannot bypass the ABI checks.
    pub fn new(
        abi: PluginAbi,
        kind: RuntimeKind,
        path: impl Into<PathBuf>,
        backend: Arc<dyn AbiBackend>,
    ) -> Result<Self, LoadError> {
        let path = path.into();
        abi.validate().map_err(|error| LoadError::Abi {
            path: path.clone(),
            error,
        })?;
        Ok(RuntimePlugin {
            abi,
            kind,
            path,
            backend,
        })
    }

    /// The plugin's self-description.
    pub fn abi(&self) -> &PluginAbi {
        &self.abi
    }

    /// Native or WASM.
    pub fn kind(&self) -> RuntimeKind {
        self.kind
    }

    /// The file it was loaded from.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Call a capability directly, with the output limit applied but not
    /// sanitized (the registry adapters sanitize).
    pub fn call(&self, capability: usize, input: &str, width: u32) -> Result<String, String> {
        let output = self.backend.call(capability, input, width)?;
        if output.len() > MAX_OUTPUT_BYTES {
            return Err(format!(
                "the plugin returned {} bytes; at most {MAX_OUTPUT_BYTES} are accepted",
                output.len()
            ));
        }
        Ok(output)
    }

    fn capability(&self, index: usize) -> Capability {
        Capability {
            plugin: Arc::new(self.clone()),
            index,
        }
    }
}

impl Plugin for RuntimePlugin {
    fn metadata(&self) -> PluginMetadata {
        let metadata = PluginMetadata::new(&*self.abi.name, &*self.abi.name, &*self.abi.version);
        if self.abi.description.is_empty() {
            metadata
        } else {
            metadata.description(&*self.abi.description)
        }
    }

    fn register(&self, registrar: &mut dyn PluginRegistrar) -> Result<(), PluginError> {
        for (index, capability) in self.abi.capabilities.iter().enumerate() {
            let adapter = self.capability(index);
            match capability.kind {
                CapabilityKind::Transform => {
                    registrar.transform(&capability.name, Arc::new(adapter));
                }
                CapabilityKind::Highlighter => {
                    registrar.highlighter(Box::new(move || Box::new(adapter.clone())));
                }
                CapabilityKind::FenceMarkup | CapabilityKind::FenceAnsi => {
                    registrar.fence_renderer(&capability.name, Arc::new(adapter));
                }
                other => {
                    return Err(PluginError::Other(format!(
                        "unsupported capability kind {other}"
                    )))
                }
            }
        }
        Ok(())
    }
}

/// One capability of a [`RuntimePlugin`], adapted to the registry's traits.
#[derive(Clone)]
struct Capability {
    plugin: Arc<RuntimePlugin>,
    index: usize,
}

impl Capability {
    fn kind(&self) -> CapabilityKind {
        self.plugin.abi.capabilities[self.index].kind
    }

    fn call(&self, input: &str, width: u32) -> Result<String, String> {
        self.plugin.call(self.index, input, width)
    }

    /// The output as a [`Text`], sanitized for this capability's kind; `None`
    /// when markup does not parse.
    fn text(&self, output: &str) -> Option<Text> {
        if self.kind().produces_ansi() {
            Some(Text::from_ansi(&sanitize_ansi_for_decoder(output), ""))
        } else if self.kind() == CapabilityKind::FenceMarkup {
            Text::from_markup(&sanitize_terminal_controls(output)).ok()
        } else {
            Some(Text::new(sanitize_terminal_controls(output)))
        }
    }
}

impl TextTransform for Capability {
    fn transform(&self, text: Text) -> Result<Text, PluginError> {
        let output = self.call(text.plain(), 0).map_err(|message| {
            PluginError::Other(format!("plugin {:?}: {message}", self.plugin.abi.name))
        })?;
        Ok(Text::new(sanitize_terminal_controls(&output)))
    }
}

impl Highlighter for Capability {
    fn highlight(&self, text: &mut Text) {
        // A highlighter cannot fail: a plugin error highlights nothing.
        let Ok(output) = self.call(text.plain(), 0) else {
            return;
        };
        let plain = text.plain().to_string();
        for span in parse_spans(&output, &plain).take(MAX_SPANS) {
            text.stylize(span.2, span.0, span.1);
        }
    }
}

/// The spans in a highlighter's output that apply to `plain`: in range, on
/// character boundaries, with a style that parses.
fn parse_spans<'a>(
    output: &'a str,
    plain: &'a str,
) -> impl Iterator<Item = (usize, usize, Style)> + 'a {
    output.lines().filter_map(move |line| {
        let mut parts = line.trim().splitn(3, ' ');
        let start: usize = parts.next()?.parse().ok()?;
        let end: usize = parts.next()?.parse().ok()?;
        let style = Style::parse(parts.next()?.trim()).ok()?;
        (start < end
            && end <= plain.len()
            && plain.is_char_boundary(start)
            && plain.is_char_boundary(end))
        .then_some((start, end, style))
    })
}

impl FenceRenderer for Capability {
    fn render_fence(
        &self,
        _language: &str,
        code: &str,
        console: &Console,
        options: &ConsoleOptions,
    ) -> Option<Vec<Segment>> {
        // A failed call leaves the fence to render as code.
        let width = u32::try_from(options.max_width).unwrap_or(u32::MAX);
        let output = self.call(code, width).ok()?;
        let text = self.text(&output)?;
        Some(text.rich_render(console, options))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ExtensionRegistry;
    use rich_plugin_api::abi::AbiCapability;

    /// A backend in Rust: `upper` upper-cases, `digits` styles digits,
    /// `box` returns markup, `paint` returns ANSI with an OSC title.
    struct Fake;

    impl AbiBackend for Fake {
        fn call(&self, capability: usize, input: &str, _width: u32) -> Result<String, String> {
            match capability {
                0 => Ok(format!("{}\u{1b}]0;pwned\u{7}", input.to_uppercase())),
                1 => Ok(input
                    .char_indices()
                    .filter(|(_, c)| c.is_ascii_digit())
                    .map(|(i, _)| format!("{i} {} bold\n", i + 1))
                    .chain(["0 999 bold\n".into(), "x y z\n".into()])
                    .collect()),
                2 => Ok(format!("[bold]{input}[/]\u{1b}[2J")),
                3 => Ok(format!("\u{1b}[31m{input}\u{1b}[0m\u{1b}]0;t\u{7}\u{9b}2J")),
                _ => Err("no such capability".into()),
            }
        }
    }

    fn plugin() -> RuntimePlugin {
        let mut abi = PluginAbi::new("fake", "1.0.0");
        for (kind, name) in [
            (CapabilityKind::Transform, "upper"),
            (CapabilityKind::Highlighter, "digits"),
            (CapabilityKind::FenceMarkup, "box"),
            (CapabilityKind::FenceAnsi, "paint"),
        ] {
            abi.capabilities.push(AbiCapability {
                kind,
                name: name.into(),
            });
        }
        RuntimePlugin::new(abi, RuntimeKind::Wasm, "fake.wasm", Arc::new(Fake)).unwrap()
    }

    #[test]
    fn capabilities_reach_the_registry_and_output_is_sanitized() {
        let mut registry = ExtensionRegistry::new();
        registry.add_plugin(&plugin()).unwrap();
        let caps = &registry.plugins()[0].capabilities;
        assert_eq!(caps.len(), 4);

        // A transform's output keeps no control: the OSC is made visible.
        let upper = registry.transform("upper").unwrap();
        let out = upper.transform(Text::new("a1")).unwrap();
        assert_eq!(out.plain(), "A1␛]0;pwned␇");

        // Spans out of range or malformed are skipped.
        let mut console = Console::builder()
            .width(20)
            .color_system(Some(rich::ColorSystem::Standard))
            .force_terminal(true)
            .build();
        registry.install(&mut console);
        let mut text = Text::new("a1b22");
        Capability {
            plugin: Arc::new(plugin()),
            index: 1,
        }
        .highlight(&mut text);
        assert_eq!(text.spans().len(), 3);

        // Fences: markup and ANSI render, controls do not survive.
        let fences = registry.fences().unwrap();
        let options = console.options();
        let render = |language: &str| {
            let segments = fences
                .render_fence(language, "hi", &console, &options)
                .unwrap();
            segments
                .iter()
                .map(|s| s.text.to_string())
                .collect::<String>()
        };
        let boxed = render("box");
        assert!(boxed.starts_with("hi␛[2J"), "{boxed:?}");
        let painted = render("paint");
        assert!(painted.starts_with("hi\\u{009B}2J"), "{painted:?}");
        let segments = fences
            .render_fence("paint", "hi", &console, &options)
            .unwrap();
        assert!(segments.iter().any(|s| s.text == "hi" && s.style.is_some()));
    }

    #[test]
    fn a_failing_fence_falls_back_and_output_is_capped() {
        struct Big;
        impl AbiBackend for Big {
            fn call(&self, _: usize, _: &str, _: u32) -> Result<String, String> {
                Ok("x".repeat(MAX_OUTPUT_BYTES + 1))
            }
        }
        let mut abi = PluginAbi::new("big", "1");
        abi.capabilities.push(AbiCapability {
            kind: CapabilityKind::FenceMarkup,
            name: "big".into(),
        });
        let plugin = RuntimePlugin::new(abi, RuntimeKind::Native, "big.so", Arc::new(Big)).unwrap();
        assert!(plugin.call(0, "", 0).unwrap_err().contains("at most"));
        let console = Console::builder().width(20).build();
        let fence = plugin.capability(0);
        assert!(fence
            .render_fence("big", "x", &console, &console.options())
            .is_none());
    }

    #[test]
    fn a_backend_cannot_bypass_the_abi_checks() {
        let abi = PluginAbi::new("Bad Name", "1");
        let error =
            RuntimePlugin::new(abi, RuntimeKind::Wasm, "x.wasm", Arc::new(Fake)).unwrap_err();
        assert!(error.to_string().contains("x.wasm"));
        let mut abi = PluginAbi::new("ok", "1");
        abi.abi_major += 1;
        assert!(matches!(
            RuntimePlugin::new(abi, RuntimeKind::Wasm, "x.wasm", Arc::new(Fake)),
            Err(LoadError::Abi {
                error: AbiError::Incompatible { .. },
                ..
            })
        ));
    }

    #[test]
    fn unknown_and_disabled_kinds_are_refused() {
        let options = LoadOptions::default();
        assert!(matches!(
            load(Path::new("plugin.txt"), &options),
            Err(LoadError::UnknownKind { .. })
        ));
        if !cfg!(feature = "wasm-plugins") {
            let error = load(Path::new("p.wasm"), &options).unwrap_err();
            assert!(error.to_string().contains("wasm-plugins"), "{error}");
        }
        if !cfg!(feature = "dylib-plugins") {
            let error = load(Path::new("p.so"), &options).unwrap_err();
            assert!(error.to_string().contains("dylib-plugins"), "{error}");
        }
    }
}
