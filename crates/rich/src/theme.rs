//! Themes — named styles.
//!
//! Port of upstream `rich/theme.py` + `rich/default_styles.py`. A [`Theme`] maps
//! style names (e.g. `"repr.number"`, `"warning"`) to [`Style`]s so that markup
//! tags and highlighters can refer to styles by name.
//!
//! Slice scope: a small representative subset of `DEFAULT_STYLES` is seeded. The
//! full table is filled in alongside the highlighter/theme-system issue.

use std::collections::HashMap;

use crate::style::Style;

/// A named collection of styles. Mirrors `rich.theme.Theme`.
#[derive(Debug, Clone, Default)]
pub struct Theme {
    styles: HashMap<String, Style>,
}

impl Theme {
    pub fn new() -> Self {
        Theme::default()
    }

    /// Look up a style by name.
    pub fn get(&self, name: &str) -> Option<&Style> {
        self.styles.get(name)
    }

    /// Insert or replace a named style.
    pub fn insert(&mut self, name: impl Into<String>, style: Style) {
        self.styles.insert(name.into(), style);
    }

    /// A subset of the upstream default styles, enough to exercise markup and
    /// the example highlighter.
    pub fn default_theme() -> Self {
        let mut theme = Theme::new();
        for (name, spec) in [
            ("none", ""),
            ("reset", ""),
            ("bold", "bold"),
            ("italic", "italic"),
            ("emphasis", "italic"),
            ("strong", "bold"),
            ("dim", "dim"),
            ("code", "bold reverse"),
            ("table.header", "bold"),
            ("table.footer", "bold"),
            ("table.title", "italic"),
            ("repr.number", "bold cyan"),
            ("repr.str", "green"),
            ("repr.bool_true", "bright_green"),
            ("repr.bool_false", "bright_red"),
            ("warning", "yellow"),
            ("error", "bold red"),
            ("info", "cyan"),
        ] {
            if let Ok(style) = Style::parse(spec) {
                theme.insert(name, style);
            }
        }
        theme
    }
}
