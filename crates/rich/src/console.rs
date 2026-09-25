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

/// A source of the current time in seconds. Upstream's `GetTimeCallable`,
/// shared by [`Console::get_time`] and [`Progress`](crate::progress::Progress).
pub type GetTime = std::sync::Arc<dyn Fn() -> f64 + Send + Sync>;

/// Seconds on a monotonic clock: upstream's default `time.monotonic`. Every
/// default clock in the crate reads this one origin, so times taken from a
/// console and from a progress display are comparable.
pub(crate) fn monotonic() -> f64 {
    static START: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
    START
        .get_or_init(std::time::Instant::now)
        .elapsed()
        .as_secs_f64()
}

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
    /// Highlight override for strings rendered under these options, or `None`
    /// for the console default. Mirrors `ConsoleOptions.highlight`: `Panel`,
    /// `Table` and `Tree` set it for their children, and upstream's
    /// `Console.render` passes it to `render_str` for a `str` renderable.
    pub highlight: Option<bool>,
    /// Markup override for strings rendered under these options, or `None` for
    /// the console default. Mirrors `ConsoleOptions.markup`.
    pub markup: Option<bool>,
    /// Height of the container (starts as the terminal height). Mirrors
    /// `ConsoleOptions.max_height`.
    pub max_height: usize,
    /// Encoding of the terminal (`"utf-8"`, or `"ascii"` for an ASCII-only
    /// console). Mirrors `ConsoleOptions.encoding`.
    pub encoding: String,
    /// Whether the target is a terminal. Mirrors `ConsoleOptions.is_terminal`.
    pub is_terminal: bool,
    /// Whether the target is a legacy Windows console. Mirrors
    /// `ConsoleOptions.legacy_windows`.
    pub legacy_windows: bool,
    /// The size of the console. Mirrors `ConsoleOptions.size`.
    pub size: ConsoleDimensions,
}

/// The size of a console in cells. Mirrors `rich.console.ConsoleDimensions`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ConsoleDimensions {
    /// Width in cells.
    pub width: usize,
    /// Height in rows.
    pub height: usize,
}

impl Default for ConsoleOptions {
    /// The options of a default 80x25 UTF-8 console, as
    /// [`Console::options`] builds them.
    fn default() -> Self {
        ConsoleOptions {
            min_width: 1,
            max_width: DEFAULT_WIDTH,
            height: None,
            justify: Justify::Default,
            overflow: None,
            no_wrap: None,
            highlight: None,
            markup: None,
            max_height: DEFAULT_HEIGHT,
            encoding: "utf-8".to_string(),
            is_terminal: false,
            legacy_windows: false,
            size: ConsoleDimensions {
                width: DEFAULT_WIDTH,
                height: DEFAULT_HEIGHT,
            },
        }
    }
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
        options.max_height = height;
        options
    }

    /// Return a copy with the height (and `max_height`) set. Port of
    /// `ConsoleOptions.update_height`.
    pub fn update_height(&self, height: usize) -> ConsoleOptions {
        let mut options = self.clone();
        options.height = Some(height);
        options.max_height = height;
        options
    }

    /// Return a copy with `height` cleared. Port of
    /// `ConsoleOptions.reset_height`.
    pub fn reset_height(&self) -> ConsoleOptions {
        let mut options = self.clone();
        options.height = None;
        options
    }

    /// Whether renderables should use ASCII only: the encoding is not a UTF
    /// one. Port of the `ConsoleOptions.ascii_only` property.
    pub fn ascii_only(&self) -> bool {
        !self.encoding.starts_with("utf")
    }
}

/// Keyword arguments of upstream's `Console.render_str`, for
/// [`Console::render_str_with`]. `None` takes the console's default.
#[derive(Clone, Default)]
pub struct RenderStrOptions<'a> {
    /// Base style of the result (`style=`, default none).
    pub style: crate::style::StyleType,
    /// `justify=`; `None` leaves the text's justify unset.
    pub justify: Option<Justify>,
    /// `overflow=`; `None` leaves the text's overflow unset.
    pub overflow: Option<Overflow>,
    /// `emoji=`: replace emoji codes, or `None` for the console default.
    pub emoji: Option<bool>,
    /// `markup=`: parse console markup, or `None` for the console default.
    pub markup: Option<bool>,
    /// `highlight=`: highlight, or `None` for the console default.
    pub highlight: Option<bool>,
    /// `highlighter=`: highlight with this instead of the console's
    /// highlighters (registered ones plus `ReprHighlighter`).
    pub highlighter: Option<&'a dyn Highlighter>,
}

