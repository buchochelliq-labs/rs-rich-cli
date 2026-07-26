//! The internal plugin registry.
//!
//! This is the (currently internal-facing) mechanism for layering our own
//! features onto a faithful-core [`Console`] without touching the core. It uses
//! **explicit registration** — a caller builds a registry, then installs it onto
//! a console — chosen over compile-time magic for debuggability. See
//! docs/PLUGINS.md for the roadmap toward a public, third-party plugin API.

use rich::{Console, Highlighter};

/// A factory that produces a fresh highlighter each time it installs.
type HighlighterFactory = Box<dyn Fn() -> Box<dyn Highlighter>>;

/// A collection of extensions to install onto a [`Console`].
#[derive(Default)]
pub struct ExtensionRegistry {
    highlighters: Vec<HighlighterFactory>,
}

impl ExtensionRegistry {
    /// An empty registry.
    pub fn new() -> Self {
        ExtensionRegistry::default()
    }

    /// A registry pre-loaded with this crate's default extensions.
    pub fn with_defaults() -> Self {
        let mut registry = ExtensionRegistry::new();
        registry.register_highlighter(|| Box::new(crate::highlighter::NumberHighlighter::new()));
        registry
    }

    /// Register a highlighter factory. The factory is invoked at [`install`]
    /// time so a registry can be installed onto multiple consoles.
    ///
    /// [`install`]: ExtensionRegistry::install
    pub fn register_highlighter<F>(&mut self, factory: F) -> &mut Self
    where
        F: Fn() -> Box<dyn Highlighter> + 'static,
    {
        self.highlighters.push(Box::new(factory));
        self
    }

    /// Install every registered extension onto `console`.
    pub fn install(&self, console: &mut Console) {
        for factory in &self.highlighters {
            console.add_highlighter(factory());
        }
    }
}

/// Convenience: install this crate's default extensions onto a console.
pub fn install_defaults(console: &mut Console) {
    ExtensionRegistry::with_defaults().install(console);
}

#[cfg(test)]
mod tests {
    use super::*;
    use rich::ColorSystem;

    #[test]
    fn number_highlighter_installs_and_styles_digits() {
        let mut console = Console::builder()
            .force_terminal(true)
            .color_system(Some(ColorSystem::Truecolor))
            .build();
        install_defaults(&mut console);
        // "42" should be wrapped in bold cyan (1;36); surrounding text plain.
        let out = console.render_str_to_string("n=42");
        assert!(out.contains("\x1b[1;36m42\x1b[0m"), "got: {out:?}");
    }
}
