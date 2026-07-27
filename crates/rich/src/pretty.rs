//! Pretty-printing of values.
//!
//! Rust-native reimagining of `rich/pretty.py`. Upstream pretty-prints and
//! colorizes a *Python* object's `repr`; Rust has no runtime reflection, so
//! [`Pretty`] instead formats a value with its [`Debug`] implementation
//! (pretty by default, `{:#?}`) and colorizes the result with the built-in
//! [`ReprHighlighter`](crate::highlighter::ReprHighlighter) — numbers, strings,
//! `None`/`Some`, paths, URLs, and so on.
//!
//! **Divergence:** the coloring targets repr-style output; a few Rust spellings
//! differ from Python's (`true`/`false` vs `True`/`False`), so those tokens are
//! left unstyled. See docs/DIVERGENCES.md.

use std::fmt::Debug;

use crate::console::{Console, ConsoleOptions};
use crate::highlighter::ReprHighlighter;
use crate::measure::Measurement;
use crate::protocol::{Highlighter, Renderable};
use crate::segment::Segment;
use crate::text::Text;

/// A syntax-highlighted view of a value's [`Debug`] output. Mirrors
/// `rich.pretty.Pretty`.
pub struct Pretty {
    text: Text,
}

impl Pretty {
    /// Pretty-print `value` (`{:#?}`, multi-line and indented) and highlight it.
    pub fn new(value: &impl Debug) -> Self {
        Pretty::from_string(format!("{value:#?}"))
    }

    /// Format `value` compactly on one line (`{:?}`) and highlight it.
    pub fn compact(value: &impl Debug) -> Self {
        Pretty::from_string(format!("{value:?}"))
    }

    fn from_string(rendered: String) -> Self {
        let mut text = Text::new(rendered);
        ReprHighlighter::new().highlight(&mut text);
        Pretty { text }
    }
}

impl Renderable for Pretty {
    fn rich_render(&self, console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
        self.text.rich_render(console, options)
    }

    fn measure(&self, console: &Console, options: &ConsoleOptions) -> Measurement {
        self.text.measure(console, options)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::color::ColorSystem;

    fn render(pretty: &Pretty) -> String {
        Console::builder()
            .force_terminal(true)
            .color_system(Some(ColorSystem::Truecolor))
            .width(80)
            .no_color(false)
            .build()
            .render_to_string(pretty)
    }

    #[test]
    fn highlights_numbers_in_debug_output() {
        // Numbers get the repr.number style (bold cyan → 1;36).
        let out = render(&Pretty::compact(&vec![1, 2, 3]));
        assert!(out.contains("\x1b[1;36m1\x1b[0m"), "got {out:?}");
        assert!(out.contains('3'));
    }

    #[test]
    fn highlights_string_in_debug_output() {
        // A quoted string gets the repr.str style (green → 32).
        let out = render(&Pretty::compact(&"hello"));
        assert!(out.contains("\x1b[32m\"hello\"\x1b[0m"), "got {out:?}");
    }

    #[test]
    fn pretty_multiline_preserves_structure() {
        // `{:#?}` on a nested collection spans multiple indented lines.
        let out = render(&Pretty::new(&vec![vec![1, 2], vec![3, 4]]));
        assert!(out.contains('\n'), "expected pretty multi-line layout");
        assert!(out.contains("\x1b[1;36m4\x1b[0m"), "numbers highlighted");
    }
}
