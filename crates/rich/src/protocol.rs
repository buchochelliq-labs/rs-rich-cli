//! Rendering & extension protocols.
//!
//! Port of upstream `rich/protocol.py` + `rich/abc.py` + the highlighter
//! interface. **These traits are the sanctioned extension points of the port.**
//! Extensions in `rich-ext` (and, later, third-party plugins) implement them;
//! the faithful core only ever ships upstream's built-in implementations. See
//! docs/PLUGINS.md.

use crate::console::{Console, ConsoleOptions};
use crate::measure::Measurement;
use crate::segment::Segment;
use crate::text::Text;

/// Anything that can be rendered to a stream of [`Segment`]s within a width.
///
/// The Rust equivalent of upstream's `__rich_console__(console, options)`
/// protocol. Implement it to make a custom type printable by [`Console`]. The
/// `options` carry the available width (and, later, height/justify) the
/// renderable must fit into. Newlines between lines are emitted as ordinary
/// segments containing `\n`.
pub trait Renderable {
    fn rich_render(&self, console: &Console, options: &ConsoleOptions) -> Vec<Segment>;

    /// The `(minimum, maximum)` cell width this renderable wants. The default
    /// assumes the renderable fills the available width (e.g. `Panel`, `Table`);
    /// `Text` overrides it with its content width so the top-level print path can
    /// shrink to fit. Port of `__rich_measure__` / `Measurement.get`.
    fn measure(&self, _console: &Console, options: &ConsoleOptions) -> Measurement {
        Measurement::new(options.max_width, options.max_width)
    }

    /// Whether a top-level `Console::print` shrinks this renderable to its
    /// measured width. Upstream renders every top-level renderable at the full
    /// console width, so the default is `false`; an extension may opt in.
    fn fit_to_measurement(&self) -> bool {
        false
    }

    /// The `Text` a top-level print renders in place of this renderable.
    ///
    /// Upstream's `Console._collect_renderables` rebuilds printed `str`/`Text`
    /// values through `Text(sep, end=end).join(...)`, whose `blank_copy` takes
    /// `justify`, `overflow` and `no_wrap` from the separator. Only `Text`
    /// overrides this.
    #[doc(hidden)]
    fn printed_text(&self) -> Option<Text> {
        None
    }

    /// The vertical alignment a `Table` cell holding this renderable uses in
    /// place of its column's. Upstream reads `getattr(renderable, "vertical",
    /// None)`; [`Align`](crate::align::Align) sets it.
    fn vertical(&self) -> Option<crate::align::VerticalAlign> {
        None
    }
}

/// Optional line-streaming extension point for renderables.
///
/// Mirrors the incremental consumption of upstream's rendering generators.
/// Consumers can write each visual line immediately instead of collecting the
/// complete segment stream. Implementations may still retain source data for
/// measurement. This trait keeps streaming hooks out of inherent core APIs.
pub trait LineRenderable: Renderable {
    /// Emit styled visual lines without trailing newlines, stopping immediately
    /// on the callback's first error. An empty segment represents a blank line;
    /// calling the callback zero times represents no output.
    fn try_for_each_line<E>(
        &self,
        console: &Console,
        options: &ConsoleOptions,
        emit: impl FnMut(Vec<Segment>) -> Result<(), E>,
    ) -> Result<(), E>;
}

/// Transfer already-owned table rows without cloning every cell string.
///
/// This extension point changes ownership only. Column definitions, measurement
/// and rendering follow the table's existing rules, including missing/extra cells.
/// Producers that parse into owned strings can release their row collection as
/// they populate a table instead of retaining a second complete copy.
pub trait OwnedTableRows {
    fn extend_owned_rows(&mut self, rows: Vec<Vec<String>>) -> &mut Self;
}

/// A transformer that adds style spans to [`Text`] (e.g. syntax/number/URL
/// highlighting). The Rust equivalent of upstream's `Highlighter` ABC.
///
/// This is the primary *plugin* seam for the first slice: `rich-ext` registers
/// [`Highlighter`]s onto a [`Console`] without the core knowing they exist.
pub trait Highlighter {
    /// Inspect `text` and apply any style spans in place.
    fn highlight(&self, text: &mut Text);
}

