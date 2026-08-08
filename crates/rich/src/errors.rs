//! Error types.
//!
//! Port of the exceptions in upstream `rich/errors.py`. As more of the library
//! is ported, additional variants are added here rather than scattering ad-hoc
//! error types across modules.

use std::fmt;

/// Errors produced while parsing colors, styles, or markup.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RichError {
    /// A color could not be parsed (`rich.errors.ColorParseError`).
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
}

impl fmt::Display for RichError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RichError::ColorParse(msg) => write!(f, "color parse error: {msg}"),
            RichError::StyleSyntax(msg) => write!(f, "style syntax error: {msg}"),
            RichError::Markup(msg) => write!(f, "markup error: {msg}"),
            RichError::Json(msg) => write!(f, "json parse error: {msg}"),
            RichError::Regex(msg) => write!(f, "regex error: {msg}"),
        }
    }
}

impl std::error::Error for RichError {}

/// Convenience alias mirroring the subset of `rich` that raises on bad input.
pub type Result<T> = std::result::Result<T, RichError>;
