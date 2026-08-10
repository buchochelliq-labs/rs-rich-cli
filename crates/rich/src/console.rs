//! The Console — the high-level rendering entry point.
//!
//! Port of upstream `rich/console.py` (core subset): terminal / color-system /
//! width detection, markup + highlighter application, and writing styled output.
//! Layout options, capture, export, and paging land in the Console-completeness
//! issue.

use std::io::{IsTerminal, Write};

use crate::color::ColorSystem;
use crate::protocol::{Highlighter, Renderable};
use crate::segment::Segment;
use crate::style::Style;
use crate::text::Text;
use crate::theme::Theme;

const DEFAULT_WIDTH: usize = 80;
const DEFAULT_HEIGHT: usize = 25;

/// Horizontal justification of a renderable within its width.
/// Mirrors `rich.console.JustifyMethod`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Justify {
    /// Renderable-defined default (usually left, no padding).
    #[default]
    Default,
    Left,
    Center,
    Right,
    Full,
}

/// What to do with text that is wider than the space available.
/// Mirrors `rich.console.OverflowMethod`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Overflow {
    /// Break over-long words across lines. Upstream's `DEFAULT_OVERFLOW`.
    #[default]
    Fold,
    /// Cut the line off at the width.
    Crop,
    /// Cut the line off one cell early and mark it with `…`.
    Ellipsis,
    /// Leave over-long lines intact, and do not wrap.
    Ignore,
}

/// The options passed to a [`Renderable`] describing the space it must fit into.
///
/// Port of the core of `rich.console.ConsoleOptions`. Only the fields needed by
/// the currently-ported renderables are present; more are added as widgets land.
#[derive(Debug, Clone)]
pub struct ConsoleOptions {
    pub min_width: usize,
    pub max_width: usize,
    pub height: Option<usize>,
    pub justify: Justify,
    /// Overflow method to impose on renderables, or `None` to let each pick its
    /// own. Mirrors `ConsoleOptions.overflow`.
    pub overflow: Option<Overflow>,
    /// Disable wrapping, or `None` to let each renderable pick. Mirrors
    /// `ConsoleOptions.no_wrap`.
    pub no_wrap: Option<bool>,
}

impl ConsoleOptions {
    /// Return a copy with `max_width` (and a clamped `min_width`) updated.
    /// Port of `ConsoleOptions.update_width`.
    pub fn update_width(&self, width: usize) -> ConsoleOptions {
        // Copy-then-overwrite rather than a fresh literal, so fields added later
        // are carried through instead of being silently reset to a default.
        let mut options = self.clone();
        options.min_width = width;
        options.max_width = width;
        options
    }

    /// Return a copy with both width and height pinned. Port of
    /// `ConsoleOptions.update_dimensions`.
    pub fn update_dimensions(&self, width: usize, height: usize) -> ConsoleOptions {
        let mut options = self.update_width(width);
        options.height = Some(height);
        options
    }
}

/// The high-level interface for rendering to a terminal. Mirrors
/// `rich.console.Console`.
pub struct Console {
    color_system: Option<ColorSystem>,
    width: usize,
    height: usize,
    is_terminal: bool,
    no_color: bool,
    emoji: bool,
    highlight: bool,
    legacy_windows: bool,
    safe_box: bool,
    ascii_only: bool,
    theme: Theme,
    base_style: Style,
    highlighters: Vec<Box<dyn Highlighter + Send>>,
    /// While capturing, print paths append their segments here instead of
    /// writing to stdout. Mirrors `Console._record_buffer` under `capture()`.
    record_buffer: std::cell::RefCell<Vec<Segment>>,
    capturing: std::cell::Cell<bool>,
}

impl Default for Console {
    fn default() -> Self {
        Console::new()
    }
}

impl Console {
    /// Auto-detect terminal capabilities from the environment.
    pub fn new() -> Self {
        ConsoleBuilder::new().build()
    }

    /// Start configuring a console explicitly (used by tests and `rich-ext`).
    pub fn builder() -> ConsoleBuilder {
        ConsoleBuilder::new()
    }

    /// The active color system, or `None` when color is disabled.
    pub fn color_system(&self) -> Option<ColorSystem> {
        if self.no_color {
            None
        } else {
            self.color_system
        }
    }

    /// The detected (or configured) width in cells.
    pub fn width(&self) -> usize {
        self.width
    }

    /// The detected (or configured) height in rows. Used by height-aware
    /// renderables such as [`Layout`](crate::layout::Layout).
    pub fn height(&self) -> usize {
        self.height
    }

    /// Whether output is going to a real terminal.
    pub fn is_terminal(&self) -> bool {
        self.is_terminal
    }

    /// Whether output targets a legacy Windows console (drives box substitution).
    pub fn legacy_windows(&self) -> bool {
        self.legacy_windows
    }

    /// Whether to substitute box glyphs for terminal-safe variants (default on).
    pub fn safe_box(&self) -> bool {
        self.safe_box
    }

