//! A conformance kit for [`CodeHighlighter`] adapters.
//!
//! Any adapter, the shipped ones or yours, runs the same checks:
//!
//! ```
//! use rich::SyntectHighlighter;
//! use rich_ext::testing::conformance;
//!
//! conformance::check(SyntectHighlighter::shared()).unwrap();
//! // Your own: `conformance::check(Arc::new(MyHighlighter))`.
//! ```
//!
//! [`check`] reports every failure at once. It checks that:
//! - the default theme is in [`CodeHighlighter::themes`] and highlights (as
//!   does `ansi_dark` if listed, or every theme with [`Options::all_themes`]),
//!   and an unknown theme is [`HighlightError::UnknownTheme`];
//! - the result has one line per `code.split('\n')` element, for empty input,
//!   a missing final newline, CRLF line endings, tabs and multi-byte text;
//! - spans are non-empty, sorted, non-overlapping, inside their line and on
//!   character boundaries;
//! - an unknown language highlights as plain text, exactly as no language;
//! - rendered through [`Syntax`], the output carries no terminal control
//!   characters beyond styling;
//! - a 10,000-line file costs at most a size-proportional multiple of a
//!   1,000-line one: a relative budget, so it is not a wall-clock test.

use std::fmt;
use std::sync::Arc;
use std::time::{Duration, Instant};

use rich::color::ColorSystem;
use rich::protocol::{CodeHighlighter, HighlightError, HighlightedCode};
use rich::syntax::Syntax;
use rich::Console;

/// Sources every check runs over: names describe what each exercises.
const SOURCES: &[(&str, &str)] = &[
    ("empty", ""),
    ("a lone newline", "\n"),
    ("no final newline", "fn main() {\n    let x = 1;\n}"),
    ("a final newline", "def f(x):\n    return x + 1  # one\n"),
    ("CRLF line endings", "fn a() {}\r\nfn b() {}\r\n"),
    ("tabs", "if x {\n\treturn \"a\\tb\";\n}\n"),
    (
        "multi-byte text",
        "let s = \"é 日本 🦀\"; // ünïcode\nprint(\"ok\")\n",
    ),
    ("blank lines", "\n\n\nx = 1\n\n"),
    (
        "an unterminated string",
        "let s = \"never closed\nnext line\n",
    ),
];

/// Languages every check runs with. A name or an extension, as `Syntax` passes.
const LANGUAGES: &[Option<&str>] = &[Some("rust"), Some("python"), Some("rs"), None];

/// A language no adapter knows.
const UNKNOWN_LANGUAGE: &str = "no-such-language-for-conformance";

/// A theme no adapter has.
const UNKNOWN_THEME: &str = "no-such-theme-for-conformance";

/// How much slower 10× the input may be: linear scaling is 10×, and the rest
/// absorbs fixed costs and noise. Quadratic behaviour is 100×.
const SCALING_BUDGET: f64 = 40.0;

/// Below this, the small run is too quick to divide by reliably.
const TIMING_FLOOR: Duration = Duration::from_millis(2);

/// One thing an adapter got wrong.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Failure {
    /// Which check failed, e.g. `"line count"`.
    pub check: &'static str,
    /// What happened, with the input that caused it.
    pub detail: String,
}

impl fmt::Display for Failure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.check, self.detail)
    }
}

/// Every failure [`check`] found.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConformanceError {
    pub failures: Vec<Failure>,
}

impl fmt::Display for ConformanceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(
            f,
            "the adapter failed {} conformance check(s):",
            self.failures.len()
        )?;
        for failure in &self.failures {
            writeln!(f, "- {failure}")?;
        }
        Ok(())
    }
}

impl std::error::Error for ConformanceError {}

/// Which checks to run. [`check`] runs them all.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Options {
    /// Also run every theme the adapter lists (otherwise only its default and,
    /// if listed, `ansi_dark`).
    pub all_themes: bool,
    /// Run the scaling check. It highlights 11,000 lines several times.
    pub scaling: bool,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            all_themes: false,
            scaling: true,
        }
    }
}

/// Run every conformance check against `highlighter`.
pub fn check(highlighter: Arc<dyn CodeHighlighter>) -> Result<(), ConformanceError> {
    check_with(highlighter, Options::default())
}