/// One styled byte range of a highlighted line.
#[derive(Clone, Debug, PartialEq)]
pub struct HighlightSpan {
    /// Byte range within the line (the line's text, without its `\n`).
    pub range: std::ops::Range<usize>,
    pub style: crate::style::Style,
}

/// The spans of one source line, plus the style of the line break after it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct HighlightedLine {
    /// Sorted, non-overlapping spans. Bytes no span covers take
    /// [`HighlightedCode::default_style`].
    pub spans: Vec<HighlightSpan>,
    /// The style an engine gives the `\n` ending this line (for example, inside a
    /// multi-line string). `None` when the engine does not style line breaks.
    pub newline_style: Option<crate::style::Style>,
}

/// What a [`CodeHighlighter`] returns: one [`HighlightedLine`] per element of
/// `code.split('\n')`, so a trailing newline yields a final empty line.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct HighlightedCode {
    pub lines: Vec<HighlightedLine>,
    /// The theme's background, if it has one. `Syntax` paints its block with it.
    pub background: Option<crate::color::Color>,
    /// The style for text no span covers.
    pub default_style: crate::style::Style,
}

/// Why a [`CodeHighlighter`] could not highlight. An unknown *language* is not an
/// error: highlighters fall back to plain text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HighlightError {
    /// The theme name is not one this highlighter provides.
    UnknownTheme(String),
    /// The engine failed on this input.
    Engine(String),
}

impl std::fmt::Display for HighlightError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HighlightError::UnknownTheme(name) => write!(f, "unknown syntax theme {name:?}"),
            HighlightError::Engine(message) => write!(f, "syntax highlighting failed: {message}"),
        }
    }
}

impl std::error::Error for HighlightError {}

/// A syntax-highlighting engine behind [`Syntax`](crate::syntax::Syntax) and
/// Markdown code blocks.
///
/// Upstream highlights with Pygments; the port's default is
/// [`SyntectHighlighter`](crate::syntax::SyntectHighlighter), and anything that
/// implements this trait can replace it (see [`Syntax::highlighter`](crate::syntax::Syntax::highlighter)).
///
/// # Contract
///
/// - `lines` has one entry per element of `code.split('\n')`.
/// - Spans are sorted, do not overlap, stay inside their line and start and end
///   on UTF-8 character boundaries.
/// - An unknown language highlights as plain text rather than failing.
///
/// Core validates what an implementation returns: out-of-range, overlapping or
/// misaligned spans are dropped, missing lines render unstyled, and span styles
/// lose any hyperlink. The rendered characters always come from the source, so
/// a highlighter cannot add text or terminal control sequences.
///
/// ```
/// use rich::protocol::{CodeHighlighter, HighlightError, HighlightSpan, HighlightedCode, HighlightedLine};
/// use rich::{Console, Style, Syntax};
/// use std::sync::Arc;
///
/// /// Makes every line bold.
/// struct Bold;
///
/// impl CodeHighlighter for Bold {
///     fn highlight(&self, code: &str, _language: Option<&str>, _theme: &str)
///         -> Result<HighlightedCode, HighlightError>
///     {
///         let bold = Style::parse("bold").unwrap();
///         let lines = code
///             .split('\n')
///             .map(|line| HighlightedLine {
///                 spans: vec![HighlightSpan { range: 0..line.len(), style: bold.clone() }]
///                     .into_iter()
///                     .filter(|span| !span.range.is_empty())
///                     .collect(),
///                 newline_style: None,
///             })
///             .collect();
///         Ok(HighlightedCode { lines, ..Default::default() })
///     }
///     fn default_theme(&self) -> &str { "bold" }
///     fn themes(&self) -> Vec<String> { vec!["bold".into()] }
///     fn languages(&self) -> Vec<String> { Vec::new() }
/// }
///
/// let console = Console::builder().width(20).force_terminal(true).build();
/// let out = console.render_to_string(&Syntax::new("x = 1", "python").highlighter(Arc::new(Bold)));
/// assert!(out.contains("\x1b[1mx = 1"));
/// ```
pub trait CodeHighlighter: Send + Sync {
    /// Highlight `code`. `language` is a name or file extension (`"rust"`,
    /// `"rs"`); `None` means plain text. `theme` is one of [`themes`](Self::themes).
    fn highlight(
        &self,
        code: &str,
        language: Option<&str>,
        theme: &str,
    ) -> Result<HighlightedCode, HighlightError>;