    /// Whether the terminal can only render ASCII (forces the `ASCII` box).
    pub fn ascii_only(&self) -> bool {
        self.ascii_only
    }

    /// The active theme.
    pub fn theme(&self) -> &Theme {
        &self.theme
    }

    /// Resolve a style name (or pass a style through) against this console's
    /// theme. Port of `Console.get_style`.
    pub fn get_style(&self, style: &crate::style::StyleType) -> crate::errors::Result<Style> {
        self.theme.get_style(style)
    }

    /// The whole-output base style.
    pub fn base_style(&self) -> &Style {
        &self.base_style
    }

    /// Register a highlighter. **The core plugin seam** — see docs/PLUGINS.md.
    /// The highlighter must be `Send` so a [`Console`](Console) can move to a
    /// background thread (e.g. an auto-refreshing [`Live`](crate::live::Live)).
    pub fn add_highlighter(&mut self, highlighter: Box<dyn Highlighter + Send>) {
        self.highlighters.push(highlighter);
    }

    /// The default render options for this console (full width, no height).
    pub fn options(&self) -> ConsoleOptions {
        ConsoleOptions {
            min_width: 1,
            max_width: self.width,
            height: None,
            justify: Justify::Default,
            overflow: None,
            no_wrap: None,
        }
    }

    /// Render a value to an ANSI string (no trailing newline). Primarily for
    /// tests and inline rendering.
    ///
    /// When no explicit justify is requested, the width is first shrunk to the
    /// renderable's measured width (matching upstream's measurement-fit for a
    /// bare top-level renderable).
    pub fn render_to_string(&self, renderable: &dyn Renderable) -> String {
        let segments = self.render_segments(renderable);
        self.segments_to_string(&segments)
    }

    /// Render a renderable to segments, applying top-level measurement-fit when
    /// no explicit justify is set (shared by the string and print paths).
    fn render_segments(&self, renderable: &dyn Renderable) -> Vec<Segment> {
        let mut options = self.options();
        if options.justify == Justify::Default {
            let measurement = renderable.measure(self, &options);
            options.max_width = measurement.maximum.min(options.max_width).max(1);
        }
        let segments = renderable.rich_render(self, &options);
        // `Console.print(crop=True)`: the final backstop against a line running
        // off the side of the terminal. Renderables that fit are untouched; this
        // is what gives `Overflow::Ignore` its "wrap nothing, but still don't
        // corrupt the display" behaviour.
        Segment::crop_lines(&segments, self.width)
    }

    /// Write (or, while capturing, record) a rendered segment stream, adding a
    /// trailing newline. The single sink for every `print*` path.
    fn emit(&self, segments: Vec<Segment>) {
        if self.capturing.get() {
            let mut buffer = self.record_buffer.borrow_mut();
            buffer.extend(segments);
            buffer.push(Segment::line());
            return;
        }
        let mut output = self.segments_to_string(&segments);
        output.push('\n');
        let stdout = std::io::stdout();
        let mut lock = stdout.lock();
        let _ = write!(lock, "{output}");
    }

    /// Render a value into a list of lines, each a list of [`Segment`]s.
    ///
    /// Port of `Console.render_lines`. When `pad` is true, every line is padded
    /// (or cropped) to `options.max_width` — this is what container renderables
    /// such as `Panel`/`Padding` rely on to get uniform-width child rows.
    pub fn render_lines(
        &self,
        renderable: &dyn Renderable,
        options: &ConsoleOptions,
        pad: bool,
    ) -> Vec<Vec<Segment>> {
        let segments = renderable.rich_render(self, options);
        let mut lines = Segment::split_lines(&segments);
        if pad {
            for line in &mut lines {
                *line = Segment::adjust_line_length(line, options.max_width, Some(Style::new()));
            }
        }
        // Honor an explicit height by cropping/padding to exactly that many rows
        // (matching `Console.render_lines`'s height handling — used by height-
        // aware containers such as `Panel` inside a `Layout`).
        if let Some(height) = options.height {
            lines.truncate(height);
            while lines.len() < height {
                lines.push(if pad {
                    vec![Segment::new(
                        " ".repeat(options.max_width),
                        Some(Style::new()),
                    )]
                } else {
                    Vec::new()
                });
            }
        }
        lines
    }

    /// Render a value exactly as [`print`](Console::print) would write it,
    /// returning the string (including the single trailing newline). For tests
    /// and export.
    pub fn render_export(&self, renderable: &dyn Renderable) -> String {
        let mut out = self.render_to_string(renderable);
        out.push('\n');
        out
    }

    /// Render a value and write it to stdout, followed by a newline.
    pub fn print(&self, renderable: &dyn Renderable) {
        let segments = self.render_segments(renderable);
        self.emit(segments);
    }

    /// Write a terminal control sequence to stdout.
    ///
    /// Port of `Console.control`. Control codes are only written when output is
    /// a real terminal (they are meaningless when redirected to a file).
    pub fn control(&self, control: &crate::control::Control) {
        if !self.is_terminal {
            return;
        }
        let text = control.as_str();
        if !text.is_empty() {
            let stdout = std::io::stdout();
            let mut lock = stdout.lock();
            let _ = write!(lock, "{text}");
        }
    }