/// [`check`] with [`Options`].
pub fn check_with(
    highlighter: Arc<dyn CodeHighlighter>,
    options: Options,
) -> Result<(), ConformanceError> {
    let mut failures = Vec::new();
    let mut fail = |check: &'static str, detail: String| failures.push(Failure { check, detail });

    let themes = highlighter.themes();
    let default = highlighter.default_theme().to_string();
    if !themes.contains(&default) {
        fail(
            "themes",
            format!("the default theme {default:?} is not in themes()"),
        );
    }
    let mut chosen: Vec<String> = if options.all_themes {
        themes.clone()
    } else {
        let mut chosen = vec![default.clone()];
        if themes.iter().any(|t| t == "ansi_dark") {
            chosen.push("ansi_dark".into());
        }
        chosen
    };
    chosen.dedup();

    // Unknown themes are errors, not a silent fallback.
    match highlighter.highlight("x = 1", Some("python"), UNKNOWN_THEME) {
        Err(HighlightError::UnknownTheme(name)) if name == UNKNOWN_THEME => {}
        Err(HighlightError::UnknownTheme(name)) => fail(
            "unknown theme",
            format!("UnknownTheme names {name:?}, not {UNKNOWN_THEME:?}"),
        ),
        Err(other) => fail(
            "unknown theme",
            format!("expected UnknownTheme, got the error {other}"),
        ),
        Ok(_) => fail(
            "unknown theme",
            format!("{UNKNOWN_THEME:?} highlighted instead of returning UnknownTheme"),
        ),
    }

    for theme in &chosen {
        for (name, code) in SOURCES {
            for language in LANGUAGES {
                let context = format!("{name}, language {language:?}, theme {theme:?}");
                match highlighter.highlight(code, *language, theme) {
                    Ok(highlighted) => check_shape(code, &highlighted, &context, &mut fail),
                    Err(error) => fail("highlights", format!("{context}: {error}")),
                }
            }
            // An unknown language is plain text: the same as no language.
            let unknown = highlighter.highlight(code, Some(UNKNOWN_LANGUAGE), theme);
            let none = highlighter.highlight(code, None, theme);
            match (unknown, none) {
                (Ok(unknown), Ok(none)) if unknown != none => fail(
                    "unknown language",
                    format!(
                        "{name}, theme {theme:?}: an unknown language differs from no language"
                    ),
                ),
                (Err(error), _) => fail(
                    "unknown language",
                    format!("{name}, theme {theme:?}: an unknown language failed: {error}"),
                ),
                _ => {}
            }
        }
    }

    // Rendered, the output holds nothing but the source and styling.
    let console = Console::builder()
        .width(40)
        .force_terminal(true)
        .color_system(Some(ColorSystem::Truecolor))
        .build();
    for (name, code) in SOURCES {
        let syntax = Syntax::new(*code, "rust")
            .theme(default.clone())
            .highlighter(highlighter.clone());
        let rendered = console.render_to_string(&syntax);
        if let Some(control) = stray_control(&rendered) {
            fail(
                "no control characters",
                format!("{name}: the rendered output contains {control:?}"),
            );
        }
    }

    if options.scaling {
        if let Some(detail) = scaling(highlighter.as_ref(), &default) {
            fail("scaling", detail);
        }
    }

    if failures.is_empty() {
        Ok(())
    } else {
        Err(ConformanceError { failures })
    }
}

/// The line and span contract.
fn check_shape(
    code: &str,
    highlighted: &HighlightedCode,
    context: &str,
    fail: &mut impl FnMut(&'static str, String),
) {
    let lines: Vec<&str> = code.split('\n').collect();
    if highlighted.lines.len() != lines.len() {
        fail(
            "line count",
            format!(
                "{context}: {} lines for {} elements of split('\\n')",
                highlighted.lines.len(),
                lines.len()
            ),
        );
    }
    for (number, (line, text)) in highlighted.lines.iter().zip(&lines).enumerate() {
        let mut end = 0;
        for span in &line.spans {
            let range = &span.range;
            let problem = if range.start >= range.end {
                Some("is empty or reversed")
            } else if range.start < end {
                Some("overlaps or precedes the previous span")
            } else if range.end > text.len() {
                Some("runs past the end of the line")
            } else if !text.is_char_boundary(range.start) || !text.is_char_boundary(range.end) {
                Some("splits a character")
            } else {
                None
            };
            if let Some(problem) = problem {
                fail(
                    "spans",
                    format!("{context}: line {number}, span {range:?} {problem}"),
                );
            }
            end = end.max(range.end);
        }
    }
}

/// A control character in rendered output other than a line break and the
/// escape sequences that style it (CSI `…m`).
fn stray_control(rendered: &str) -> Option<char> {
    let mut chars = rendered.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            if chars.next() != Some('[') {
                return Some(c);
            }
            // Parameters, then the final `m`.
            let mut finished = false;
            for c in chars.by_ref() {
                if c == 'm' {
                    finished = true;
                    break;
                }
                if !(c.is_ascii_digit() || c == ';') {
                    return Some(c);
                }
            }
            if !finished {
                return Some('\u{1b}');
            }
        } else if c.is_control() && c != '\n' {
            return Some(c);
        }
    }
    None
}

/// The time to highlight 10,000 lines against 1,000, as a ratio; `None` when
/// it is within budget.
fn scaling(highlighter: &dyn CodeHighlighter, theme: &str) -> Option<String> {
    let block = "fn f(x: u32) -> u32 {\n    // add one\n    let s = \"text\";\n    x + 1\n}\n";
    let small = block.repeat(200);
    let large = block.repeat(2_000);
    let time = |code: &str| {
        (0..3)
            .map(|_| {
                let started = Instant::now();
                let _ = highlighter.highlight(code, Some("rust"), theme);
                started.elapsed()
            })
            .min()
            .unwrap_or_default()
    };
    let _ = time(&small); // warm up any lazy loading
    let small_time = time(&small).max(TIMING_FLOOR);
    let large_time = time(&large);
    let ratio = large_time.as_secs_f64() / small_time.as_secs_f64();
    (ratio > SCALING_BUDGET).then(|| {
        format!(
            "10,000 lines took {ratio:.1}× as long as 1,000 ({large_time:?} against \
             {small_time:?}); the budget is {SCALING_BUDGET}×"
        )
    })
}