    /// The theme used when none is chosen.
    fn default_theme(&self) -> &str;

    /// Every theme name `highlight` accepts.
    fn themes(&self) -> Vec<String>;

    /// Language names this highlighter knows, for help text and completion.
    fn languages(&self) -> Vec<String>;

    /// The language for a file path, if the highlighter recognises it.
    fn language_for_path(&self, _path: &std::path::Path) -> Option<String> {
        None
    }

    /// `theme`'s style for a Pygments token type — `"Text"` or `"Comment"` —
    /// as upstream's `SyntaxTheme.get_style_for_token`. `Syntax` colours its
    /// line numbers and indent guides with it; `None` (the default) means the
    /// theme sets nothing for the token.
    fn token_style(&self, _theme: &str, _token: &str) -> Option<crate::style::Style> {
        None
    }
}

/// Renders fenced Markdown code blocks of particular languages (for example
/// ```` ```mermaid ````) in place of the usual highlighted code.
///
/// Upstream always renders a fence through `Syntax`, and so does
/// [`Markdown`](crate::markdown::Markdown) unless a renderer is added with
/// [`Markdown::fence_renderer`](crate::markdown::Markdown::fence_renderer).
/// Markdown asks each renderer in turn; the first to return `Some` wins, and if
/// none does the block is highlighted as code as before.
///
/// The fence body comes from the document, so treat it as untrusted: whatever
/// text of it an implementation echoes must not carry terminal control
/// sequences.
pub trait FenceRenderer: Send + Sync {
    /// Render the body of a fence whose info string starts with `language`, or
    /// return `None` to decline. The returned segments fit `options.max_width`
    /// and separate lines with `\n` segments, like [`Renderable::rich_render`].
    fn render_fence(
        &self,
        language: &str,
        code: &str,
        console: &Console,
        options: &ConsoleOptions,
    ) -> Option<Vec<Segment>>;
}

/// A console-wide default [`CodeHighlighter`] and the theme to use with it.
/// See [`ConsoleCodeHighlighting`].
#[derive(Clone)]
pub struct CodeHighlighting {
    pub highlighter: std::sync::Arc<dyn CodeHighlighter>,
    /// One of `highlighter`'s themes; `None` is its default theme.
    pub theme: Option<String>,
}

impl std::fmt::Debug for CodeHighlighting {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CodeHighlighting")
            .field("default_theme", &self.highlighter.default_theme())
            .field("theme", &self.theme)
            .finish()
    }
}

/// Attach/query a console's default code highlighter.
///
/// A [`Syntax`](crate::syntax::Syntax) without a highlighter of its own, and
/// so Markdown code blocks, highlights with the console's when it renders;
/// the console's theme applies when the `Syntax` names none. Without one,
/// the default [`SyntectHighlighter`](crate::syntax::SyntectHighlighter) is
/// used, as before: upstream has no such setting.
pub trait ConsoleCodeHighlighting {
    fn set_code_highlighting(&mut self, value: Option<CodeHighlighting>);
    fn code_highlighting(&self) -> Option<&CodeHighlighting>;
}

/// Evidence for an optional output protocol; inference is not confirmation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Support {
    Unsupported,
    Inferred,
    Confirmed,
}

/// Immutable destination capabilities supplied by an extension. No detection or I/O.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TargetCapabilities {
    pub width: usize,
    pub height: usize,
    pub color_system: Option<crate::color::ColorSystem>,
    pub interactive: bool,
    pub unicode: bool,
    pub hyperlinks: bool,
    pub sixel: Support,
}

/// Optional context shared by nested renderables without changing their protocol.
pub trait RenderEnvironment: Send + Sync {
    fn capabilities(&self) -> TargetCapabilities;
}

/// Attach/query a per-console immutable extension environment.
pub trait ConsoleEnvironment {
    fn set_render_environment(&mut self, value: Option<std::sync::Arc<dyn RenderEnvironment>>);
    fn render_environment(&self) -> Option<&dyn RenderEnvironment>;
}
