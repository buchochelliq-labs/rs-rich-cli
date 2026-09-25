//! The plugin contract for the `rich` Rust port.
//!
//! A plugin implements [`Plugin`]: it describes itself with [`PluginMetadata`]
//! and adds capabilities through a [`PluginRegistrar`]. A host (`rs-rich-ext`'s
//! `ExtensionRegistry`) calls [`Plugin::register`], checks the result, and then
//! makes the capabilities available to consoles and commands.
//!
//! This crate depends only on core `rich`, so a plugin never needs `rs-rich-ext`.
//! Everything first-party plugins do goes through this same contract.
//!
//! ```
//! use rich_plugin_api::{Plugin, PluginError, PluginMetadata, PluginRegistrar};
//! use rich::Theme;
//!
//! struct Solarized;
//!
//! impl Plugin for Solarized {
//!     fn metadata(&self) -> PluginMetadata {
//!         PluginMetadata::new("solarized", "Solarized themes", "1.0.0")
//!     }
//!     fn register(&self, registrar: &mut dyn PluginRegistrar) -> Result<(), PluginError> {
//!         let theme = Theme::from_styles([("repr.number", "#268bd2")], true)
//!             .map_err(|e| PluginError::Other(e.to_string()))?;
//!         registrar.theme("solarized", theme);
//!         Ok(())
//!     }
//! }
//! # assert_eq!(Solarized.metadata().api_version, rich_plugin_api::PLUGIN_API_VERSION);
//! ```
//!
//! **Stability:** at 0.0.x this contract still changes. Every breaking change
//! bumps [`PLUGIN_API_VERSION`], and hosts refuse a plugin built for another
//! version rather than misbehaving.

use std::fmt;
use std::sync::Arc;

use rich::r#box::Box as BoxStyle;
use rich::{CodeHighlighter, Highlighter, Renderable, Theme};

/// The version of this contract. A host accepts a plugin only if the plugin's
/// [`PluginMetadata::api_version`] equals the host's.
pub const PLUGIN_API_VERSION: u32 = 1;

/// Makes a fresh regex [`Highlighter`] each time a host installs it, so one
/// registration can be installed onto many consoles.
pub type HighlighterFactory = Box<dyn Fn() -> Box<dyn Highlighter + Send> + Send + Sync>;

/// Who a plugin is. Build it with [`PluginMetadata::new`].
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct PluginMetadata {
    /// A stable, unique identifier: lowercase letters, digits, `-`, `_` and `.`.
    pub id: String,
    /// A human-readable name.
    pub name: String,
    /// The plugin's own version, usually `env!("CARGO_PKG_VERSION")`.
    pub version: String,
    /// The [`PLUGIN_API_VERSION`] the plugin was built against.
    pub api_version: u32,
    /// One line on what the plugin adds.
    pub description: Option<String>,
}

impl PluginMetadata {
    /// Metadata for a plugin built against this crate's [`PLUGIN_API_VERSION`].
    pub fn new(id: impl Into<String>, name: impl Into<String>, version: impl Into<String>) -> Self {
        PluginMetadata {
            id: id.into(),
            name: name.into(),
            version: version.into(),
            api_version: PLUGIN_API_VERSION,
            description: None,
        }
    }

    /// Add a one-line description.
    pub fn description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }
}

/// Turns source text into a renderable: a diagram, a data format, a report.
/// Registered under a name with [`PluginRegistrar::renderer`].
pub trait SourceRenderer: Send + Sync {
    /// Render `source`. Errors should say what was wrong with the input.
    fn render(&self, source: &str) -> Result<Box<dyn Renderable + Send + Sync>, PluginError>;
}

/// What a plugin can add. [`Plugin::register`] receives one of these.
///
/// Names are checked by the host when `register` returns: they must be
/// non-empty and use lowercase letters, digits, `-`, `_` and `.`, and two
/// plugins may not register the same capability under the same name.
pub trait PluginRegistrar {
    /// A regex highlighter applied to printed text.
    fn highlighter(&mut self, factory: HighlighterFactory);

    /// A syntax-highlighting engine, selectable by `name`.
    fn code_highlighter(&mut self, name: &str, highlighter: Arc<dyn CodeHighlighter>);

    /// A named theme.
    fn theme(&mut self, name: &str, theme: Theme);

    /// A named table/panel box style.
    fn box_style(&mut self, name: &str, style: BoxStyle);

