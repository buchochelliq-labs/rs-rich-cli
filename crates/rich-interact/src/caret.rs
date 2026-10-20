//! The text caret's shape: a block, a bar or an underline, set on the
//! terminal's own cursor with DECSCUSR (`CSI n SP q`), which xterm, the
//! kitty, WezTerm, iTerm2, Windows Terminal, foot, Alacritty and the VTE
//! terminals follow. A terminal that does not know it ignores it.
//!
//! ```
//! use rich_interact::caret::{CaretShape, RESET};
//!
//! assert_eq!(CaretShape::Bar.sequence(), "\x1b[6 q");
//! assert_eq!(CaretShape::Block.blinking(), "\x1b[1 q");
//! assert_eq!(RESET, "\x1b[0 q");
//! ```

/// Puts the terminal's cursor back to the shape its user chose.
pub const RESET: &str = "\x1b[0 q";

/// How the caret looks.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum CaretShape {
    /// A cell-sized block over the character after the caret.
    #[default]
    Block,
    /// A thin bar before the character after the caret, as most editors
    /// show it while typing.
    Bar,
    /// A line under the character after the caret.
    Underline,
}

impl CaretShape {
    /// DECSCUSR for the shape, steady.
    pub fn sequence(self) -> &'static str {
        match self {
            CaretShape::Block => "\x1b[2 q",
            CaretShape::Underline => "\x1b[4 q",
            CaretShape::Bar => "\x1b[6 q",
        }
    }

    /// DECSCUSR for the shape, blinking.
    pub fn blinking(self) -> &'static str {
        match self {
            CaretShape::Block => "\x1b[1 q",
            CaretShape::Underline => "\x1b[3 q",
            CaretShape::Bar => "\x1b[5 q",
        }
    }

    /// The shape by name: `block`, `bar` (or `beam`, `line`) or
    /// `underline` (or `underscore`).
    pub fn parse(name: &str) -> Option<CaretShape> {
        match name.trim().to_ascii_lowercase().as_str() {
            "block" => Some(CaretShape::Block),
            "bar" | "beam" | "line" => Some(CaretShape::Bar),
            "underline" | "underscore" => Some(CaretShape::Underline),
            _ => None,
        }
    }
}
