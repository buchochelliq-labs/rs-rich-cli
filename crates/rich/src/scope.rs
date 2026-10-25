//! Port of upstream rich/scope.py.
//!
//! [`render_scope`] draws a mapping of variable names to values (upstream
//! passes `locals()`) as a two-column grid of `name =` and the value's
//! [`Pretty`] repr, in a fitted panel. Rust has no reflection over a scope,
//! so the caller passes the names and [`PrettyValue`]s.

use std::sync::Arc;

use crate::console::{Justify, Overflow};
use crate::highlighter::ReprHighlighter;
use crate::panel::Panel;
use crate::pretty::{Pretty, PrettyValue};
use crate::table::{Cell, Table};
use crate::text::Text;

/// The keyword arguments of `render_scope`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScopeOptions {
    /// A title for the panel, as console markup (upstream `title`).
    pub title: Option<String>,
    /// Sort the names: `__special__` names first, then case-insensitively
    /// (upstream `sort_keys`, default `True`).
    pub sort_keys: bool,
    /// Draw indent guides in the values (upstream `indent_guides`).
    pub indent_guides: bool,
    /// Abbreviate containers longer than this (upstream `max_length`).
    pub max_length: Option<usize>,
    /// Truncate strings longer than this (upstream `max_string`).
    pub max_string: Option<usize>,
    /// Stop at this depth of nesting (upstream `max_depth`).
    pub max_depth: Option<usize>,
    /// How values that do not fit overflow (upstream `overflow`).
    pub overflow: Option<Overflow>,
}

impl Default for ScopeOptions {
    fn default() -> Self {
        ScopeOptions {
            title: None,
            sort_keys: true,
            indent_guides: false,
            max_length: None,
            max_string: None,
            max_depth: None,
            overflow: None,
        }
    }
}

/// Render variables in a scope: each name, `=`, and its pretty repr, in a
/// panel. Port of `rich.scope.render_scope`.
pub fn render_scope<K: Into<String>>(
    scope: impl IntoIterator<Item = (K, PrettyValue)>,
    options: &ScopeOptions,
) -> Panel {
    let mut items_table = Table::grid().padding(0, 1, 0, 1).expand(false);
    items_table.add_column_justify("", Justify::Right);

    let mut items: Vec<(String, PrettyValue)> = scope
        .into_iter()
        .map(|(key, value)| (key.into(), value))
        .collect();
    if options.sort_keys {
        // Special variables first, then alphabetically. The sort is stable,
        // as Python's `sorted` is.
        items.sort_by_cached_key(|(key, _)| (!key.starts_with("__"), key.to_lowercase()));
    }
    for (key, value) in items {
        let key_style = if key.starts_with("__") {
            "scope.key.special"
        } else {
            "scope.key"
        };
        let mut key_text = Text::new("");
        key_text.append(&key, Some(key_style.into()));
        key_text.append(" =", Some("scope.equals".into()));
        let pretty = Pretty::from_value(value)
            .highlighter(ReprHighlighter::new())
            .indent_guides(options.indent_guides)
            .max_length(options.max_length)
            .max_string(options.max_string)
            .max_depth(options.max_depth)
            .overflow(options.overflow);
        items_table.add_row_cells(vec![
            Cell::Text(key_text),
            Cell::Renderable(Arc::new(pretty)),
        ]);
    }
    let mut panel = Panel::fit(Box::new(items_table))
        .border_style("scope.border")
        .padding((0, 1, 0, 1));
    if let Some(title) = &options.title {
        panel = panel.title(title.clone());
    }
    panel
}
