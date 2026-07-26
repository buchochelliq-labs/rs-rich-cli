//! Terminal control codes.
//!
//! Port of upstream `rich/control.py`. A [`Control`] is a renderable that emits
//! a non-printable control sequence (cursor movement, screen clear, show/hide
//! cursor, alt-screen toggle). It renders to a single *control* [`Segment`],
//! which the [`Console`](crate::console::Console) writes verbatim only when the
//! output is a real terminal (control codes are meaningless when captured to a
//! file).

use crate::console::{Console, ConsoleOptions};
use crate::protocol::Renderable;
use crate::segment::Segment;

/// Non-printable control codes which typically translate to ANSI sequences.
///
/// Port of `rich.segment.ControlType`. The parameterized variants carry their
/// integer arguments so a [`Control`] can be built and its escape string
/// derived deterministically.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControlType {
    Bell,
    CarriageReturn,
    Home,
    Clear,
    ShowCursor,
    HideCursor,
    EnableAltScreen,
    DisableAltScreen,
    CursorUp(u32),
    CursorDown(u32),
    CursorForward(u32),
    CursorBackward(u32),
    /// Move to a zero-based column (rendered as `column + 1`).
    CursorMoveToColumn(u32),
    /// Move to an absolute zero-based `(x, y)` (rendered as `y + 1;x + 1`).
    CursorMoveTo(u32, u32),
    /// Erase in line with the given mode parameter.
    EraseInLine(u32),
}

impl ControlType {
    /// The ANSI/VT escape string for this code. Port of `CONTROL_CODES_FORMAT`.
    fn format(self) -> String {
        match self {
            ControlType::Bell => "\x07".to_string(),
            ControlType::CarriageReturn => "\r".to_string(),
            ControlType::Home => "\x1b[H".to_string(),
            ControlType::Clear => "\x1b[2J".to_string(),
            ControlType::EnableAltScreen => "\x1b[?1049h".to_string(),
            ControlType::DisableAltScreen => "\x1b[?1049l".to_string(),
            ControlType::ShowCursor => "\x1b[?25h".to_string(),
            ControlType::HideCursor => "\x1b[?25l".to_string(),
            ControlType::CursorUp(n) => format!("\x1b[{n}A"),
            ControlType::CursorDown(n) => format!("\x1b[{n}B"),
            ControlType::CursorForward(n) => format!("\x1b[{n}C"),
            ControlType::CursorBackward(n) => format!("\x1b[{n}D"),
            ControlType::CursorMoveToColumn(x) => format!("\x1b[{}G", x + 1),
            ControlType::CursorMoveTo(x, y) => format!("\x1b[{};{}H", y + 1, x + 1),
            ControlType::EraseInLine(n) => format!("\x1b[{n}K"),
        }
    }
}

/// A renderable that inserts terminal control codes.
///
/// Mirrors `rich.control.Control`. Construct it via the factory methods
/// ([`Control::clear`], [`Control::move`], …) or [`Control::new`] with an
/// explicit list of codes, which are concatenated in order.
pub struct Control {
    segment: Segment,
}

impl Control {
    /// Build a control from a sequence of codes, rendered end to end.
    pub fn new(codes: &[ControlType]) -> Self {
        let text: String = codes.iter().map(|c| c.format()).collect();
        Control {
            segment: Segment::control(text),
        }
    }

    fn single(code: ControlType) -> Self {
        Control::new(&[code])
    }

    /// Ring the terminal bell.
    pub fn bell() -> Self {
        Control::single(ControlType::Bell)
    }

    /// Move the cursor to the home position (top-left).
    pub fn home() -> Self {
        Control::single(ControlType::Home)
    }

    /// Clear the screen.
    pub fn clear() -> Self {
        Control::single(ControlType::Clear)
    }

    /// Move the cursor relative to its current position (`x` columns, `y` rows;
    /// positive is right/down). Port of `Control.move`.
    #[allow(clippy::should_implement_trait)]
    pub fn move_(x: i32, y: i32) -> Self {
        let mut codes = Vec::new();
        if x != 0 {
            codes.push(if x > 0 {
                ControlType::CursorForward(x.unsigned_abs())
            } else {
                ControlType::CursorBackward(x.unsigned_abs())
            });
        }
        if y != 0 {
            codes.push(if y > 0 {
                ControlType::CursorDown(y.unsigned_abs())
            } else {
                ControlType::CursorUp(y.unsigned_abs())
            });
        }
        Control::new(&codes)
    }

    /// Move to a zero-based column, optionally offset the row by `y`. Port of
    /// `Control.move_to_column`.
    pub fn move_to_column(x: u32, y: i32) -> Self {
        if y != 0 {
            let vertical = if y > 0 {
                ControlType::CursorDown(y.unsigned_abs())
            } else {
                ControlType::CursorUp(y.unsigned_abs())
            };
            Control::new(&[ControlType::CursorMoveToColumn(x), vertical])
        } else {
            Control::single(ControlType::CursorMoveToColumn(x))
        }
    }

    /// Move the cursor to an absolute zero-based `(x, y)` position.
    pub fn move_to(x: u32, y: u32) -> Self {
        Control::single(ControlType::CursorMoveTo(x, y))
    }

    /// Show or hide the cursor.
    pub fn show_cursor(show: bool) -> Self {
        Control::single(if show {
            ControlType::ShowCursor
        } else {
            ControlType::HideCursor
        })
    }

    /// Enable or disable the terminal's alternate screen buffer.
    pub fn alt_screen(enable: bool) -> Self {
        Control::single(if enable {
            ControlType::EnableAltScreen
        } else {
            ControlType::DisableAltScreen
        })
    }

    /// The raw escape string this control emits.
    pub fn as_str(&self) -> &str {
        &self.segment.text
    }
}

impl Renderable for Control {
    fn rich_render(&self, _console: &Console, _options: &ConsoleOptions) -> Vec<Segment> {
        if self.segment.text.is_empty() {
            Vec::new()
        } else {
            vec![self.segment.clone()]
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // All expected strings captured from real Python `rich` 15.0.0.
    #[test]
    fn escape_strings_match_upstream() {
        assert_eq!(Control::clear().as_str(), "\x1b[2J");
        assert_eq!(Control::home().as_str(), "\x1b[H");
        assert_eq!(Control::bell().as_str(), "\x07");
        assert_eq!(Control::show_cursor(true).as_str(), "\x1b[?25h");
        assert_eq!(Control::show_cursor(false).as_str(), "\x1b[?25l");
        assert_eq!(Control::move_(2, -1).as_str(), "\x1b[2C\x1b[1A");
        assert_eq!(Control::move_to(3, 4).as_str(), "\x1b[5;4H");
        assert_eq!(Control::move_to_column(5, 0).as_str(), "\x1b[6G");
        assert_eq!(Control::alt_screen(true).as_str(), "\x1b[?1049h");
        assert_eq!(Control::alt_screen(false).as_str(), "\x1b[?1049l");
    }

    #[test]
    fn renders_control_segment() {
        let control = Control::clear();
        let segments = control.rich_render(&Console::new(), &Console::new().options());
        assert_eq!(segments.len(), 1);
        assert!(segments[0].control);
        assert_eq!(segments[0].cell_length(), 0);
    }
}