    /// Show or hide the cursor. Port of `Console.show_cursor`.
    pub fn show_cursor(&self, show: bool) {
        self.control(&crate::control::Control::show_cursor(show));
    }

    /// Clear the screen. Port of `Console.clear`.
    pub fn clear(&self) {
        self.control(&crate::control::Control::clear());
    }

    /// Ring the terminal bell. Port of `Console.bell`.
    pub fn bell(&self) {
        self.control(&crate::control::Control::bell());
    }

    /// Capture everything printed inside `f` instead of writing it to stdout,
    /// returning it as a rendered (ANSI) string.
    ///
    /// The Rust analogue of upstream's `with console.capture() as capture:` —
    /// the closure receives the same console, and captures nest correctly.
    /// Equivalent to what would have been written to the terminal.
    pub fn capture(&self, f: impl FnOnce(&Console)) -> String {
        let segments = self.record(f);
        self.segments_to_string(&segments)
    }

    /// Like [`capture`](Self::capture) but with all styles stripped, returning
    /// plain text. Port of `Console.export_text(styles=False)`.
    pub fn export_text(&self, f: impl FnOnce(&Console)) -> String {
        let segments = self.record(f);
        segments_to_plain(&segments)
    }

    /// Buffer everything printed inside `f` and display it through the system
    /// pager. The Rust analogue of upstream's `with console.pager():` block.
    ///
    /// Styles are stripped unless `styles` is set, matching
    /// `Console.pager(styles=False)`. When there's no terminal to page in (piped
    /// output, `TERM=dumb`) or no pager can be started, the content is written
    /// straight to stdout.
    pub fn page(&self, styles: bool, f: impl FnOnce(&Console)) -> std::io::Result<()> {
        self.page_with(&crate::pager::SystemPager, styles, f)
    }

    /// Like [`page`](Self::page) but with an explicit [`Pager`](crate::pager::Pager)
    /// — the seam upstream exposes as `Console.pager(pager=…)`.
    pub fn page_with(
        &self,
        pager: &dyn crate::pager::Pager,
        styles: bool,
        f: impl FnOnce(&Console),
    ) -> std::io::Result<()> {
        let segments = self.record(f);
        let content = if styles {
            self.segments_to_string(&segments)
        } else {
            segments_to_plain(&segments)
        };
        pager.show(&content)
    }

    /// Capture output printed inside `f` and export it as a self-contained HTML
    /// document (inline styles), using the default terminal theme. Port of
    /// `Console.export_html(inline_styles=True)`.
    pub fn export_html(&self, f: impl FnOnce(&Console)) -> String {
        self.export_html_themed(&crate::terminal_theme::DEFAULT_TERMINAL_THEME, f)
    }

    /// Like [`export_html`](Self::export_html) but with an explicit palette —
    /// upstream's `export_html(theme=…)`. See [`terminal_theme`] for the
    /// bundled presets.
    ///
    /// [`terminal_theme`]: crate::terminal_theme
    pub fn export_html_themed(
        &self,
        theme: &crate::terminal_theme::TerminalTheme,
        f: impl FnOnce(&Console),
    ) -> String {
        let segments = self.record(f);
        crate::export::export_html_inline(&segments, theme)
    }

    /// Like [`export_html`](Self::export_html) but with a generated CSS-class
    /// stylesheet (`.r1 {…}`) instead of inline styles. Port of upstream's
    /// default `Console.export_html(inline_styles=False)`.
    pub fn export_html_classes(&self, f: impl FnOnce(&Console)) -> String {
        self.export_html_classes_themed(&crate::terminal_theme::DEFAULT_TERMINAL_THEME, f)
    }

    /// Like [`export_html_classes`](Self::export_html_classes) but with an
    /// explicit palette — upstream's `export_html(theme=…, inline_styles=False)`.
    pub fn export_html_classes_themed(
        &self,
        theme: &crate::terminal_theme::TerminalTheme,
        f: impl FnOnce(&Console),
    ) -> String {
        let segments = self.record(f);
        crate::export::export_html_classes(&segments, theme)
    }

    /// Capture output printed inside `f` and export it as a self-contained SVG
    /// image of a terminal window, using [`SVG_EXPORT_THEME`]. Port of
    /// `Console.export_svg`.
    ///
    /// `unique_id` prefixes every generated id/class. Upstream's auto-computed
    /// default hashes Python `repr()` output (not reproducible in Rust), so this
    /// port takes an explicit id; output is byte-parity with
    /// `export_svg(title=…, unique_id=…)` (see docs/DIVERGENCES.md #15).
    ///
    /// [`SVG_EXPORT_THEME`]: crate::terminal_theme::SVG_EXPORT_THEME
    pub fn export_svg(&self, title: &str, unique_id: &str, f: impl FnOnce(&Console)) -> String {
        self.export_svg_themed(
            &crate::terminal_theme::SVG_EXPORT_THEME,
            title,
            unique_id,
            f,
        )
    }