/// The high-level interface for rendering to a terminal. Mirrors
/// `rich.console.Console`.
pub struct Console {
    render_environment: Option<std::sync::Arc<dyn crate::protocol::RenderEnvironment>>,
    /// The default code highlighter (see `ConsoleCodeHighlighting`).
    code_highlighting: Option<crate::protocol::CodeHighlighting>,
    color_system: Option<ColorSystem>,
    width: usize,
    height: usize,
    is_terminal: bool,
    no_color: bool,
    /// Upstream's `Console.get_time`: the clock animations read.
    get_time: GetTime,
    emoji: bool,
    highlight: bool,
    legacy_windows: bool,
    safe_box: bool,
    ascii_only: bool,
    /// Upstream's `ThemeStack`: the builder's theme at the bottom, pushed
    /// themes above it. Never empty; styles resolve against the top entry.
    theme_stack: Vec<Theme>,
    base_style: Style,
    /// Registered highlighters. Shared (`Arc`) so a [`Clone`] of the console
    /// keeps them; each is behind a lock so the console is `Sync`.
    highlighters: Vec<std::sync::Arc<dyn Highlighter + Send + Sync>>,
    /// Upstream's `Console(markup=…)`: whether printed strings are parsed as
    /// console markup.
    markup: bool,
    /// Upstream's `Console(emoji_variant=…)`: the variant appended to emoji
    /// codes that name none.
    emoji_variant: Option<crate::emoji::EmojiVariant>,
    /// Upstream's `Console(tab_size=…)`: the tab stop width `Text` expands to.
    tab_size: usize,
    /// While capturing, print paths append their segments here instead of
    /// writing to stdout. Mirrors `Console._record_buffer` under `capture()`.
    /// A mutex (upstream guards it with `_record_buffer_lock`) so the console
    /// is `Sync`.
    record_buffer: std::sync::Mutex<Vec<Segment>>,
    capturing: std::sync::atomic::AtomicBool,
}

/// A `Send`-only highlighter behind a lock, so it can be shared by a `Sync`
/// console.
struct LockedHighlighter(std::sync::Mutex<Box<dyn Highlighter + Send>>);

impl Highlighter for LockedHighlighter {
    fn highlight(&self, text: &mut Text) {
        let highlighter = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        highlighter.highlight(text);
    }
}

impl Clone for Console {
    /// An independent console with the same configuration, theme stack and
    /// highlighters (shared). The clone starts with an empty capture buffer
    /// and is not capturing, whatever the original is doing.
    fn clone(&self) -> Self {
        Console {
            render_environment: self.render_environment.clone(),
            code_highlighting: self.code_highlighting.clone(),
            color_system: self.color_system,
            width: self.width,
            height: self.height,
            is_terminal: self.is_terminal,
            no_color: self.no_color,
            get_time: self.get_time.clone(),
            emoji: self.emoji,
            highlight: self.highlight,
            legacy_windows: self.legacy_windows,
            safe_box: self.safe_box,
            ascii_only: self.ascii_only,
            theme_stack: self.theme_stack.clone(),
            base_style: self.base_style.clone(),
            highlighters: self.highlighters.clone(),
            markup: self.markup,
            emoji_variant: self.emoji_variant,
            tab_size: self.tab_size,
            record_buffer: std::sync::Mutex::new(Vec::new()),
            capturing: std::sync::atomic::AtomicBool::new(false),
        }
    }
}

/// A theme in use on a [`Console`] until this guard drops. Returned by
/// [`Console::use_theme`]; upstream's `ThemeContext`.
pub struct ThemeContext<'a> {
    console: &'a mut Console,
}

impl std::ops::Deref for ThemeContext<'_> {
    type Target = Console;

    fn deref(&self) -> &Console {
        self.console
    }
}

impl std::ops::DerefMut for ThemeContext<'_> {
    fn deref_mut(&mut self) -> &mut Console {
        self.console
    }
}

