//! Rendering errors.
//!
//! Rust-native reimagining of `rich/traceback.py`. Upstream renders a *Python*
//! exception traceback (stack frames + highlighted source); Rust errors don't
//! carry frames, so [`Traceback`] instead renders an error's message and its
//! [`Error::source`] chain (`Caused by:`) inside a red-bordered panel — the same
//! "here's what went wrong, clearly" utility.
//!
//! **Divergence:** no stack frames / source code (Rust errors don't expose
//! them). Pair with `std::backtrace::Backtrace` at the call site if you want a
//! frame list. See docs/DIVERGENCES.md #19.

use std::error::Error;

use crate::console::{Console, ConsoleOptions};
use crate::panel::Panel;
use crate::protocol::Renderable;
use crate::r#box::HEAVY;
use crate::segment::Segment;
use crate::style::Style;
use crate::text::Text;

/// A rendered error: the top-level message plus its `Caused by:` chain. Mirrors
/// the role of `rich.traceback.Traceback`.
pub struct Traceback {
    message: String,
    causes: Vec<String>,
}

impl Traceback {
    /// Build from an error, walking its [`Error::source`] chain.
    pub fn new(error: &dyn Error) -> Self {
        let mut causes = Vec::new();
        let mut source = error.source();
        while let Some(err) = source {
            causes.push(err.to_string());
            source = err.source();
        }
        Traceback {
            message: error.to_string(),
            causes,
        }
    }

    /// Build from a plain message (e.g. a captured panic string), with no chain.
    pub fn from_message(message: impl Into<String>) -> Self {
        Traceback {
            message: message.into(),
            causes: Vec::new(),
        }
    }

    /// The styled inner content: the message, then the `Caused by:` chain.
    fn content(&self) -> Text {
        let error_style = Style::parse("bold red").expect("valid style");
        let label_style = Style::parse("dim").expect("valid style");
        let cause_style = Style::parse("red").expect("valid style");

        let mut text = Text::new("");
        text.append(&self.message, Some(error_style));
        if !self.causes.is_empty() {
            text.append("\n\nCaused by:", Some(label_style));
            for (index, cause) in self.causes.iter().enumerate() {
                text.append(&format!("\n  {}: ", index + 1), None);
                text.append(cause, Some(cause_style.clone()));
            }
        }
        text
    }

    fn panel(&self) -> Panel {
        Panel::new(Box::new(self.content()))
            .box_set(HEAVY)
            .border_style(Style::parse("red").expect("valid style"))
            .title("Traceback (most recent call last)")
    }
}

impl Renderable for Traceback {
    fn rich_render(&self, console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
        self.panel().rich_render(console, options)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::color::ColorSystem;
    use std::fmt;

    #[derive(Debug)]
    struct Inner;
    impl fmt::Display for Inner {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(f, "disk full")
        }
    }
    impl Error for Inner {}

    #[derive(Debug)]
    struct Outer;
    static INNER: Inner = Inner;
    impl fmt::Display for Outer {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(f, "failed to save file")
        }
    }
    impl Error for Outer {
        fn source(&self) -> Option<&(dyn Error + 'static)> {
            Some(&INNER)
        }
    }

    fn render(tb: &Traceback) -> String {
        Console::builder()
            .force_terminal(true)
            .color_system(Some(ColorSystem::Truecolor))
            .width(40)
            .no_color(false)
            .build()
            .render_to_string(tb)
    }

    #[test]
    fn renders_error_chain() {
        let out = render(&Traceback::new(&Outer));
        assert!(out.contains("Traceback (most recent call last)"));
        assert!(out.contains("failed to save file"));
        assert!(out.contains("Caused by:"));
        assert!(out.contains("disk full"));
        // Red bordered (SGR 31 present).
        assert!(out.contains("\x1b[31m"), "expected red border/message");
    }

    #[test]
    fn from_message_has_no_chain() {
        let out = render(&Traceback::from_message("something broke"));
        assert!(out.contains("something broke"));
        assert!(!out.contains("Caused by:"));
    }
}