    /// Like [`export_svg`](Self::export_svg) but with an explicit palette —
    /// upstream's `export_svg(theme=…)`.
    pub fn export_svg_themed(
        &self,
        theme: &crate::terminal_theme::TerminalTheme,
        title: &str,
        unique_id: &str,
        f: impl FnOnce(&Console),
    ) -> String {
        let segments = self.record(f);
        crate::svg::export_svg(&segments, theme, title, unique_id, self.width())
    }

    /// Record everything `f` prints and hand back the raw segments, without
    /// writing to the terminal.
    ///
    /// This is the seam for producing *several* outputs from one render — the
    /// terminal bytes and an HTML and an SVG file, say — which is what
    /// `rich --export-html … --export-svg …` needs. Upstream reaches the same
    /// place with `Console(record=True)` plus `save_html(clear=False)`; here the
    /// buffer is returned instead of being held on the console, so the caller
    /// decides what to do with it and there is no hidden state to clear.
    ///
    /// Pair with [`segments_to_string`](Self::segments_to_string) to get the
    /// terminal form, [`export::export_html_classes`](crate::export::export_html_classes)
    /// for HTML, and [`svg::export_svg`](crate::svg::export_svg) for SVG.
    ///
    /// Rendering twice instead would be wrong, not merely wasteful: a renderable
    /// reading standard input only yields its content once.
    pub fn record_output(&self, f: impl FnOnce(&Console)) -> Vec<Segment> {
        self.record(f)
    }

    /// Run `f` with output recorded to a fresh buffer, returning the captured
    /// segments and restoring the previous capture state (so captures nest).
    fn record(&self, f: impl FnOnce(&Console)) -> Vec<Segment> {
        let previous = std::mem::take(&mut *self.record_buffer.borrow_mut());
        let was_capturing = self.capturing.replace(true);
        f(self);
        let captured = std::mem::replace(&mut *self.record_buffer.borrow_mut(), previous);
        self.capturing.set(was_capturing);
        captured
    }

    /// Parse `content` as console markup, apply registered highlighters, and
    /// print it. This is the `console.print("...")` path.
    pub fn print_str(&self, content: &str) {
        let text = self.build_text(content);
        self.print(&text);
    }

    /// Same as [`Console::print_str`] but returns the ANSI string.
    pub fn render_str_to_string(&self, content: &str) -> String {
        let text = self.build_text(content);
        self.render_to_string(&text)
    }

    /// Parse `content` as console markup (expanding emoji + applying the active
    /// highlighters), returning the styled [`Text`] that `print_str` would print.
    /// Exposed so callers can wrap the markup in another renderable.
    pub fn build_text(&self, content: &str) -> Text {
        // Malformed markup falls back to printing the text as-is. Upstream would
        // raise `MarkupError` instead; use `try_build_text` (or `try_print_str`)
        // when the markup comes from a user and a mistake should be reported
        // rather than rendered. See docs/DIVERGENCES.md §2.
        self.try_build_text(content)
            .unwrap_or_else(|_| self.decorate(Text::new(self.expand_emoji(content))))
    }

    /// As [`build_text`](Console::build_text), but returns
    /// [`RichError::Markup`](crate::errors::RichError::Markup) for malformed
    /// markup instead of falling back to the raw text — upstream's behaviour.
    pub fn try_build_text(&self, content: &str) -> crate::errors::Result<Text> {
        let expanded = self.expand_emoji(content);
        let markup = Text::from_markup(&expanded)?;

        // The highlighter runs on the *markup-stripped* text and its spans go on
        // first; the markup spans are appended afterwards. Spans combine in
        // order, so this is what makes an explicit tag beat the highlighter —
        // `[green]123[/]` is green, not `repr.number` cyan.
        //
        // Upstream reaches the same result a different way: `Console.render_str`
        // highlights a fresh `Text(str(rich_text))` and then calls
        // `highlight_text.copy_styles(rich_text)`, whose `_spans.extend` appends
        // the markup spans last. Decorating the markup `Text` in place — the
        // obvious reading — inverts the precedence.
        let mut text = self.decorate(Text::new(markup.plain()));
        for span in markup.spans() {
            text.push_span(span.clone());
        }
        Ok(text)
    }

    /// As [`print_str`](Console::print_str), but reports malformed markup.
    pub fn try_print_str(&self, content: &str) -> crate::errors::Result<()> {
        self.print(&self.try_build_text(content)?);
        Ok(())
    }

    /// As [`print_justified`](Console::print_justified), but reports malformed
    /// markup.
    pub fn try_print_justified(
        &self,
        content: &str,
        justify: Justify,
    ) -> crate::errors::Result<()> {
        let text = self.try_build_text(content)?;
        let mut options = self.options();
        options.justify = justify;
        self.emit(text.rich_render(self, &options));
        Ok(())
    }

