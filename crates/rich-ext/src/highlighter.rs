//! Example extension: a number highlighter.
//!
//! This lives in `rich-ext`, **not** in the faithful core, and demonstrates the
//! plugin seam: it implements the core [`Highlighter`] trait and is registered
//! onto a `Console` from outside. The core has zero knowledge of it.

use rich::text::Text;
use rich::{Highlighter, Style};

/// Highlights runs of ASCII digits with the `repr.number`-ish style.
pub struct NumberHighlighter {
    style: Style,
}

impl NumberHighlighter {
    pub fn new() -> Self {
        // A fixed style so construction can't fail; "bold cyan" mirrors the
        // spirit of upstream's `repr.number`.
        let style = Style::parse("bold cyan").expect("valid built-in style");
        NumberHighlighter { style }
    }
}

impl Default for NumberHighlighter {
    fn default() -> Self {
        NumberHighlighter::new()
    }
}

impl Highlighter for NumberHighlighter {
    fn highlight(&self, text: &mut Text) {
        let plain = text.plain().to_string();
        let bytes = plain.as_bytes();
        let mut i = 0;
        while i < bytes.len() {
            if bytes[i].is_ascii_digit() {
                let start = i;
                while i < bytes.len() && bytes[i].is_ascii_digit() {
                    i += 1;
                }
                text.stylize(self.style.clone(), start, i);
            } else {
                i += 1;
            }
        }
    }
}
