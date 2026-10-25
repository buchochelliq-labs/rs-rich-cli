//! Error types.
//!
//! Port of the exceptions in upstream `rich/errors.py`. Upstream's classes
//! form a small hierarchy (`ConsoleError` and `StyleError` at the root); here
//! each class is a [`RichError`] variant, and [`RichError::is_console_error`]
//! and [`RichError::is_style_error`] answer upstream's `isinstance` checks.
//! As more of the library is ported, additional variants are added here rather
//! than scattering ad-hoc error types across modules.

use std::fmt;

/// Errors produced while parsing colors, styles, or markup.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RichError {
    /// A color could not be parsed (`rich.color.ColorParseError`).
    ColorParse(String),
    /// A style definition could not be parsed (`rich.errors.StyleSyntaxError`).
    StyleSyntax(String),
    /// Console markup was malformed (`rich.errors.MarkupError`).
    Markup(String),
    /// A JSON string could not be parsed (used by `rich.json.JSON`).
    Json(String),
    /// A regular expression could not be compiled or run. Python has no rich
    /// equivalent — it lets `re.error` propagate — but a caller-supplied pattern
    /// has to fail somewhere, and returning it beats panicking.
    Regex(String),
    /// The console's base theme cannot be popped (`rich.theme.ThemeStackError`).
    ThemeStack(String),
    /// A theme config file could not be read or parsed. Upstream surfaces
    /// Python's `configparser` errors (`NoSectionError`, `DuplicateOptionError`,
    /// `ParsingError`, `InterpolationSyntaxError`) and `OSError` here.
    ThemeConfig(String),
    /// A screen update without the alternate screen
    /// (`rich.errors.NoAltScreen`).
    NoAltScreen(String),
    /// An error in console operation (`rich.errors.ConsoleError`).
    Console(String),
    /// An error in styles (`rich.errors.StyleError`).
    Style(String),
    /// No such style (`rich.errors.MissingStyle`).
    MissingStyle(String),
    /// The style stack is invalid (`rich.errors.StyleStackError`).
    StyleStack(String),
    /// An object is not renderable (`rich.errors.NotRenderableError`).
    NotRenderable(String),
    /// An error related to a live display (`rich.errors.LiveError`).
    Live(String),
}

impl RichError {
    /// The message upstream's exception carries, which is what `str(error)`
    /// prints there. [`Display`](fmt::Display) prefixes it with the kind.
    pub fn message(&self) -> &str {
        match self {
            RichError::ColorParse(message)
            | RichError::StyleSyntax(message)
            | RichError::Markup(message)
            | RichError::Json(message)
            | RichError::Regex(message)
            | RichError::ThemeStack(message)
            | RichError::ThemeConfig(message)
            | RichError::NoAltScreen(message)
            | RichError::Console(message)
            | RichError::Style(message)
            | RichError::MissingStyle(message)
            | RichError::StyleStack(message)
            | RichError::NotRenderable(message)
            | RichError::Live(message) => message,
        }
    }

    /// Whether upstream's class derives from `ConsoleError`: `ConsoleError`
    /// itself, `StyleSyntaxError`, `StyleStackError`, `NotRenderableError`,
    /// `MarkupError`, `LiveError` and `NoAltScreen`.
    pub fn is_console_error(&self) -> bool {
        matches!(
            self,
            RichError::Console(_)
                | RichError::StyleSyntax(_)
                | RichError::StyleStack(_)
                | RichError::NotRenderable(_)
                | RichError::Markup(_)
                | RichError::Live(_)
                | RichError::NoAltScreen(_)
        )
    }

    /// Whether upstream's class derives from `StyleError`: `StyleError`
    /// itself and `MissingStyle`.
    pub fn is_style_error(&self) -> bool {
        matches!(self, RichError::Style(_) | RichError::MissingStyle(_))
    }
}

impl fmt::Display for RichError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RichError::ColorParse(msg) => write!(f, "color parse error: {msg}"),
            RichError::StyleSyntax(msg) => write!(f, "style syntax error: {msg}"),
            RichError::Markup(msg) => write!(f, "markup error: {msg}"),
            RichError::Json(msg) => write!(f, "json parse error: {msg}"),
            RichError::Regex(msg) => write!(f, "regex error: {msg}"),
            RichError::ThemeStack(msg) => write!(f, "theme stack error: {msg}"),
            RichError::ThemeConfig(msg) => write!(f, "theme config error: {msg}"),
            RichError::NoAltScreen(msg) => write!(f, "{msg}"),
            RichError::Console(msg) => write!(f, "console error: {msg}"),
            RichError::Style(msg) => write!(f, "style error: {msg}"),
            RichError::MissingStyle(msg) => write!(f, "missing style: {msg}"),
            RichError::StyleStack(msg) => write!(f, "style stack error: {msg}"),
            RichError::NotRenderable(msg) => write!(f, "not renderable: {msg}"),
            RichError::Live(msg) => write!(f, "live error: {msg}"),
        }
    }
}

impl std::error::Error for RichError {}

/// Convenience alias mirroring the subset of `rich` that raises on bad input.
pub type Result<T> = std::result::Result<T, RichError>;

#[cfg(test)]
mod tests {
    use super::*;

    /// The hierarchy of `rich/errors.py`.
    #[test]
    fn hierarchy_matches_upstream() {
        assert!(RichError::Markup(String::new()).is_console_error());
        assert!(RichError::StyleSyntax(String::new()).is_console_error());
        assert!(!RichError::StyleSyntax(String::new()).is_style_error());
        assert!(RichError::MissingStyle(String::new()).is_style_error());
        assert!(!RichError::MissingStyle(String::new()).is_console_error());
        assert_eq!(RichError::Live("x".into()).message(), "x");
    }
}