    /// Expand `:emoji:` shortcodes. Runs before markup parsing (matching
    /// upstream's default `emoji=True`); `:name:` and `[tag]` don't overlap.
    fn expand_emoji(&self, content: &str) -> String {
        if self.emoji {
            crate::emoji::replace(content)
        } else {
            content.to_string()
        }
    }

    /// Apply the registered highlighters, plus the built-in `ReprHighlighter`
    /// when `highlight` is on.
    fn decorate(&self, mut text: Text) -> Text {
        for highlighter in &self.highlighters {
            highlighter.highlight(&mut text);
        }
        if self.highlight {
            crate::highlighter::ReprHighlighter::new().highlight(&mut text);
        }
        text
    }

    /// Parse `content` as markup and print it justified to the console width.
    /// This is the `console.print("...", justify=...)` path.
    pub fn print_justified(&self, content: &str, justify: Justify) {
        let text = self.build_text(content);
        let mut options = self.options();
        options.justify = justify;
        let segments = text.rich_render(self, &options);
        self.emit(segments);
    }

    /// Same as [`Console::print_justified`] but returns the ANSI string.
    ///
    /// The justify is passed via `options.justify`, which — matching upstream —
    /// disables the measurement-fit so the text pads to the full width.
    pub fn render_justified_to_string(&self, content: &str, justify: Justify) -> String {
        let text = self.build_text(content);
        let mut options = self.options();
        options.justify = justify;
        let segments = text.rich_render(self, &options);
        self.segments_to_string(&segments)
    }

    /// Convert rendered segments into a terminal string, applying this console's
    /// colour system (and honouring `no_color`).
    pub fn segments_to_string(&self, segments: &[Segment]) -> String {
        let system = self.color_system();
        let mut out = String::new();
        for segment in segments {
            // Control codes are meaningless off a terminal — upstream's
            // `_render_buffer` drops them when `not is_terminal`.
            if segment.control && !self.is_terminal {
                continue;
            }
            match (&segment.style, system) {
                (Some(style), Some(sys)) => out.push_str(&style.render(&segment.text, Some(sys))),
                _ => out.push_str(&segment.text),
            }
        }
        out
    }
}

/// Join the visible text of a segment stream, dropping control codes. Port of
/// `Console.export_text(styles=False)`'s join.
fn segments_to_plain(segments: &[Segment]) -> String {
    segments
        .iter()
        .filter(|s| !s.control)
        .map(|s| s.text.as_str())
        .collect()
}

impl Renderable for Text {
    fn rich_render(&self, console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
        // Wrap to the available width; the effective justify is this text's own
        // justify, falling back to the console options' justify.
        let justify = if self.get_justify() != Justify::Default {
            self.get_justify()
        } else {
            options.justify
        };
        // Same precedence for overflow and no_wrap: the text's own setting wins,
        // then the options', then upstream's default. Mirrors the `self.x or
        // options.x or DEFAULT` chain in `Text.__rich_console__`.
        let overflow = self
            .get_overflow()
            .or(options.overflow)
            .unwrap_or(Overflow::Fold);
        let no_wrap = self.get_no_wrap().or(options.no_wrap).unwrap_or(false);
        self.render_joined_wrapped(
            console.theme(),
            console.base_style(),
            options.max_width,
            justify,
            overflow,
            no_wrap,
        )
    }

    fn measure(&self, _console: &Console, options: &ConsoleOptions) -> crate::measure::Measurement {
        let (minimum, maximum) = self.measurement();
        crate::measure::Measurement::new(
            minimum.min(options.max_width),
            maximum.min(options.max_width),
        )
    }
}

/// Builder for [`Console`], allowing detection to be overridden.
pub struct ConsoleBuilder {
    force_terminal: Option<bool>,
    color_system: Option<ColorSystem>,
    color_system_set: bool,
    width: Option<usize>,
    height: Option<usize>,
    no_color: Option<bool>,
    emoji: Option<bool>,
    highlight: Option<bool>,
    legacy_windows: Option<bool>,
    safe_box: Option<bool>,
    ascii_only: Option<bool>,
    theme: Option<Theme>,
}

impl ConsoleBuilder {
    fn new() -> Self {
        ConsoleBuilder {
            force_terminal: None,
            color_system: None,
            color_system_set: false,
            width: None,
            height: None,
            no_color: None,
            emoji: None,
            highlight: None,
            legacy_windows: None,
            safe_box: None,
            ascii_only: None,
            theme: None,
        }
    }

    pub fn force_terminal(mut self, value: bool) -> Self {
        self.force_terminal = Some(value);
        self
    }

    /// Force legacy-Windows-console behavior (box substitution). Default off.
    pub fn legacy_windows(mut self, value: bool) -> Self {
        self.legacy_windows = Some(value);
        self
    }

    /// Enable/disable terminal-safe box substitution (default on).
    pub fn safe_box(mut self, value: bool) -> Self {
        self.safe_box = Some(value);
        self
    }