    /// A named source renderer.
    fn renderer(&mut self, name: &str, renderer: Arc<dyn SourceRenderer>);
}

/// Something that extends `rich`.
pub trait Plugin: Send + Sync {
    /// Who the plugin is. Called before [`register`](Self::register).
    fn metadata(&self) -> PluginMetadata;

    /// Add capabilities. If this returns an error, the host keeps nothing the
    /// plugin registered.
    fn register(&self, registrar: &mut dyn PluginRegistrar) -> Result<(), PluginError>;
}

/// One thing a plugin registered, as a host reports it.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[non_exhaustive]
pub enum Capability {
    /// A regex highlighter (these are not named).
    Highlighter,
    CodeHighlighter(String),
    Theme(String),
    BoxStyle(String),
    Renderer(String),
}

impl fmt::Display for Capability {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Capability::Highlighter => write!(f, "highlighter"),
            Capability::CodeHighlighter(name) => write!(f, "code highlighter {name:?}"),
            Capability::Theme(name) => write!(f, "theme {name:?}"),
            Capability::BoxStyle(name) => write!(f, "box style {name:?}"),
            Capability::Renderer(name) => write!(f, "renderer {name:?}"),
        }
    }
}

/// Why a plugin could not be added, or a renderer failed.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum PluginError {
    /// The plugin was built for another [`PLUGIN_API_VERSION`].
    IncompatibleApi {
        plugin: String,
        built_for: u32,
        host: u32,
    },
    /// A plugin with this id is already registered.
    DuplicatePlugin { id: String },
    /// Two plugins registered the same capability under the same name.
    Conflict {
        capability: Capability,
        existing: String,
        plugin: String,
    },
    /// An id or capability name is empty or uses characters other than
    /// lowercase letters, digits, `-`, `_` and `.`.
    InvalidName { plugin: String, name: String },
    /// The plugin's own `register` failed.
    Failed { plugin: String, message: String },
    /// A plugin-defined error, for example from a [`SourceRenderer`].
    Other(String),
}

impl fmt::Display for PluginError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PluginError::IncompatibleApi {
                plugin,
                built_for,
                host,
            } => write!(
                f,
                "plugin {plugin:?} was built for plugin API {built_for}, but this host supports \
                 plugin API {host}; use a version of the plugin built for API {host}"
            ),
            PluginError::DuplicatePlugin { id } => {
                write!(f, "a plugin with id {id:?} is already registered")
            }
            PluginError::Conflict {
                capability,
                existing,
                plugin,
            } => write!(
                f,
                "plugin {plugin:?} registers {capability}, which plugin {existing:?} already provides"
            ),
            PluginError::InvalidName { plugin, name } => write!(
                f,
                "plugin {plugin:?} uses the invalid name {name:?}: use lowercase letters, digits, \
                 '-', '_' and '.'"
            ),
            PluginError::Failed { plugin, message } => {
                write!(f, "plugin {plugin:?} failed to register: {message}")
            }
            PluginError::Other(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for PluginError {}

/// Whether `name` is a valid plugin id or capability name.
pub fn is_valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name.bytes().all(|b| {
            b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'-' | b'_' | b'.')
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metadata_targets_this_api_version() {
        let meta = PluginMetadata::new("x", "X", "1.0.0").description("does x");
        assert_eq!(meta.api_version, PLUGIN_API_VERSION);
        assert_eq!(meta.description.as_deref(), Some("does x"));
    }

    #[test]
    fn names_are_restricted() {
        for good in ["syntect", "lumis", "my-plugin_2.x"] {
            assert!(is_valid_name(good), "{good}");
        }
        for bad in [
            "",
            "Upper",
            "has space",
            "semi;colon",
            "\u{1b}]0;x",
            &"a".repeat(65),
        ] {
            assert!(!is_valid_name(bad), "{bad:?}");
        }
    }

    #[test]
    fn errors_name_both_sides() {
        let conflict = PluginError::Conflict {
            capability: Capability::Theme("dark".into()),
            existing: "a".into(),
            plugin: "b".into(),
        };
        let message = conflict.to_string();
        assert!(message.contains("\"a\"") && message.contains("\"b\"") && message.contains("dark"));
        let incompatible = PluginError::IncompatibleApi {
            plugin: "old".into(),
            built_for: 0,
            host: PLUGIN_API_VERSION,
        };
        assert!(incompatible.to_string().contains("plugin API 0"));
    }
}
