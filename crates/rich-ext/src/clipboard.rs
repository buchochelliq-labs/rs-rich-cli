//! Copy to the terminal's clipboard with OSC 52 (#488), and the text a
//! table cell or row copies as (#434).
//!
//! OSC 52 asks the terminal to put text on the system clipboard:
//! `ESC ] 52 ; c ; <base64> BEL`. It works over SSH, since the terminal does
//! the copying, but a terminal that does not support it prints nothing or,
//! worse, garbage, and many cap the payload. So nothing is written blindly:
//! [`detect`] decides, with provenance, like every other capability in
//! [`capabilities`](crate::capabilities).
//!
//! # Detection rules
//!
//! The first rule that matches decides:
//!
//! * `RICH_CLIPBOARD=0|1` (also `true/false/yes/no/on/off`) — the override;
//! * not a terminal → no: a pipe, a log or an export never gets the escape;
//! * `TERM=dumb` → no;
//! * inside tmux or screen → no: tmux only passes OSC 52 on with
//!   `set-clipboard on`, which is not its default. Override with
//!   `RICH_CLIPBOARD=1` when yours does;
//! * a terminal known to accept it → yes: kitty, iTerm2, WezTerm, Windows
//!   Terminal, ghostty, Alacritty, foot, contour, rio and VS Code's terminal;
//! * anything else → no.
//!
//! ```
//! use rich_ext::capabilities::MapEnvironment;
//! use rich_ext::clipboard::{self, Clipboard};
//!
//! let kitty = MapEnvironment::tty().var("TERM", "xterm-kitty");
//! assert!(clipboard::detect(&kitty).value);
//! // A pipe never gets the escape, whatever the terminal.
//! assert!(!clipboard::detect(&kitty.clone().terminal(false)).value);
//!
//! let mut out = Vec::new();
//! Clipboard::detect(&kitty).copy(&mut out, "hi").unwrap();
//! assert_eq!(out, b"\x1b]52;c;aGk=\x07");
//! ```

use std::fmt;
use std::io::Write;

use crate::capabilities::{parse_bool, Environment, Field, Origin, SystemEnvironment};

/// The variable that turns OSC 52 on (`1`) or off (`0`), whatever the
/// terminal.
pub const CLIPBOARD_VAR: &str = "RICH_CLIPBOARD";

/// The most bytes one copy sends: 74,994 bytes are 100,000 in base64, the
/// smallest limit among the terminals that cap OSC 52 (hterm, and xterm's
/// default). A larger copy is refused rather than cut.
pub const MAX_BYTES: usize = 74_994;

/// Terminals that take OSC 52 by default, by `TERM_PROGRAM`.
const PROGRAMS: &[&str] = &["iTerm.app", "WezTerm", "ghostty", "vscode", "rio", "contour"];

/// Terminals that take OSC 52 by default, by the start of `TERM`.
const TERMS: &[&str] = &[
    "xterm-kitty",
    "xterm-ghostty",
    "alacritty",
    "foot",
    "contour",
    "wezterm",
    "rio",
];

/// Whether copies reach the clipboard here, and why.
pub fn detect(env: &dyn Environment) -> Field<bool> {
    let get = |name: &str| env.var(name).filter(|value| !value.is_empty());
    let field = |value: bool, origin: Origin, reason: String| Field {
        value,
        origin,
        reason,
    };
    let var = |name: &str| Origin::Environment(name.to_string());
    if let Some(raw) = get(CLIPBOARD_VAR) {
        if let Some(value) = parse_bool(&raw) {
            return field(value, var(CLIPBOARD_VAR), format!("{CLIPBOARD_VAR}={raw}"));
        }
    }
    if !env.is_terminal() {
        return field(false, Origin::Inferred, "not a terminal".into());
    }
    let term = get("TERM").unwrap_or_default();
    let lower = term.to_ascii_lowercase();
    if lower == "dumb" {
        return field(false, var("TERM"), "TERM=dumb".into());
    }
    if get("TMUX").is_some() {
        return field(false, var("TMUX"), "inside tmux".into());
    }
    if lower.starts_with("screen") || lower.starts_with("tmux") {
        return field(false, var("TERM"), "inside a multiplexer".into());
    }
    if get("KITTY_WINDOW_ID").is_some() {
        return field(true, var("KITTY_WINDOW_ID"), "kitty".into());
    }
    if get("WT_SESSION").is_some() {
        return field(true, var("WT_SESSION"), "Windows Terminal".into());
    }
    if let Some(program) = get("TERM_PROGRAM") {
        if PROGRAMS.iter().any(|p| program.eq_ignore_ascii_case(p)) {
            return field(
                true,
                var("TERM_PROGRAM"),
                format!("TERM_PROGRAM={program}"),
            );
        }
    }
    if TERMS.iter().any(|t| lower.starts_with(t)) {
        return field(true, var("TERM"), format!("TERM={term}"));
    }
    if get("ALACRITTY_WINDOW_ID").is_some() {
        return field(true, var("ALACRITTY_WINDOW_ID"), "Alacritty".into());
    }
    field(
        false,
        Origin::Default,
        "terminal not known to accept OSC 52".into(),
    )
}