    /// Force ASCII-only box rendering (default off). Set for non-UTF-8 terminals.
    pub fn ascii_only(mut self, value: bool) -> Self {
        self.ascii_only = Some(value);
        self
    }

    /// Force a specific color system (use for reproducible output/tests).
    pub fn color_system(mut self, system: Option<ColorSystem>) -> Self {
        self.color_system = system;
        self.color_system_set = true;
        self
    }

    pub fn width(mut self, width: usize) -> Self {
        self.width = Some(width);
        self
    }

    /// Set the console height in rows (used by [`Layout`](crate::layout::Layout)).
    pub fn height(mut self, height: usize) -> Self {
        self.height = Some(height);
        self
    }

    pub fn no_color(mut self, value: bool) -> Self {
        self.no_color = Some(value);
        self
    }

    /// Enable/disable `:emoji:` shortcode replacement (default enabled).
    pub fn emoji(mut self, value: bool) -> Self {
        self.emoji = Some(value);
        self
    }

    /// Enable/disable automatic repr highlighting. Defaults to **on**, matching
    /// upstream `Console(highlight=True)`.
    pub fn highlight(mut self, value: bool) -> Self {
        self.highlight = Some(value);
        self
    }

    pub fn theme(mut self, theme: Theme) -> Self {
        self.theme = Some(theme);
        self
    }

    pub fn build(self) -> Console {
        let is_terminal = self
            .force_terminal
            .unwrap_or_else(|| std::io::stdout().is_terminal());
        // Upstream's rule is `environ.get("NO_COLOR", "") != ""`, so an EMPTY
        // NO_COLOR does not disable colour — only a non-empty value does. That
        // matters because a shell that exports `NO_COLOR=` (a common way to
        // clear it) would otherwise still be treated as opting out.
        let no_color = self
            .no_color
            .unwrap_or_else(|| std::env::var_os("NO_COLOR").is_some_and(|value| !value.is_empty()));
        let color_system = if self.color_system_set {
            self.color_system
        } else if is_terminal {
            Some(detect_color_system())
        } else {
            None
        };
        let width = self.width.unwrap_or_else(detect_width);
        let height = self.height.unwrap_or_else(detect_height);
        Console {
            color_system,
            width,
            height,
            is_terminal,
            no_color,
            emoji: self.emoji.unwrap_or(true),
            // Upstream's `Console(highlight=True)` default. Getting this wrong is
            // invisible in the fixtures (every one is captured with
            // highlight=False) but is the first thing a user sees: numbers,
            // paths, booleans and URLs come out plain instead of coloured.
            highlight: self.highlight.unwrap_or(true),
            legacy_windows: self.legacy_windows.unwrap_or(false),
            safe_box: self.safe_box.unwrap_or(true),
            ascii_only: self.ascii_only.unwrap_or(false),
            theme: self.theme.unwrap_or_else(Theme::default_theme),
            base_style: Style::new(),
            highlighters: Vec::new(),
            record_buffer: std::cell::RefCell::new(Vec::new()),
            capturing: std::cell::Cell::new(false),
        }
    }
}

/// Detect the terminal color system.
///
/// `COLORTERM`/`TERM` are the portable signals, but **Windows sets neither**.
/// Detecting from them alone meant every Windows console fell back to
/// [`ColorSystem::Standard`] — 16 colors — for all output. Measured on a real
/// Windows Terminal session: 28 distinct colors in a rendered heat map against
/// 140 once truecolor was detected.
///
/// Upstream `rich` special-cases Windows for the same reason. It reaches the
/// platform APIs directly; we ask `anstyle-query`, which avoids hand-written
/// `unsafe` FFI for a console handle (see `docs/DIVERGENCES.md`).
fn detect_color_system() -> ColorSystem {
    if let Some(colorterm) = std::env::var_os("COLORTERM") {
        let colorterm = colorterm.to_string_lossy().to_ascii_lowercase();
        if colorterm.contains("truecolor") || colorterm.contains("24bit") {
            return ColorSystem::Truecolor;
        }
    }

    // A Windows console that accepts ENABLE_VIRTUAL_TERMINAL_PROCESSING is a
    // modern one, and every Windows console that speaks VT also speaks 24-bit
    // color. The call also *enables* VT processing, which legacy `conhost`
    // needs before it will honour any escape sequence at all — so this both
    // detects the capability and turns it on.
    #[cfg(windows)]
    if anstyle_query::windows::enable_ansi_colors() == Some(true) {
        return ColorSystem::Truecolor;
    }

    if let Some(term) = std::env::var_os("TERM") {
        if term.to_string_lossy().contains("256") {
            return ColorSystem::EightBit;
        }
    }
    ColorSystem::Standard
}

/// Detect the terminal width: `COLUMNS`, then the real terminal, then a default.
fn detect_width() -> usize {
    if let Some(columns) = std::env::var_os("COLUMNS") {
        if let Ok(value) = columns.to_string_lossy().trim().parse::<usize>() {
            if value > 0 {
                return value;
            }
        }
    }
    if let Some((terminal_size::Width(w), _)) = terminal_size::terminal_size() {
        if w > 0 {
            return w as usize;
        }
    }
    DEFAULT_WIDTH
}

