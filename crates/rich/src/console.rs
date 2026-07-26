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
}

impl ConsoleOptions {
    /// Return a copy with `max_width` (and a clamped `min_width`) updated.
    /// Port of `ConsoleOptions.update_width`.
    pub fn update_width(&self, width: usize) -> ConsoleOptions {
        ConsoleOptions {
            min_width: width,
            max_width: width,
            height: self.height,
            justify: self.justify,
        }
    }

    /// Return a copy with both width and height pinned. Port of
    /// `ConsoleOptions.update_dimensions`.
    pub fn update_dimensions(&self, width: usize, height: usize) -> ConsoleOptions {
        ConsoleOptions {
            min_width: width,
            max_width: width,
            height: Some(height),
            justify: self.justify,
        }
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
    theme: Theme,
    base_style: Style,
    highlighters: Vec<Box<dyn Highlighter>>,
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

    /// The active theme.
    pub fn theme(&self) -> &Theme {
        &self.theme
    }

    /// The whole-output base style.
    pub fn base_style(&self) -> &Style {
        &self.base_style
    }

    /// Register a highlighter. **The core plugin seam** — see docs/PLUGINS.md.
    pub fn add_highlighter(&mut self, highlighter: Box<dyn Highlighter>) {
        self.highlighters.push(highlighter);
    }

    /// The default render options for this console (full width, no height).
    pub fn options(&self) -> ConsoleOptions {
        ConsoleOptions {
            min_width: 1,
            max_width: self.width,
            height: None,
            justify: Justify::Default,
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
        renderable.rich_render(self, &options)
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

    fn build_text(&self, content: &str) -> Text {
        // Emoji shortcodes are expanded before markup parsing (matching upstream's
        // default `emoji=True`); `:name:` and `[tag]` don't overlap.
        let expanded = if self.emoji {
            crate::emoji::replace(content)
        } else {
            content.to_string()
        };
        let mut text =
            Text::from_markup(&expanded, &self.theme).unwrap_or_else(|_| Text::new(&expanded));
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

    /// Convert rendered segments into a string, applying the color system.
    fn segments_to_string(&self, segments: &[Segment]) -> String {
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
        self.render_joined_justified(console.base_style(), options.max_width, justify)
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
            theme: None,
        }
    }

    pub fn force_terminal(mut self, value: bool) -> Self {
        self.force_terminal = Some(value);
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

    /// Enable/disable automatic repr highlighting (default disabled).
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
        let no_color = self
            .no_color
            .unwrap_or_else(|| std::env::var_os("NO_COLOR").is_some());
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
            highlight: self.highlight.unwrap_or(false),
            theme: self.theme.unwrap_or_else(Theme::default_theme),
            base_style: Style::new(),
            highlighters: Vec::new(),
            record_buffer: std::cell::RefCell::new(Vec::new()),
            capturing: std::cell::Cell::new(false),
        }
    }
}

/// Detect the terminal color system from environment variables.
fn detect_color_system() -> ColorSystem {
    if let Some(colorterm) = std::env::var_os("COLORTERM") {
        let colorterm = colorterm.to_string_lossy().to_ascii_lowercase();
        if colorterm.contains("truecolor") || colorterm.contains("24bit") {
            return ColorSystem::Truecolor;
        }
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