/// Why a copy did not happen.
#[derive(Debug)]
pub enum ClipboardError {
    /// Detection said no; the reason says why.
    Unsupported(String),
    /// The text is over [`MAX_BYTES`].
    TooLarge { bytes: usize },
    /// Writing the sequence failed.
    Io(std::io::Error),
}

impl fmt::Display for ClipboardError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ClipboardError::Unsupported(reason) => {
                write!(f, "the terminal clipboard is off ({reason}; set {CLIPBOARD_VAR}=1 to force it)")
            }
            ClipboardError::TooLarge { bytes } => write!(
                f,
                "{bytes} bytes is too much to copy through the terminal (at most {MAX_BYTES})"
            ),
            ClipboardError::Io(error) => write!(f, "could not copy: {error}"),
        }
    }
}

impl std::error::Error for ClipboardError {}

/// The terminal clipboard, as detected once.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Clipboard {
    enabled: Field<bool>,
}

impl Clipboard {
    /// Detect from `env`.
    pub fn detect(env: &dyn Environment) -> Clipboard {
        Clipboard {
            enabled: detect(env),
        }
    }

    /// Detect from the process: `RICH_CLIPBOARD`, standard output's
    /// terminal and the terminal's identity.
    pub fn system() -> Clipboard {
        Clipboard::detect(&SystemEnvironment)
    }

    /// On or off, whatever the environment says.
    pub fn forced(on: bool) -> Clipboard {
        Clipboard {
            enabled: Field {
                value: on,
                origin: Origin::Override,
                reason: "override".into(),
            },
        }
    }

    /// Whether copies are written.
    pub fn enabled(&self) -> bool {
        self.enabled.value
    }

    /// The decision, with where it came from.
    pub fn field(&self) -> &Field<bool> {
        &self.enabled
    }

    /// Write the OSC 52 sequence that copies `text` to `out`, and flush.
    pub fn copy(&self, out: &mut dyn Write, text: &str) -> Result<(), ClipboardError> {
        if !self.enabled.value {
            return Err(ClipboardError::Unsupported(self.enabled.reason.clone()));
        }
        let sequence = osc52(text)?;
        out.write_all(sequence.as_bytes())
            .and_then(|()| out.flush())
            .map_err(ClipboardError::Io)
    }
}

/// The OSC 52 sequence that puts `text` on the clipboard, whether or not
/// anything supports it: `ESC ] 52 ; c ; <base64> BEL`.
pub fn osc52(text: &str) -> Result<String, ClipboardError> {
    if text.len() > MAX_BYTES {
        return Err(ClipboardError::TooLarge { bytes: text.len() });
    }
    Ok(format!("\x1b]52;c;{}\x07", base64(text.as_bytes())))
}