/// Detect the terminal height: `LINES`, then the real terminal, then a default.
fn detect_height() -> usize {
    if let Some(lines) = std::env::var_os("LINES") {
        if let Ok(value) = lines.to_string_lossy().trim().parse::<usize>() {
            if value > 0 {
                return value;
            }
        }
    }
    if let Some((_, terminal_size::Height(h))) = terminal_size::terminal_size() {
        if h > 0 {
            return h as usize;
        }
    }
    DEFAULT_HEIGHT
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_console() -> Console {
        Console::builder()
            .force_terminal(true)
            .color_system(Some(ColorSystem::Truecolor))
            .width(80)
            .no_color(false)
            .build()
    }

    /// The strict path reports malformed markup where the lenient one prints it
    /// literally. Both must still agree on markup that is actually valid.
    #[test]
    fn try_build_text_reports_bad_markup() {
        let console = test_console();

        let err = console
            .try_build_text("[/nope]")
            .expect_err("an unmatched closing tag must be an error");
        assert!(
            matches!(err, crate::errors::RichError::Markup(_)),
            "{err:?}"
        );
        // The lenient path swallows it and prints the source text as-is.
        assert_eq!(console.build_text("[/nope]").plain(), "[/nope]");

        let strict = console.try_build_text("[bold]hi[/]").expect("valid markup");
        assert_eq!(strict.plain(), "hi");
        assert_eq!(
            strict.spans().len(),
            console.build_text("[bold]hi[/]").spans().len()
        );
    }

    /// An unknown tag *name* is not an error — it renders as a no-op, tag
    /// consumed. Only genuine syntax errors fail.
    ///
    /// Verified against real rich 15.0.0: `Console().print("[nope]x[/]")` writes
    /// `x`, while `[bold]a[/italic]` raises `MarkupError`. Before names were
    /// carried on spans, the port resolved `nope` eagerly, failed, and fell back
    /// to printing the markup source literally.
    #[test]
    fn unknown_tag_names_render_as_no_ops() {
        let console = test_console();
        let text = console
            .try_build_text("[nope]x[/]")
            .expect("an unknown tag name is not a syntax error");
        assert_eq!(console.render_to_string(&text), "x");
        assert_eq!(
            console.render_to_string(&console.build_text("[a.b.c]x[/]")),
            "x"
        );

        // A mismatched closing tag is still an error, on both paths.
        assert!(console.try_build_text("[bold]a[/italic]").is_err());
        assert!(console.try_build_text("[/nope]").is_err());
    }

    /// Markup styles bind to the theme of the console that renders the text, not
    /// the one that parsed it. Verified against real rich 15.0.0.
    #[test]
    fn markup_styles_bind_at_render_not_at_parse() {
        let themed = |definition: &str| {
            let mut theme = Theme::default_theme();
            theme.insert("accent", Style::parse(definition).unwrap());
            Console::builder()
                .force_terminal(true)
                .color_system(Some(ColorSystem::Truecolor))
                .width(80)
                .no_color(false)
                .theme(theme)
                .build()
        };
        let red = themed("bold red");
        let green = themed("underline green");

        // Built once, by the red console...
        let text = red.build_text("[accent]hi[/]");
        assert_eq!(red.render_to_string(&text), "\x1b[1;31mhi\x1b[0m");
        // ...and the green console still renders it in green.
        assert_eq!(green.render_to_string(&text), "\x1b[4;32mhi\x1b[0m");
    }

    /// Emoji expansion and the highlighters have to run on both paths, or the
    /// strict variant would quietly render differently from the lenient one.
    #[test]
    fn try_build_text_expands_emoji_like_build_text() {
        let console = test_console();
        assert_eq!(
            console
                .try_build_text(":rocket: go")
                .expect("valid")
                .plain(),
            console.build_text(":rocket: go").plain()
        );
    }

    #[test]
    fn renders_markup_string() {
        let console = test_console();
        assert_eq!(
            console.render_str_to_string("[bold red]hi[/]"),
            "\x1b[1;31mhi\x1b[0m"
        );
    }

    #[test]
    fn print_justify_pads_to_width() {
        let console = Console::builder()
            .force_terminal(true)
            .color_system(Some(ColorSystem::Truecolor))
            .width(10)
            .build();
        // Captured from real rich 15.0.0: console.print("hi", justify=...).
        assert_eq!(
            console.render_justified_to_string("hi", Justify::Left),
            "hi        "
        );
        assert_eq!(
            console.render_justified_to_string("hi", Justify::Center),
            "    hi    "
        );
        assert_eq!(
            console.render_justified_to_string("hi", Justify::Right),
            "        hi"
        );
    }

    #[test]
    fn capture_records_ansi_instead_of_stdout() {
        let console = Console::builder()
            .force_terminal(true)
            .color_system(Some(ColorSystem::Truecolor))
            .width(20)
            .build();
        // Captured from real rich 15.0.0 (Console.capture()).
        let out = console.capture(|c| c.print_str("[bold red]hi[/] there"));
        assert_eq!(out, "\x1b[1;31mhi\x1b[0m there\n");
    }

    #[test]
    fn themed_exports_use_the_given_palette() {
        use crate::terminal_theme::{MONOKAI, NIGHT_OWLISH};

        let console = Console::builder()
            .force_terminal(true)
            .color_system(Some(ColorSystem::Truecolor))
            .width(20)
            .no_color(false)
            .build();
        let render = |c: &Console| c.print_str("hi");

        // Monokai's background is #0c0c0c and Night Owlish's is #ffffff, so the
        // chosen theme has to show up in the emitted CSS.
        let monokai = console.export_html_themed(&MONOKAI, render);
        assert!(
            monokai.contains("#0c0c0c"),
            "monokai bg missing:\n{monokai}"
        );

        let owlish = console.export_html_themed(&NIGHT_OWLISH, render);
        assert!(owlish.contains("#ffffff"), "owlish bg missing:\n{owlish}");
        assert!(!owlish.contains("#0c0c0c"), "leaked monokai into owlish");

        // The class form and SVG take a theme too.
        let classes = console.export_html_classes_themed(&MONOKAI, render);
        assert!(classes.contains("#0c0c0c"), "class-form ignored the theme");
        let svg = console.export_svg_themed(&MONOKAI, "t", "id", render);
        assert!(svg.contains("#0c0c0c"), "svg ignored the theme");

        // The convenience methods keep their documented defaults.
        assert!(console.export_html(render).contains("#ffffff"));
    }

    #[test]
    fn page_with_honors_the_styles_flag() {
        use std::sync::Mutex;

        #[derive(Default)]
        struct Recorder(Mutex<String>);
        impl crate::pager::Pager for Recorder {
            fn show(&self, content: &str) -> std::io::Result<()> {
                *self.0.lock().unwrap() = content.to_string();
                Ok(())
            }
        }

        let console = Console::builder()
            .force_terminal(true)
            .color_system(Some(ColorSystem::Truecolor))
            .width(20)
            .no_color(false)
            .build();

        // styles = false (upstream's `Console.pager()` default) strips ANSI.
        let plain = Recorder::default();
        console
            .page_with(&plain, false, |c| c.print_str("[bold red]hi[/] there"))
            .unwrap();
        assert_eq!(plain.0.lock().unwrap().as_str(), "hi there\n");

        // styles = true keeps it, matching `Console.pager(styles=True)`.
        let styled = Recorder::default();
        console
            .page_with(&styled, true, |c| c.print_str("[bold red]hi[/] there"))
            .unwrap();
        assert_eq!(
            styled.0.lock().unwrap().as_str(),
            "\x1b[1;31mhi\x1b[0m there\n"
        );
    }

    #[test]
    fn export_text_strips_styles() {
        let console = Console::builder()
            .force_terminal(true)
            .color_system(Some(ColorSystem::Truecolor))
            .width(20)
            .build();
        // Captured from real rich 15.0.0 (Console.export_text(styles=False)).
        let out = console.export_text(|c| c.print_str("[bold red]hi[/] there"));
        assert_eq!(out, "hi there\n");
    }

    #[test]
    fn export_html_matches_upstream() {
        let console = Console::builder()
            .force_terminal(true)
            .color_system(Some(ColorSystem::Truecolor))
            .width(20)
            .no_color(false)
            .build();
        let html = console.export_html(|c| {
            c.print_str("[bold red]hi[/] there");
            c.print_str("plain line");
        });
        // Regenerated from real rich by `scripts/capture_golden.py`, so CI's
        // drift check covers exports too. Keep this input in step with the
        // matching console in that script.
        let expected = include_str!("../tests/golden/export_html.html").replace("\r\n", "\n");
        assert_eq!(html, expected);
    }

    #[test]
    fn export_html_classes_matches_upstream() {
        let console = Console::builder()
            .force_terminal(true)
            .color_system(Some(ColorSystem::Truecolor))
            .width(20)
            .no_color(false)
            .build();
        let html = console.export_html_classes(|c| c.print_str("[bold red]hi[/] there"));
        // As above: regenerated by `scripts/capture_golden.py`. Note this test
        // prints ONE line where the inline-styles test prints two.
        let expected =
            include_str!("../tests/golden/export_html_classes.html").replace("\r\n", "\n");
        assert_eq!(html, expected);
    }

    #[test]
    fn capture_matches_direct_render() {
        let console = test_console();
        let panel = crate::panel::Panel::new(Box::new(Text::new("hi")));
        assert_eq!(
            console.capture(|c| c.print(&panel)),
            console.render_export(&panel)
        );
    }

    #[test]
    fn no_color_strips_styles() {
        let console = Console::builder()
            .force_terminal(true)
            .color_system(None)
            .build();
        assert_eq!(console.render_str_to_string("[bold red]hi[/]"), "hi");
    }
}
