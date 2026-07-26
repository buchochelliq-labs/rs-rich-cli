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
}

/// The high-level interface for rendering to a terminal. Mirrors
/// `rich.console.Console`.
pub struct Console {
    color_system: Option<ColorSystem>,
    width: usize,
    is_terminal: bool,
    no_color: bool,
    emoji: bool,
    highlight: bool,
    theme: Theme,
    base_style: Style,
    highlighters: Vec<Box<dyn Highlighter>>,
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
        let mut options = self.options();
        if options.justify == Justify::Default {
            let measurement = renderable.measure(self, &options);
            options.max_width = measurement.maximum.min(options.max_width).max(1);
        }
        let segments = renderable.rich_render(self, &options);
        self.segments_to_string(&segments)
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
        let output = self.render_to_string(renderable);
        let stdout = std::io::stdout();
        let mut lock = stdout.lock();
        let _ = writeln!(lock, "{output}");
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
        let output = self.render_justified_to_string(content, justify);
        let stdout = std::io::stdout();
        let mut lock = stdout.lock();
        let _ = writeln!(lock, "{output}");
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
        Console {
            color_system,
            width,
            is_terminal,
            no_color,
            emoji: self.emoji.unwrap_or(true),
            highlight: self.highlight.unwrap_or(false),
            theme: self.theme.unwrap_or_else(Theme::default_theme),
            base_style: Style::new(),
            highlighters: Vec::new(),
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
    fn no_color_strips_styles() {
        let console = Console::builder()
            .force_terminal(true)
            .color_system(None)
            .build();
        assert_eq!(console.render_str_to_string("[bold red]hi[/]"), "hi");
    }
}