impl Drop for ThemeContext<'_> {
    fn drop(&mut self) {
        // Upstream's `__exit__` pops unconditionally. This only fails if the
        // caller already popped back down to the base through the guard.
        let _ = self.console.pop_theme();
    }
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

    /// The active color system, or `None` when styles are not rendered at
    /// all. Port of `Console.color_system`.
    ///
    /// Like upstream, this is independent of [`no_color`](Self::no_color):
    /// no-colour mode keeps the colour system and strips only the colours at
    /// output time, so bold, italic, underline and the like still render.
    /// Callers asking "will colour reach the terminal?" must check both.
    pub fn color_system(&self) -> Option<ColorSystem> {
        self.color_system
    }

    /// Whether colour output is disabled. Port of `Console.no_color`: set by
    /// the builder, or by a non-empty `NO_COLOR` environment variable.
    pub fn no_color(&self) -> bool {
        self.no_color
    }

    /// The current time in seconds from this console's clock. Port of
    /// `Console.get_time` (default `time.monotonic`); override it with
    /// [`ConsoleBuilder::get_time`] for deterministic animation.
    pub fn get_time(&self) -> f64 {
        (self.get_time)()
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

    /// The active theme: the top of the theme stack.
    pub fn theme(&self) -> &Theme {
        self.theme_stack
            .last()
            .expect("the theme stack always holds its base theme")
    }

    /// Resolve a style name (or pass a style through) against this console's
    /// theme. Port of `Console.get_style`.
    pub fn get_style(&self, style: &crate::style::StyleType) -> crate::errors::Result<Style> {
        self.theme().get_style(style)
    }

    /// Push a theme on to the top of the stack. Port of `Console.push_theme`.
    ///
    /// With `inherit` the new top is the current top's styles overridden by
    /// `theme`'s; without it, the new top is exactly `theme`. Prefer
    /// [`use_theme`](Self::use_theme), which pops again automatically.
    pub fn push_theme(&mut self, theme: Theme, inherit: bool) {
        let top = if inherit {
            let mut merged = self.theme().clone();
            merged.extend_from(&theme);
            merged
        } else {
            theme
        };
        self.theme_stack.push(top);
    }

    /// Remove the top theme, restoring the previous one. Port of
    /// `Console.pop_theme`; popping the base theme is an error
    /// (upstream's `ThemeStackError("Unable to pop base theme")`).
    pub fn pop_theme(&mut self) -> crate::errors::Result<()> {
        if self.theme_stack.len() == 1 {
            return Err(crate::errors::RichError::ThemeStack(
                "Unable to pop base theme".to_string(),
            ));
        }
        self.theme_stack.pop();
        Ok(())
    }

    /// Use a theme until the returned guard is dropped. Port of
    /// `Console.use_theme`, Python's context manager as an RAII guard.
    ///
    /// The guard dereferences to the console, so print *through the guard*
    /// while it is alive; dropping it pops the theme, including during a
    /// panic unwind.
    ///
    /// ```
    /// # use rich::{Console, Theme, Style};
    /// let mut console = Console::builder().width(20).build();
    /// let mut theme = Theme::new();
    /// theme.insert("warning", Style::parse("bold red").unwrap());
    /// {
    ///     let themed = console.use_theme(theme);
    ///     assert!(themed.theme().get("warning").is_some());
    /// }
    /// assert!(console.theme().get("warning").is_none());
    /// ```
    ///
    /// Upstream's `use_theme` also takes `inherit`, but its `ThemeContext`
    /// never passes it on to `push_theme`, so a used theme always inherits
    /// (verified against rich 15.0.0). This port keeps that behaviour and
    /// omits the ignored parameter; call [`push_theme`](Self::push_theme) to
    /// replace the styles outright.
    pub fn use_theme(&mut self, theme: Theme) -> ThemeContext<'_> {
        self.push_theme(theme, true);
        ThemeContext { console: self }
    }

    /// The whole-output base style.
    pub fn base_style(&self) -> &Style {
        &self.base_style
    }

    /// Register a highlighter. **The core plugin seam** — see docs/PLUGINS.md.
    /// The highlighter must be `Send` so a [`Console`](Console) can move to a
    /// background thread (e.g. an auto-refreshing [`Live`](crate::live::Live)).
    pub fn add_highlighter(&mut self, highlighter: Box<dyn Highlighter + Send>) {
        self.highlighters.push(std::sync::Arc::new(LockedHighlighter(
            std::sync::Mutex::new(highlighter),
        )));
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
            highlight: None,
            markup: None,
            max_height: self.height,
            encoding: self.encoding().to_string(),
            is_terminal: self.is_terminal,
            legacy_windows: self.legacy_windows,
            size: self.size(),
        }
    }

    /// The size of the console. Port of the `Console.size` property.
    pub fn size(&self) -> ConsoleDimensions {
        ConsoleDimensions {
            width: self.width,
            height: self.height,
        }
    }

    /// The output encoding: `"ascii"` for an [ASCII-only](Self::ascii_only)
    /// console, else `"utf-8"`. Port of the `Console.encoding` property (which
    /// reads the output file's encoding; this port has no file to ask).
    pub fn encoding(&self) -> &'static str {
        if self.ascii_only {
            "ascii"
        } else {
            "utf-8"
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

    /// Render a renderable to segments as `Console.print` does (shared by the
    /// string and print paths): a printed `Text` goes through upstream's
    /// `Text.join`, and an extension that opts into measurement-fit is shrunk
    /// to its measured width when no explicit justify is set.
    fn render_segments(&self, renderable: &dyn Renderable) -> Vec<Segment> {
        self.render_segments_with(renderable, &self.options())
    }

    /// Render a renderable to segments with explicit render options, exactly
    /// as [`print_with`](Self::print_with) does before writing: a printed
    /// `Text` goes through upstream's `Text.join`, a renderable that opts in
    /// is fitted to its measurement, and the result is cropped to the console
    /// width (`print(crop=True)`). No trailing newline is added.
    ///
    /// For upstream's lower-level `Console.render`, see [`render`](Self::render).
    pub fn render_segments_with(
        &self,
        renderable: &dyn Renderable,
        options: &ConsoleOptions,
    ) -> Vec<Segment> {
        let mut options = options.clone();
        let joined;
        let renderable = match renderable.printed_text() {
            Some(text) => {
                joined = text;
                &joined as &dyn Renderable
            }
            None => renderable,
        };
        if options.justify == Justify::Default && renderable.fit_to_measurement() {
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

    /// Render a renderable to segments. Port of `Console.render`: `options`
    /// defaults to [`options`](Self::options), and nothing is rendered when
    /// there is no width (`max_width < 1`).
    ///
    /// Unlike the print path ([`render_segments_with`](Self::render_segments_with))
    /// the renderable is rendered as-is: no `Text.join`, no fitting and no
    /// crop. Container renderables use this for their children.
    pub fn render(
        &self,
        renderable: &dyn Renderable,
        options: Option<&ConsoleOptions>,
    ) -> Vec<Segment> {
        let default_options;
        let options = match options {
            Some(options) => options,
            None => {
                default_options = self.options();
                &default_options
            }
        };
        if options.max_width < 1 {
            return Vec::new();
        }
        renderable.rich_render(self, options)
    }

    /// Write (or, while capturing, record) a rendered segment stream, adding a
    /// trailing newline. The single sink for every `print*` path.
    fn emit(&self, segments: Vec<Segment>) {
        self.emit_end(segments, true);
    }

    /// [`emit`](Self::emit), with the trailing newline optional: upstream's
    /// `print(…, end="")` when `newline` is false. Output written straight to
    /// stdout is flushed in that case, so a prompt shows before input is read.
    fn emit_end(&self, segments: Vec<Segment>, newline: bool) {
        if segments.is_empty() {
            return;
        }
        if self.capturing.load(std::sync::atomic::Ordering::SeqCst) {
            let mut buffer = self.lock_record_buffer();
            buffer.extend(segments);
            if newline {
                buffer.push(Segment::line());
            }
            return;
        }
        let mut output = self.segments_to_string(&segments);
        if newline {
            output.push('\n');
        }
        let stdout = std::io::stdout();
        let mut lock = stdout.lock();
        let _ = write!(lock, "{output}");
        if !newline {
            let _ = lock.flush();
        }
    }

    /// Display `prompt` and read a line of input from standard input. Port of
    /// `Console.input`: the prompt is console markup, printed through the
    /// console with `end=""` so it is captured and exported like any other
    /// output. `None` means end of input.
    pub fn input(&self, prompt: &str) -> std::io::Result<Option<String>> {
        let prompt = self.build_text(prompt);
        self.input_from(&prompt, &mut crate::prompt::StdinInput)
    }

    /// [`input`](Self::input) with a renderable prompt (upstream accepts a
    /// `Text`) and an explicit input source — upstream's `stream=` argument.
    pub fn input_from(
        &self,
        prompt: &dyn Renderable,
        stream: &mut dyn crate::prompt::InputSource,
    ) -> std::io::Result<Option<String>> {
        // `if prompt: self.print(prompt, end="")`.
        let segments = self.render_segments(prompt);
        if segments.iter().any(|segment| !segment.text.is_empty()) {
            self.emit_end(segments, false);
        }
        stream.read_line()
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
        self.render_lines_styled(renderable, options, None, pad)
    }

    /// [`render_lines`](Self::render_lines) with upstream's `style=` argument:
    /// the style is applied under every rendered segment and to the padding
    /// that fills each line, as `Panel` and `Padding` use it.
    pub fn render_lines_styled(
        &self,
        renderable: &dyn Renderable,
        options: &ConsoleOptions,
        style: Option<&Style>,
        pad: bool,
    ) -> Vec<Vec<Segment>> {
        let style = style.filter(|style| !style.is_null());
        // Upstream `Console.render` yields nothing when `max_width < 1`, so a
        // renderable squeezed to zero width contributes no lines (#449).
        let mut segments = if options.max_width < 1 {
            Vec::new()
        } else {
            renderable.rich_render(self, options)
        };
        if let Some(style) = style {
            segments = Segment::apply_style(&segments, style);
        }
        let mut lines = Segment::split_lines(&segments);
        // An empty `Text` renders as a lone empty segment: upstream renders it
        // as its `end` newline, which `split_and_crop_lines` turns into one
        // blank line (#442).
        if lines.is_empty()
            && !segments.is_empty()
            && segments
                .iter()
                .all(|segment| !segment.control && segment.text.is_empty())
        {
            lines.push(Vec::new());
        }
        let pad_style = Some(style.cloned().unwrap_or_default());
        if pad {
            for line in &mut lines {
                *line = Segment::adjust_line_length(line, options.max_width, pad_style.clone());
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
                        pad_style.clone(),
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
        let segments = self.render_segments(renderable);
        let mut out = self.segments_to_string(&segments);
        if !segments.is_empty() {
            out.push('\n');
        }
        out
    }

    /// Render a value and write it to stdout, followed by a newline.
    pub fn print(&self, renderable: &dyn Renderable) {
        let segments = self.render_segments(renderable);
        self.emit(segments);
    }

    /// Print with explicit render options, the equivalent of upstream's
    /// `Console.print(renderable, justify=…, overflow=…, no_wrap=…)`. Start
    /// from [`options`](Self::options) and set the fields to override. A
    /// printed `Text` defers to these options, because upstream's `Text.join`
    /// drops the text's own `justify`, `overflow` and `no_wrap`.
    pub fn print_with(&self, renderable: &dyn Renderable, options: &ConsoleOptions) {
        let segments = self.render_segments_with(renderable, options);
        self.emit(segments);
    }

    /// Like [`render_export`](Self::render_export), with explicit render
    /// options as for [`print_with`](Self::print_with).
    pub fn render_export_with(
        &self,
        renderable: &dyn Renderable,
        options: &ConsoleOptions,
    ) -> String {
        let segments = self.render_segments_with(renderable, options);
        let mut out = self.segments_to_string(&segments);
        if !segments.is_empty() {
            out.push('\n');
        }
        out
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
        use std::sync::atomic::Ordering;
        let previous = std::mem::take(&mut *self.lock_record_buffer());
        let was_capturing = self.capturing.swap(true, Ordering::SeqCst);
        f(self);
        let captured = std::mem::replace(&mut *self.lock_record_buffer(), previous);
        self.capturing.store(was_capturing, Ordering::SeqCst);
        captured
    }

    /// The capture buffer, recovering it if a panicking print poisoned it.
    fn lock_record_buffer(&self) -> std::sync::MutexGuard<'_, Vec<Segment>> {
        self.record_buffer
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
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
        if !self.markup {
            // `Console(markup=False)`: the string is taken literally.
            return Ok(self.decorate(Text::new(self.expand_emoji_plain(content))));
        }
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

    /// Convert a plain string to [`Text`] the way a `str` renderable is
    /// converted upstream: emoji codes expand (per the console), console
    /// markup is parsed, and highlighting runs when `highlight` (or, when
    /// `None`, the console default) enables it. Port of `Console.render_str`,
    /// which `Table` cells, `Tree` labels and `Columns` items go through.
    ///
    /// Malformed markup falls back to the literal text, as
    /// [`build_text`](Console::build_text) does (docs/DIVERGENCES.md §2).
    pub fn render_str(&self, content: &str, highlight: Option<bool>) -> Text {
        let highlight = highlight.unwrap_or(self.highlight);
        // `markup.render` returns the (emoji-replaced) string untouched when it
        // holds no `[`; skip the parser for the common plain cell.
        let markup = if !self.markup {
            Text::new(self.expand_emoji_plain(content))
        } else if content.contains('[') {
            let expanded = self.expand_emoji(content);
            Text::from_markup(&expanded).unwrap_or_else(|_| Text::new(expanded))
        } else if content.contains(':') {
            Text::new(self.expand_emoji(content))
        } else {
            Text::new(content)
        };
        if !highlight {
            return markup;
        }
        // Highlight the plain text, then append the markup spans, as
        // `highlight_text.copy_styles(rich_text)` does (see `try_build_text`).
        let mut text = self.decorate_with_repr(Text::new(markup.plain()));
        for span in markup.spans() {
            text.push_span(span.clone());
        }
        text
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
    ///
    /// The console's default emoji variant applies only when `content` holds
    /// no `[`: upstream's `markup.render` passes `default_variant` on its
    /// tag-free fast path only, and replaces the text between tags without it.
    pub(crate) fn expand_emoji(&self, content: &str) -> String {
        if !self.emoji {
            return content.to_string();
        }
        let variant = if content.contains('[') {
            None
        } else {
            self.emoji_variant
        };
        crate::emoji::replace_with_variant(content, variant)
    }

    /// Expand `:emoji:` shortcodes in a string that is not markup, with the
    /// console's default variant (upstream's `markup=False` branch of
    /// `render_str`).
    fn expand_emoji_plain(&self, content: &str) -> String {
        if self.emoji {
            crate::emoji::replace_with_variant(content, self.emoji_variant)
        } else {
            content.to_string()
        }
    }

    /// Convert a string to [`Text`] with every keyword upstream's
    /// `Console.render_str` takes. Port of `Console.render_str`, strict:
    /// malformed markup is an error (`MarkupError`), not the literal text.
    ///
    /// Emoji, markup and highlighting default to the console's settings when
    /// the option is `None`. As upstream, a highlighted result is a fresh
    /// `Text` carrying only spans (the highlighter's, then the markup's): the
    /// `style`, `justify` and `overflow` are dropped by `copy_styles`.
    pub fn render_str_with(
        &self,
        content: &str,
        options: &RenderStrOptions<'_>,
    ) -> crate::errors::Result<Text> {
        let emoji = options.emoji.unwrap_or(self.emoji);
        let markup = options.markup.unwrap_or(self.markup);
        let highlight = options.highlight.unwrap_or(self.highlight);

        let mut rich_text = if markup {
            let expanded = if !emoji {
                content.to_string()
            } else if content.contains('[') {
                crate::emoji::replace(content)
            } else {
                crate::emoji::replace_with_variant(content, self.emoji_variant)
            };
            Text::from_markup(&expanded)?
        } else if emoji {
            Text::new(crate::emoji::replace_with_variant(
                content,
                self.emoji_variant,
            ))
        } else {
            Text::new(content)
        };
        rich_text.set_base_style(options.style.clone());
        rich_text.set_justify(options.justify.unwrap_or_default());
        rich_text.set_overflow(options.overflow);

        if !highlight {
            return Ok(rich_text);
        }
        let mut text = Text::new(rich_text.plain());
        match options.highlighter {
            Some(highlighter) => highlighter.highlight(&mut text),
            None => text = self.decorate_with_repr(text),
        }
        for span in rich_text.spans() {
            text.push_span(span.clone());
        }
        Ok(text)
    }

    /// Whether `:emoji:` codes are replaced by default. Port of
    /// `Console(emoji=…)`.
    pub fn emoji(&self) -> bool {
        self.emoji
    }

    /// Whether printed strings are highlighted by default. Port of
    /// `Console(highlight=…)`.
    pub fn highlight(&self) -> bool {
        self.highlight
    }

    /// Whether printed strings are parsed as console markup by default. Port
    /// of `Console(markup=…)`.
    pub fn markup(&self) -> bool {
        self.markup
    }

    /// The default emoji variant. Port of `Console(emoji_variant=…)`.
    pub fn emoji_variant(&self) -> Option<crate::emoji::EmojiVariant> {
        self.emoji_variant
    }

    /// The tab stop width `Text` expands tabs to. Port of `Console.tab_size`.
    pub fn tab_size(&self) -> usize {
        self.tab_size
    }

    /// Change the tab stop width. Upstream's `Console.tab_size` is a plain
    /// attribute.
    pub fn set_tab_size(&mut self, tab_size: usize) {
        self.tab_size = tab_size;
    }

    /// Set the terminal window title. Port of `Console.set_window_title`:
    /// only a terminal is sent the code, and the return value says whether it
    /// was.
    pub fn set_window_title(&self, title: &str) -> bool {
        if self.is_terminal {
            self.control(&crate::control::Control::title(title));
            true
        } else {
            false
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

    /// [`decorate`](Self::decorate) for a caller that has already decided to
    /// highlight (upstream's `highlight=True` override of the console default).
    fn decorate_with_repr(&self, mut text: Text) -> Text {
        for highlighter in &self.highlighters {
            highlighter.highlight(&mut text);
        }
        crate::highlighter::ReprHighlighter::new().highlight(&mut text);
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
    /// colour system (and honouring `no_color`). Port of `Console._render_buffer`.
    pub fn segments_to_string(&self, segments: &[Segment]) -> String {
        let system = self.color_system;
        // `if self.no_color and color_system: buffer = Segment.remove_color(…)`:
        // colours go, every other attribute stays.
        let colorless;
        let segments = if self.no_color && system.is_some() {
            colorless = Segment::remove_color(segments);
            &colorless[..]
        } else {
            segments
        };
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
    fn printed_text(&self) -> Option<Text> {
        // `Text("").join([self])`: the text and its spans survive; justify,
        // overflow and no_wrap come from the blank separator (#446). The base
        // style does not: `join` takes the separator's (none) and re-applies
        // this text's as a leading span (`if text.style: append_span(...)`),
        // so print-level justify padding is left unstyled.
        let mut text = self.clone();
        text.clear_layout_options();
        text.base_style_to_span();
        Some(text)
    }

    fn rich_render(&self, console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
        // Empty Text still represents a printable blank line; an empty
        // generator such as Markdown does not. Preserve that distinction.
        if self.is_empty() {
            return vec![Segment::new("", None)];
        }
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
        // `tab_size = console.tab_size if self.tab_size is None else …`.
        self.render_joined_wrapped_tabs(
            console.theme(),
            console.base_style(),
            options.max_width,
            justify,
            overflow,
            no_wrap,
            self.console_tab_size(console),
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
    get_time: Option<GetTime>,
    emoji: Option<bool>,
    highlight: Option<bool>,
    legacy_windows: Option<bool>,
    safe_box: Option<bool>,
    ascii_only: Option<bool>,
    theme: Option<Theme>,
    markup: Option<bool>,
    emoji_variant: Option<crate::emoji::EmojiVariant>,
    tab_size: Option<usize>,
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
            get_time: None,
            emoji: None,
            highlight: None,
            legacy_windows: None,
            safe_box: None,
            ascii_only: None,
            theme: None,
            markup: None,
            emoji_variant: None,
            tab_size: None,
        }
    }

    /// Enable/disable console markup in printed strings (default enabled).
    /// Port of `Console(markup=…)`.
    pub fn markup(mut self, value: bool) -> Self {
        self.markup = Some(value);
        self
    }

    /// The emoji variant appended to codes that name none (default none).
    /// Port of `Console(emoji_variant=…)`.
    pub fn emoji_variant(mut self, variant: Option<crate::emoji::EmojiVariant>) -> Self {
        self.emoji_variant = variant;
        self
    }

    /// The tab stop width `Text` expands tabs to (default 8). Port of
    /// `Console(tab_size=…)`.
    pub fn tab_size(mut self, tab_size: usize) -> Self {
        self.tab_size = Some(tab_size);
        self
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

    /// Enable no-colour mode: colours are stripped from output while other
    /// attributes (bold, underline, …) still render. Unset, a non-empty
    /// `NO_COLOR` environment variable enables it. Port of `no_color=`.
    pub fn no_color(mut self, value: bool) -> Self {
        self.no_color = Some(value);
        self
    }

    /// Read the current time (seconds) from `clock` instead of the monotonic
    /// clock. Port of `Console(get_time=…)`; animations such as
    /// [`Spinner`](crate::spinner::Spinner) render the frame for this time.
    pub fn get_time(mut self, clock: impl Fn() -> f64 + Send + Sync + 'static) -> Self {
        self.get_time = Some(std::sync::Arc::new(clock));
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
        } else if is_terminal && !is_dumb_terminal() {
            Some(detect_color_system())
        } else {
            None
        };
        let width = self.width.unwrap_or_else(detect_width);
        let height = self.height.unwrap_or_else(detect_height);
        Console {
            render_environment: None,
            code_highlighting: None,
            color_system,
            width,
            height,
            is_terminal,
            no_color,
            get_time: self
                .get_time
                .unwrap_or_else(|| std::sync::Arc::new(monotonic)),
            emoji: self.emoji.unwrap_or(true),
            // Upstream's `Console(highlight=True)` default. Getting this wrong is
            // invisible in the fixtures (every one is captured with
            // highlight=False) but is the first thing a user sees: numbers,
            // paths, booleans and URLs come out plain instead of coloured.
            highlight: self.highlight.unwrap_or(true),
            legacy_windows: self.legacy_windows.unwrap_or(false),
            safe_box: self.safe_box.unwrap_or(true),
            ascii_only: self.ascii_only.unwrap_or(false),
            theme_stack: vec![self.theme.unwrap_or_else(Theme::default_theme)],
            base_style: Style::new(),
            highlighters: Vec::new(),
            markup: self.markup.unwrap_or(true),
            emoji_variant: self.emoji_variant,
            // `tab_size: int = 8`.
            tab_size: self.tab_size.unwrap_or(crate::text::DEFAULT_TAB_SIZE),
            record_buffer: std::sync::Mutex::new(Vec::new()),
            capturing: std::sync::atomic::AtomicBool::new(false),
        }
    }
}

/// Whether `TERM` names a terminal that cannot render styles. Port of
/// `Console.is_dumb_terminal` (the caller supplies the `is_terminal` half):
/// `_detect_color_system` returns no colour system for one, so its output is
/// plain even though it is a terminal.
fn is_dumb_terminal() -> bool {
    std::env::var("TERM")
        .map(|term| matches!(term.to_lowercase().as_str(), "dumb" | "unknown"))
        .unwrap_or(false)
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

    // Windows. This function is only reached when stdout is a terminal (see
    // ConsoleBuilder::build), and every modern Windows console that can be a
    // terminal speaks 24-bit color, so report truecolor.
    //
    // The call below is for its SIDE EFFECT — it turns on
    // ENABLE_VIRTUAL_TERMINAL_PROCESSING, which legacy `conhost` needs before
    // it honours any escape sequence. Its RETURN VALUE is deliberately ignored:
    // it enables VT on stdout *and stderr* and propagates failure with `?`, so
    // merely redirecting stderr (`rich ... 2>log`, the most natural CI
    // invocation) made it report failure and dropped the whole console to 16
    // colors — even though stdout was still a fully capable terminal.
    #[cfg(windows)]
    {
        let _ = anstyle_query::windows::enable_ansi_colors();
        ColorSystem::Truecolor
    }

    // `TERM` is meaningless on Windows and the branch above always returns, so
    // gating this keeps either platform free of unreachable code.
    #[cfg(not(windows))]
    {
        if let Some(term) = std::env::var_os("TERM") {
            if term.to_string_lossy().contains("256") {
                return ColorSystem::EightBit;
            }
        }
        ColorSystem::Standard
    }
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

impl crate::protocol::ConsoleCodeHighlighting for Console {
    fn set_code_highlighting(&mut self, value: Option<crate::protocol::CodeHighlighting>) {
        self.code_highlighting = value;
    }
    fn code_highlighting(&self) -> Option<&crate::protocol::CodeHighlighting> {
        self.code_highlighting.as_ref()
    }
}

impl crate::protocol::ConsoleEnvironment for Console {
    fn set_render_environment(
        &mut self,
        value: Option<std::sync::Arc<dyn crate::protocol::RenderEnvironment>>,
    ) {
        self.render_environment = value;
    }
    fn render_environment(&self) -> Option<&dyn crate::protocol::RenderEnvironment> {
        self.render_environment.as_deref()
    }
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
    fn empty_text_and_empty_renderables_have_distinct_endings() {
        let console = Console::builder().force_terminal(false).build();
        assert_eq!(console.render_export(&Text::new("")), "\n");
        assert_eq!(
            console.render_export(&crate::markdown::Markdown::new("")),
            ""
        );
        assert_eq!(console.render_export(&crate::table::Table::new()), "\n");
    }

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

    #[test]
    fn used_theme_applies_through_the_guard_and_pops_on_drop() {
        let mut console = Console::builder()
            .force_terminal(true)
            .color_system(Some(ColorSystem::Truecolor))
            .width(20)
            .highlight(false)
            .build();
        let theme = Theme::from_styles([("accent", "bold red")], false).unwrap();
        {
            let themed = console.use_theme(theme);
            let out = themed.capture(|c| c.print_str("[accent]x[/]"));
            assert_eq!(out, "\x1b[1;31mx\x1b[0m\n");
        }
        assert!(console.theme().get("accent").is_none());
        assert!(console.pop_theme().is_err(), "the base theme must remain");
    }

    #[test]
    fn used_theme_is_popped_during_a_panic_unwind() {
        let mut console = Console::builder().width(20).build();
        let theme = Theme::from_styles([("accent", "bold")], false).unwrap();
        let unwound = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _themed = console.use_theme(theme);
            panic!("render failed");
        }));
        assert!(unwound.is_err());
        assert!(console.theme().get("accent").is_none());
    }

    #[test]
    fn a_printed_text_defers_layout_to_the_print_options() {
        // Captured from real rich 15.0.0 (#446, #447): `Text.join` drops the
        // text's own overflow and justify; print-level options still apply.
        let console = Console::builder()
            .force_terminal(true)
            .color_system(Some(crate::color::ColorSystem::Truecolor))
            .width(6)
            .highlight(false)
            .build();
        let text = Text::new("abcdefghij").overflow(Overflow::Ellipsis);
        assert_eq!(console.render_export(&text), "abcdef\nghij\n");
        let mut options = console.options();
        options.overflow = Some(Overflow::Ellipsis);
        assert_eq!(
            console.render_export_with(&Text::new("abcdefghij"), &options),
            "abcde…\n"
        );
        let wide = Console::builder()
            .force_terminal(true)
            .color_system(Some(crate::color::ColorSystem::Truecolor))
            .width(20)
            .highlight(false)
            .build();
        let tabbed = Text::new("a\tb").justify(Justify::Right);
        assert_eq!(wide.render_export(&tabbed), "a       b\n");
    }
}