/// Standard base64, padded.
fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] =
        b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = (u32::from(chunk[0]) << 16)
            | (u32::from(*chunk.get(1).unwrap_or(&0)) << 8)
            | u32::from(*chunk.get(2).unwrap_or(&0));
        for (i, shift) in [18, 12, 6, 0].into_iter().enumerate() {
            if i <= chunk.len() {
                out.push(ALPHABET[(n >> shift) as usize & 63] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

// ---------------------------------------------------------------------------
// What a table's cells copy as (#434).

/// How a copied table cell or row is written.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum CopyFormat {
    /// Cells as they are, separated by tabs: pastes into a spreadsheet.
    #[default]
    Text,
    /// One CSV record (RFC 4180 quoting).
    Csv,
    /// A JSON object keyed by the headings (an array without them), or a
    /// JSON string for one cell.
    Json,
}

impl CopyFormat {
    /// `text`, `csv` or `json`.
    pub fn parse(name: &str) -> Option<CopyFormat> {
        match name.trim().to_ascii_lowercase().as_str() {
            "text" | "tsv" | "plain" => Some(CopyFormat::Text),
            "csv" => Some(CopyFormat::Csv),
            "json" => Some(CopyFormat::Json),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            CopyFormat::Text => "text",
            CopyFormat::Csv => "csv",
            CopyFormat::Json => "json",
        }
    }

    /// The next format, for a key that cycles through them.
    pub fn next(self) -> CopyFormat {
        match self {
            CopyFormat::Text => CopyFormat::Csv,
            CopyFormat::Csv => CopyFormat::Json,
            CopyFormat::Json => CopyFormat::Text,
        }
    }

    /// A row of `cells` under `headers` (which may be empty).
    ///
    /// ```
    /// use rich_ext::clipboard::CopyFormat;
    ///
    /// let headers = ["name", "note"];
    /// let row = ["ada", "says \"hi\", twice"];
    /// assert_eq!(CopyFormat::Text.row(&headers, &row), "ada\tsays \"hi\", twice");
    /// assert_eq!(CopyFormat::Csv.row(&headers, &row), "ada,\"says \"\"hi\"\", twice\"");
    /// assert_eq!(
    ///     CopyFormat::Json.row(&headers, &row),
    ///     r#"{"name": "ada", "note": "says \"hi\", twice"}"#
    /// );
    /// ```
    pub fn row<H: AsRef<str>, C: AsRef<str>>(self, headers: &[H], cells: &[C]) -> String {
        match self {
            CopyFormat::Text => join(cells.iter().map(|c| c.as_ref().to_string()), "\t"),
            CopyFormat::Csv => join(cells.iter().map(|c| csv_field(c.as_ref())), ","),
            CopyFormat::Json if headers.is_empty() => format!(
                "[{}]",
                join(cells.iter().map(|c| json_string(c.as_ref())), ", ")
            ),
            CopyFormat::Json => {
                let fields = cells.iter().enumerate().map(|(i, cell)| {
                    let key = headers
                        .get(i)
                        .map_or_else(|| format!("column {}", i + 1), |h| h.as_ref().to_string());
                    format!("{}: {}", json_string(&key), json_string(cell.as_ref()))
                });
                format!("{{{}}}", join(fields, ", "))
            }
        }
    }

    /// One cell.
    ///
    /// ```
    /// use rich_ext::clipboard::CopyFormat;
    ///
    /// assert_eq!(CopyFormat::Text.cell("a,b"), "a,b");
    /// assert_eq!(CopyFormat::Csv.cell("a,b"), "\"a,b\"");
    /// assert_eq!(CopyFormat::Json.cell("a\nb"), r#""a\nb""#);
    /// ```
    pub fn cell(self, cell: &str) -> String {
        match self {
            CopyFormat::Text => cell.to_string(),
            CopyFormat::Csv => csv_field(cell),
            CopyFormat::Json => json_string(cell),
        }
    }
}

fn join(parts: impl Iterator<Item = String>, separator: &str) -> String {
    parts.collect::<Vec<_>>().join(separator)
}

/// A CSV field, quoted when it holds a comma, a quote or a line break.
fn csv_field(cell: &str) -> String {
    if cell.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", cell.replace('"', "\"\""))
    } else {
        cell.to_string()
    }
}

/// `text` as a JSON string literal, every control character escaped.
pub fn json_string(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capabilities::MapEnvironment;

    #[test]
    fn base64_pads_like_the_standard() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"foob"), "Zm9vYg==");
        assert_eq!(base64("é✓".as_bytes()), "w6ninJM=");
    }

    #[test]
    fn detection_needs_a_terminal_and_a_known_one() {
        let tty = MapEnvironment::tty();
        assert!(!detect(&tty).value, "an unknown terminal");
        assert!(detect(&tty.clone().var("TERM_PROGRAM", "WezTerm")).value);
        assert!(detect(&tty.clone().var("WT_SESSION", "x")).value);
        assert!(detect(&tty.clone().var("TERM", "foot-extra")).value);
        let kitty = tty.clone().var("TERM", "xterm-kitty");
        assert!(!detect(&kitty.clone().var("TMUX", "/tmp/t")).value);
        assert!(!detect(&kitty.clone().terminal(false)).value);
        assert!(!detect(&MapEnvironment::tty().var("TERM", "dumb")).value);
    }

    #[test]
    fn the_variable_overrides_detection_both_ways() {
        let off = MapEnvironment::tty()
            .var("TERM", "xterm-kitty")
            .var(CLIPBOARD_VAR, "0");
        let field = detect(&off);
        assert!(!field.value);
        assert_eq!(field.origin, Origin::Environment(CLIPBOARD_VAR.into()));
        // Forced on even through a pipe: the user said so.
        assert!(detect(&MapEnvironment::new().var(CLIPBOARD_VAR, "yes")).value);
        // Nonsense is ignored.
        assert!(!detect(&MapEnvironment::tty().var(CLIPBOARD_VAR, "maybe")).value);
    }

    #[test]
    fn a_disabled_or_oversized_copy_writes_nothing() {
        let mut out = Vec::new();
        let error = Clipboard::forced(false).copy(&mut out, "x").unwrap_err();
        assert!(matches!(error, ClipboardError::Unsupported(_)));
        let big = "x".repeat(MAX_BYTES + 1);
        let error = Clipboard::forced(true).copy(&mut out, &big).unwrap_err();
        assert!(matches!(error, ClipboardError::TooLarge { .. }));
        assert!(out.is_empty());
    }

    #[test]
    fn rows_without_headings_are_arrays() {
        let none: [&str; 0] = [];
        assert_eq!(CopyFormat::Json.row(&none, &["a", "b"]), r#"["a", "b"]"#);
        assert_eq!(
            CopyFormat::Json.row(&["k"], &["a", "b"]),
            r#"{"k": "a", "column 2": "b"}"#
        );
        assert_eq!(CopyFormat::Csv.row(&none, &["a\nb", "c"]), "\"a\nb\",c");
        assert_eq!(CopyFormat::Json.cell("\u{1b}"), r#""\u001b""#);
    }
}
