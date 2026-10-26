//! The Console — the high-level rendering entry point.
//!
//! Port of upstream `rich/console.py`: terminal, colour-system and size
//! detection, markup and highlighting, printing and logging, capture,
//! recording and export, render hooks, the alternate screen and paging.
//! What reads Python objects (file objects, Jupyter, `print_exception`,
//! `log_locals`) is not ported (docs/DIVERGENCES.md §43).

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

/// An environment to detect the terminal from, in place of the process's:
/// upstream's `Console(_environ=…)`. See [`ConsoleBuilder::environ`].
pub type Environ = std::collections::HashMap<String, String>;

/// A replacement for [`Console::on_broken_pipe`]'s default.
type BrokenPipeHandler = std::sync::Arc<dyn Fn(&Console) + Send + Sync>;

/// Upstream's `get_datetime`: the clock [`Console::log`] reads.
type GetDateTime = std::sync::Arc<dyn Fn() -> std::time::SystemTime + Send + Sync>;

/// Upstream's `FormatTimeCallable`: formats the time column of
/// [`Console::log`].
type FormatTime = std::sync::Arc<dyn Fn(std::time::SystemTime) -> Text + Send + Sync>;

/// Upstream's `log_time_format`: a `strftime` format or a callable.
#[derive(Clone)]
enum LogTimeFormat {
    Strftime(String),
    Callable(FormatTime),
}

/// Read `key` from `environ`, or from the process environment without one.
fn environ_get(environ: Option<&Environ>, key: &str) -> Option<String> {
    match environ {
        Some(environ) => environ.get(key).cloned(),
        None => std::env::var_os(key).map(|value| value.to_string_lossy().into_owned()),
    }
}

/// Python's `str.isdigit()` then `int(…)`, for `COLUMNS` and `LINES`: only
/// a non-empty run of digits counts.
fn env_int(environ: Option<&Environ>, key: &str) -> Option<usize> {
    let value = environ_get(environ, key)?;
    if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    value.parse().ok()
}

/// Hooks in to the render process. Port of `rich.console.RenderHook`: a
/// hook pushed with [`Console::push_render_hook`] is handed what each
/// `print` and `log` is about to write, and returns what to write instead
/// (upstream's live display adds a cursor move before the output and
/// itself after it).
///
/// Upstream hands the hook the renderables themselves, rendered afterwards
/// at the print's options. Here each one is already rendered at those
/// options when the hook sees it, so that it can be kept beyond the call:
/// putting one in another renderable (a panel, say) shows the same lines,
/// where upstream's would render again at the new width. What the hook adds
/// is rendered at the print's options, its last line ended unless it says
/// otherwise ([`Renderable::print_end_line`]).
pub trait RenderHook: Send + Sync {
    /// Return the renderables to write in place of `renderables`. Port of
    /// `RenderHook.process_renderables`.
    fn process_renderables(
        &self,
        renderables: Vec<Box<dyn Renderable>>,
    ) -> Vec<Box<dyn Renderable>>;
}

/// One printed renderable, already rendered at the print's options (its last
/// line ended): what a [`RenderHook`] is handed.
struct Rendered(Vec<Segment>);

impl Renderable for Rendered {
    fn rich_render(&self, _console: &Console, _options: &ConsoleOptions) -> Vec<Segment> {
        self.0.clone()
    }

    fn measure(
        &self,
        _console: &Console,
        _options: &ConsoleOptions,
    ) -> crate::measure::Measurement {
        let widest = Segment::split_lines(&self.0)
            .iter()
            .map(|line| Segment::get_line_length(line))
            .max()
            .unwrap_or(0);
        crate::measure::Measurement::new(widest, widest)
    }

    /// The line break is already among the segments.
    fn print_end_line(&self, _options: &ConsoleOptions) -> bool {
        false
    }
}

/// Per-thread capture buffer stacks (innermost capture last).
type CaptureStacks = std::collections::HashMap<std::thread::ThreadId, Vec<Vec<Segment>>>;

/// The calling thread's innermost capture buffer, if it is capturing.
fn current_capture(captures: &mut CaptureStacks) -> Option<&mut Vec<Segment>> {
    captures
        .get_mut(&std::thread::current().id())
        .and_then(|stack| stack.last_mut())
}

/// The high-level interface for rendering to a terminal. Mirrors
/// `rich.console.Console`.
pub struct Console {
    render_environment: Option<std::sync::Arc<dyn crate::protocol::RenderEnvironment>>,
    /// The semantic-region observer (see `ConsoleRegions`); none by default.
    region_sink: Option<std::sync::Arc<dyn crate::protocol::RegionSink>>,
    /// The default code highlighter (see `ConsoleCodeHighlighting`).
    code_highlighting: Option<crate::protocol::CodeHighlighting>,
    color_system: Option<ColorSystem>,
    /// Upstream's `Console._width`: the width given, or read from `COLUMNS`
    /// when the console was built. `None` falls back to `terminal_size`.
    width: Option<usize>,
    /// Upstream's `Console._height`, as `width` (`LINES`).
    height: Option<usize>,
    /// The terminal's size when the console was built (80x25 when it has
    /// none), the width already less one on a legacy Windows console. Upstream
    /// asks `os.get_terminal_size` on each `size` read; this port asks once.
    terminal_size: ConsoleDimensions,
    /// Upstream's `Console._environ`: the environment detection reads, or
    /// `None` for the process environment.
    environ: Option<std::sync::Arc<Environ>>,
    is_terminal: bool,
    no_color: bool,
    /// Upstream's `Console.quiet`: suppress all output. Atomic because
    /// [`on_broken_pipe`](Console::on_broken_pipe) sets it through `&self`.
    quiet: std::sync::atomic::AtomicBool,
    /// Upstream's `Console.soft_wrap`: the default of `print(soft_wrap=…)`.
    soft_wrap: bool,
    /// Upstream's `Console.style`: a style applied to all output.
    style: Option<crate::style::StyleType>,
    /// What a write to a closed pipe calls: upstream's overridable
    /// `Console.on_broken_pipe`. `None` runs upstream's default.
    on_broken_pipe: Option<BrokenPipeHandler>,
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
    /// While a thread is capturing, its print paths append their segments to
    /// the innermost buffer of its stack here instead of writing to stdout.
    /// Mirrors upstream's `ConsoleThreadLocals.buffer`: capture state is per
    /// thread, so a thread printing while another captures is not swallowed
    /// by that capture, and concurrent captures never mix. Each nested
    /// capture on a thread pushes a buffer and pops it when it ends.
    captures: std::sync::Mutex<CaptureStacks>,
    /// Upstream's `Console._is_alt_screen`: set by
    /// [`set_alt_screen`](Console::set_alt_screen).
    is_alt_screen: std::sync::atomic::AtomicBool,
    /// Upstream's `Console.record`: keep what is written for the
    /// `export_*_recorded` methods.
    record: std::sync::atomic::AtomicBool,
    /// Upstream's `Console._record_buffer`: every segment written while
    /// recording (not what a capture takes, nor what a quiet console drops).
    record_buffer: std::sync::Mutex<Vec<Segment>>,
    /// Upstream's `Console._log_render`, built from `log_time` and
    /// `log_path`; it remembers the last time shown.
    log_render: std::sync::Mutex<crate::log_render::LogRender>,
    log_time: bool,
    log_path: bool,
    log_time_format: LogTimeFormat,
    /// Upstream's `Console.get_datetime` (default `datetime.now`).
    get_datetime: GetDateTime,
    /// Upstream's `Console._render_hooks`.
    render_hooks: std::sync::Mutex<Vec<std::sync::Arc<dyn RenderHook>>>,
    /// Upstream's `Console._live_stack`.
    live_stack: std::sync::Mutex<Vec<std::sync::Arc<dyn std::any::Any + Send + Sync>>>,
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

/// `NoAltScreen("Alt screen must be enabled to call update_screen")`.
fn no_alt_screen() -> crate::errors::RichError {
    crate::errors::RichError::NoAltScreen(
        "Alt screen must be enabled to call update_screen".to_string(),
    )
}

/// Rendered lines placed at an offset on the screen: each line is preceded
/// by a cursor move to its row. Port of `rich.console.ScreenUpdate`.
pub struct ScreenUpdate {
    lines: Vec<Vec<Segment>>,
    x: usize,
    y: usize,
}

impl ScreenUpdate {
    /// `lines` drawn from column `x`, row `y`.
    pub fn new(lines: Vec<Vec<Segment>>, x: usize, y: usize) -> Self {
        ScreenUpdate { lines, x, y }
    }
}

impl Renderable for ScreenUpdate {
    fn rich_render(&self, _console: &Console, _options: &ConsoleOptions) -> Vec<Segment> {
        let mut segments = Vec::new();
        for (offset, line) in self.lines.iter().enumerate() {
            // `Control.move_to(x, y + offset)`, widened so huge coordinates
            // are written in full (upstream's ints are unbounded).
            let to = crate::control::move_to_code(self.x as u128, self.y as u128 + offset as u128);
            segments.push(Segment::control(to));
            segments.extend(line.iter().cloned());
        }
        segments
    }
}

impl Clone for Console {
    /// An independent console with the same configuration, theme stack and
    /// highlighters (shared). The clone starts with an empty capture buffer
    /// and is not capturing, whatever the original is doing.
    fn clone(&self) -> Self {
        Console {
            render_environment: self.render_environment.clone(),
            region_sink: self.region_sink.clone(),
            code_highlighting: self.code_highlighting.clone(),
            color_system: self.color_system,
            width: self.width,
            height: self.height,
            terminal_size: self.terminal_size,
            environ: self.environ.clone(),
            is_terminal: self.is_terminal,
            no_color: self.no_color,
            quiet: std::sync::atomic::AtomicBool::new(self.quiet()),
            soft_wrap: self.soft_wrap,
            style: self.style.clone(),
            on_broken_pipe: self.on_broken_pipe.clone(),
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
            captures: std::sync::Mutex::new(CaptureStacks::new()),
            is_alt_screen: std::sync::atomic::AtomicBool::new(false),
            record: std::sync::atomic::AtomicBool::new(self.record()),
            record_buffer: std::sync::Mutex::new(Vec::new()),
            render_hooks: std::sync::Mutex::new(Vec::new()),
            live_stack: std::sync::Mutex::new(Vec::new()),
            log_render: std::sync::Mutex::new(new_log_render(self.log_time, self.log_path)),
            log_time: self.log_time,
            log_path: self.log_path,
            log_time_format: self.log_time_format.clone(),
            get_datetime: self.get_datetime.clone(),
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

/// The alternate screen in use on a [`Console`] until this guard drops.
/// Returned by [`Console::screen`]; upstream's `ScreenContext`.
pub struct ScreenContext<'a> {
    console: &'a Console,
    hide_cursor: bool,
    changed: bool,
    /// Always `Some`; taken only while it is rebuilt.
    screen: Option<crate::screen::Screen>,
    style: crate::style::StyleType,
}

impl ScreenContext<'_> {
    /// Print the screen, after replacing what it shows when `renderables`
    /// are given (one after another, upstream's `Group`) and its style when
    /// `style` is. Port of `ScreenContext.update(*renderables, style=…)`.
    pub fn update(
        &mut self,
        renderables: Vec<Box<dyn Renderable>>,
        style: Option<crate::style::StyleType>,
    ) {
        if let Some(style) = style {
            self.style = style;
        }
        let screen = if renderables.is_empty() {
            self.screen
                .take()
                .unwrap_or_else(crate::screen::Screen::empty)
        } else {
            crate::screen::Screen::from_renderables(renderables)
        };
        let screen = screen.style(self.style.clone());
        // `console.print(self.screen, end="")`.
        self.console.print(&screen);
        self.screen = Some(screen);
    }

    /// The console the screen is on.
    pub fn console(&self) -> &Console {
        self.console
    }
}

impl Drop for ScreenContext<'_> {
    fn drop(&mut self) {
        // Upstream's `__exit__`.
        if self.changed {
            self.console.set_alt_screen(false);
            if self.hide_cursor {
                self.console.show_cursor(true);
            }
        }
    }
}

/// Lock `mutex`, recovering it if a panicking holder poisoned it.
fn lock_or_recover<T>(mutex: &std::sync::Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
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

    /// The width in cells. Port of the `Console.width` property: the
    /// width of [`size`](Self::size).
    pub fn width(&self) -> usize {
        self.size().width
    }

    /// The height in rows. Port of the `Console.height` property: the
    /// height of [`size`](Self::size). Used by height-aware renderables such
    /// as [`Layout`](crate::layout::Layout).
    pub fn height(&self) -> usize {
        self.size().height
    }

    /// Set the width. Port of the `Console.width` setter.
    pub fn set_width(&mut self, width: usize) {
        self.width = Some(width);
    }

    /// Set the height. Port of the `Console.height` setter.
    pub fn set_height(&mut self, height: usize) {
        self.height = Some(height);
    }

    /// Set the width and height. Port of the `Console.size` setter.
    pub fn set_size(&mut self, width: usize, height: usize) {
        self.width = Some(width);
        self.height = Some(height);
    }

    /// Whether all output is suppressed. Port of `Console.quiet`.
    pub fn quiet(&self) -> bool {
        self.quiet.load(std::sync::atomic::Ordering::SeqCst)
    }

    /// Suppress (or restore) all output. Upstream's `Console.quiet` is a
    /// plain attribute; [`on_broken_pipe`](Self::on_broken_pipe) sets it.
    pub fn set_quiet(&self, quiet: bool) {
        self.quiet.store(quiet, std::sync::atomic::Ordering::SeqCst);
    }

    /// The default of `print(soft_wrap=…)`. Port of `Console.soft_wrap`.
    pub fn soft_wrap(&self) -> bool {
        self.soft_wrap
    }

    /// Change the soft wrap default. Upstream's `Console.soft_wrap` is a
    /// plain attribute.
    pub fn set_soft_wrap(&mut self, soft_wrap: bool) {
        self.soft_wrap = soft_wrap;
    }

    /// The style applied to all output, if any. Port of `Console.style`.
    pub fn style(&self) -> Option<&crate::style::StyleType> {
        self.style.as_ref()
    }

    /// Change the style applied to all output. Upstream's `Console.style`
    /// is a plain attribute.
    pub fn set_style(&mut self, style: Option<crate::style::StyleType>) {
        self.style = style;
    }

    /// Whether the console writes to a terminal that cannot render styles:
    /// `TERM` is `dumb` or `unknown`. Port of `Console.is_dumb_terminal`.
    pub fn is_dumb_terminal(&self) -> bool {
        self.is_terminal && term_is_dumb(self.environ.as_deref())
    }

    /// Called when a write fails because the reader closed the pipe. Port of
    /// `Console.on_broken_pipe`: by default the console goes
    /// [quiet](Self::quiet) and the process exits with status 1 (upstream
    /// raises `SystemExit(1)`). Upstream lets a subclass override it; here
    /// [`ConsoleBuilder::on_broken_pipe`] gives the replacement.
    ///
    /// Upstream also points standard output at `os.devnull` so that the
    /// interpreter's flush at exit does not fail again. A Rust process does
    /// not flush at exit, so nothing is redirected.
    pub fn on_broken_pipe(&self) {
        if let Some(handler) = &self.on_broken_pipe {
            handler(self);
            return;
        }
        self.set_quiet(true);
        std::process::exit(1);
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
        self.highlighters
            .push(std::sync::Arc::new(LockedHighlighter(
                std::sync::Mutex::new(highlighter),
            )));
    }

    /// The default render options for this console (full width, no height).
    pub fn options(&self) -> ConsoleOptions {
        let size = self.size();
        ConsoleOptions {
            min_width: 1,
            max_width: size.width,
            height: None,
            justify: Justify::Default,
            overflow: None,
            no_wrap: None,
            highlight: None,
            markup: None,
            max_height: size.height,
            encoding: self.encoding().to_string(),
            is_terminal: self.is_terminal,
            legacy_windows: self.legacy_windows,
            size,
        }
    }

    /// The size of the console. Port of the `Console.size` property: the
    /// width and height given (the width one less on a legacy Windows
    /// console, as upstream subtracts it when both are set), else 80x25 on
    /// a [dumb terminal](Self::is_dumb_terminal), else the terminal's size
    /// for whichever was not given.
    pub fn size(&self) -> ConsoleDimensions {
        if let (Some(width), Some(height)) = (self.width, self.height) {
            return ConsoleDimensions {
                width: width.saturating_sub(usize::from(self.legacy_windows)),
                height,
            };
        }
        if self.is_dumb_terminal() {
            return ConsoleDimensions {
                width: DEFAULT_WIDTH,
                height: DEFAULT_HEIGHT,
            };
        }
        ConsoleDimensions {
            width: self.width.unwrap_or(self.terminal_size.width),
            height: self.height.unwrap_or(self.terminal_size.height),
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
        self.print_segments(renderable, options, false)
    }

    /// The segments `print` writes for `renderable`: those of
    /// [`render_segments_with`](Self::render_segments_with), followed, when
    /// `end_line` and the renderable ends its last line, by the line break.
    ///
    /// The console's [`soft_wrap`](Self::soft_wrap) applies as upstream's
    /// `print(soft_wrap=None)` takes it (no wrapping, `overflow="ignore"`,
    /// nothing cropped), and its [`style`](Self::style) goes under all of
    /// the output (`_collect_renderables` wraps each renderable in `Styled`).
    fn print_segments(
        &self,
        renderable: &dyn Renderable,
        options: &ConsoleOptions,
        end_line: bool,
    ) -> Vec<Segment> {
        let mut options = options.clone();
        let crop = !self.soft_wrap;
        if self.soft_wrap {
            options.no_wrap = options.no_wrap.or(Some(true));
            options.overflow = options.overflow.or(Some(Overflow::Ignore));
        }
        let ends_line = end_line && Self::print_ends_line(renderable, &options);
        let joined;
        let joined_text = renderable.printed_text();
        let renderable = match joined_text.clone() {
            Some(text) => {
                // `print(justify="left"|"center"|"right")` wraps the joined
                // text in `Align`, which renders a zero-width text (such as an
                // empty one) at no width at all: nothing is printed.
                if matches!(
                    options.justify,
                    Justify::Left | Justify::Center | Justify::Right
                ) && text.measurement().1 == 0
                {
                    return Vec::new();
                }
                joined = text;
                &joined as &dyn Renderable
            }
            None => renderable,
        };
        // `print(justify="left"|"center"|"right")` wraps every other
        // renderable in `Align(renderable, justify)` (`_collect_renderables`).
        let align = match options.justify {
            Justify::Left if joined_text.is_none() => Some(crate::align::HorizontalAlign::Left),
            Justify::Center if joined_text.is_none() => Some(crate::align::HorizontalAlign::Center),
            Justify::Right if joined_text.is_none() => Some(crate::align::HorizontalAlign::Right),
            _ => None,
        };
        if options.justify == Justify::Default && renderable.fit_to_measurement() {
            let measurement = renderable.measure(self, &options);
            options.max_width = measurement.maximum.min(options.max_width).max(1);
        }
        let mut segments = match align {
            // `Console.render` yields nothing when there is no width.
            _ if options.max_width < 1 => Vec::new(),
            Some(align) => crate::align::Align::render_child(renderable, align, self, &options),
            None => renderable.rich_render(self, &options),
        };
        if !segments.is_empty() {
            if ends_line {
                segments.push(Segment::line());
            }
            if let Some(style) = self.console_style() {
                segments = Segment::apply_style_with(&segments, Some(&style), None);
            }
        }
        // A print (not a render to a string) goes through the render hooks.
        if end_line {
            segments = self.process_render_hooks(vec![segments], &options).concat();
        }
        if segments.is_empty() || !crop {
            return segments;
        }
        // `Console.print(crop=True)`: the final backstop against a line running
        // off the side of the terminal. Renderables that fit are untouched; this
        // is what gives `Overflow::Ignore` its "wrap nothing, but still don't
        // corrupt the display" behaviour. The line breaks come out plain, as
        // upstream's `split_and_crop_lines` writes them.
        Segment::crop_lines(&segments, self.width())
    }

    /// The console's [`style`](Self::style), resolved against its theme
    /// (an unknown name is no style on this lenient print path).
    fn console_style(&self) -> Option<Style> {
        let style = self.style.as_ref()?;
        self.get_style(style).ok().filter(|style| !style.is_null())
    }

    /// Render a renderable to segments. Port of `Console.render`: `options`
    /// defaults to [`options`](Self::options), and nothing is rendered when
    /// there is no width (`max_width < 1`).
    ///
    /// Unlike the print path ([`render_segments_with`](Self::render_segments_with))
    /// the renderable is rendered as-is: no `Text.join`, no fitting and no
    /// crop. Container renderables use this for their children.
    ///
    /// As everywhere in this port, lines are *separated* by newline segments:
    /// the final line has no trailing `"\n"` where upstream's has one.
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
        // `_check_buffer`: a quiet console drops its output, captured or not.
        if segments.is_empty() || self.quiet() {
            return;
        }
        let segments = {
            let mut captures = self.lock_captures();
            if let Some(buffer) = current_capture(&mut captures) {
                buffer.extend(segments);
                if newline {
                    buffer.push(Segment::line());
                }
                return;
            }
            segments
        };
        // `_write_buffer`: what is written is recorded first.
        if self.record() {
            let mut recorded = self.lock_record_buffer();
            recorded.extend(segments.iter().cloned());
            if newline {
                recorded.push(Segment::line());
            }
        }
        let mut output = self.segments_to_string(&segments);
        if newline {
            output.push('\n');
        }
        let stdout = std::io::stdout();
        let mut lock = stdout.lock();
        let mut written = lock.write_all(output.as_bytes());
        if written.is_ok() && !newline {
            written = lock.flush();
        }
        drop(lock);
        // `except BrokenPipeError: self.on_broken_pipe()`. Other write
        // errors are ignored, as this port has always done.
        if written.is_err_and(|error| error.kind() == std::io::ErrorKind::BrokenPipe) {
            self.on_broken_pipe();
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
        // Upstream pads with the `style` argument as given (default `None`),
        // so the pad segments carry no style unless one was passed.
        let pad_style = style.cloned();
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
        // An empty `Text` renders as a lone empty segment: upstream renders it
        // as its `end` newline, which `split_and_crop_lines` turns into one
        // blank line (#442).
        let only_empty = !segments.is_empty()
            && segments
                .iter()
                .all(|segment| !segment.control && segment.text.is_empty());
        let mut lines = Segment::split_lines_owned(segments);
        if lines.is_empty() && only_empty {
            lines.push(Vec::new());
        }
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
        self.render_export_with(renderable, &self.options())
    }

    /// Render a value and write it to stdout, followed by a newline (unless
    /// the renderable ends without one upstream:
    /// [`Renderable::print_end_line`]).
    pub fn print(&self, renderable: &dyn Renderable) {
        self.print_with(renderable, &self.options());
    }

    /// Whether printing `renderable` under `options` ends its last line: a
    /// renderable that `print(justify=…)` wraps in `Align` always does.
    fn print_ends_line(renderable: &dyn Renderable, options: &ConsoleOptions) -> bool {
        renderable.print_end_line(options)
            || (renderable.printed_text().is_none()
                && matches!(
                    options.justify,
                    Justify::Left | Justify::Center | Justify::Right
                ))
    }

    /// Print with explicit render options, the equivalent of upstream's
    /// `Console.print(renderable, justify=…, overflow=…, no_wrap=…)`. Start
    /// from [`options`](Self::options) and set the fields to override. A
    /// printed `Text` defers to these options, because upstream's `Text.join`
    /// drops the text's own `justify`, `overflow` and `no_wrap`.
    pub fn print_with(&self, renderable: &dyn Renderable, options: &ConsoleOptions) {
        let segments = self.print_segments(renderable, options, true);
        self.emit_end(segments, false);
    }

    /// Like [`render_export`](Self::render_export), with explicit render
    /// options as for [`print_with`](Self::print_with).
    pub fn render_export_with(
        &self,
        renderable: &dyn Renderable,
        options: &ConsoleOptions,
    ) -> String {
        let segments = self.print_segments(renderable, options, true);
        self.segments_to_string(&segments)
    }

    /// Write a terminal control sequence to stdout.
    ///
    /// Port of `Console.control`. Control codes are only written when output is
    /// a real terminal (they are meaningless when redirected to a file).
    pub fn control(&self, control: &crate::control::Control) {
        // Upstream buffers control codes with everything else, so a capture
        // or a recording keeps them; only a terminal is sent them
        // (`_render_buffer` drops them otherwise).
        let text = control.as_str();
        if !text.is_empty() {
            self.emit_end(vec![Segment::control(text)], false);
        }
    }

    /// Add a render hook to the stack. Port of `Console.push_render_hook`.
    pub fn push_render_hook(&self, hook: std::sync::Arc<dyn RenderHook>) {
        lock_or_recover(&self.render_hooks).push(hook);
    }

    /// Remove the last render hook pushed. Port of
    /// `Console.pop_render_hook`; with none left, nothing happens (upstream
    /// raises `IndexError`).
    pub fn pop_render_hook(&self) {
        lock_or_recover(&self.render_hooks).pop();
    }

    /// Hand what a print is about to write to the render hooks, in the order
    /// they were pushed, and render what they return. Each item is one
    /// renderable already rendered, its last line ended.
    fn process_render_hooks(
        &self,
        items: Vec<Vec<Segment>>,
        options: &ConsoleOptions,
    ) -> Vec<Vec<Segment>> {
        let hooks = lock_or_recover(&self.render_hooks).clone();
        if hooks.is_empty() {
            return items;
        }
        let mut renderables: Vec<Box<dyn Renderable>> = items
            .into_iter()
            .map(|segments| Box::new(Rendered(segments)) as Box<dyn Renderable>)
            .collect();
        for hook in &hooks {
            renderables = hook.process_renderables(renderables);
        }
        renderables
            .iter()
            .map(|renderable| {
                let mut segments = self.render(renderable.as_ref(), Some(options));
                if !segments.is_empty() && renderable.print_end_line(options) {
                    segments.push(Segment::line());
                }
                segments
            })
            .collect()
    }

    /// Push a live display on to this console's stack, returning whether it
    /// is the only one (the display that draws). Port of `Console.set_live`,
    /// which upstream's `Live` calls when it starts.
    ///
    /// The port's [`Live`](crate::live::Live) owns its console and does not
    /// share it with another display ([DIVERGENCES §17]), so it has no stack
    /// to join; the stack is here for a display that shares a console.
    ///
    /// [DIVERGENCES §17]: https://github.com/buchochelliq-labs/rs-rich-cli/blob/main/docs/DIVERGENCES.md#17-live-owns-its-console-and-sink-redirect-through-proxy-writers-settled-in-0022
    pub fn set_live(&self, live: std::sync::Arc<dyn std::any::Any + Send + Sync>) -> bool {
        let mut stack = lock_or_recover(&self.live_stack);
        stack.push(live);
        stack.len() == 1
    }

    /// Pop the last live display pushed by [`set_live`](Self::set_live).
    /// Port of `Console.clear_live`; with none left, nothing happens
    /// (upstream raises `IndexError`).
    pub fn clear_live(&self) {
        lock_or_recover(&self.live_stack).pop();
    }

    /// Use the alternate screen until the returned guard drops. Port of
    /// `Console.screen(hide_cursor=…, style=…)`, Python's context manager as
    /// an RAII guard: the alternate screen is switched on (and the cursor
    /// hidden, when `hide_cursor`), and switched back when the guard drops,
    /// if it was switched at all (only a terminal is). Print a full-screen
    /// renderable with [`ScreenContext::update`].
    pub fn screen(
        &self,
        hide_cursor: bool,
        style: Option<crate::style::StyleType>,
    ) -> ScreenContext<'_> {
        let changed = self.set_alt_screen(true);
        if changed && hide_cursor {
            self.show_cursor(false);
        }
        // `Screen(style=style or "")`.
        let style = style.unwrap_or_else(|| "".into());
        ScreenContext {
            console: self,
            hide_cursor,
            changed,
            screen: Some(crate::screen::Screen::empty().style(style.clone())),
            style,
        }
    }

    /// Display a status and spinner until it is stopped. Port of
    /// `Console.status`: `status` carries the message and upstream's
    /// keyword arguments (`spinner`, `spinner_style`, `speed`,
    /// `refresh_per_second`), and the display starts at once on a copy of
    /// this console, writing to standard output, as `with
    /// console.status(…):` does; [`LiveStatus::stop`](crate::status::LiveStatus::stop)
    /// ends it.
    pub fn status(
        &self,
        status: crate::status::Status,
    ) -> crate::status::LiveStatus<std::io::Stdout> {
        self.status_to(status, std::io::stdout())
    }

    /// [`status`](Self::status), writing to `writer` instead of standard
    /// output (upstream's console `file`).
    pub fn status_to<W: Write + Send + 'static>(
        &self,
        status: crate::status::Status,
        writer: W,
    ) -> crate::status::LiveStatus<W> {
        status.start(self.clone(), writer)
    }

    /// Whether the alternate screen is enabled. Port of
    /// `Console.is_alt_screen`.
    pub fn is_alt_screen(&self) -> bool {
        self.is_alt_screen.load(std::sync::atomic::Ordering::SeqCst)
    }

    /// Enable or disable the alternate screen. Port of
    /// `Console.set_alt_screen`: only a terminal (not a legacy Windows
    /// console) switches, and the result says whether it did.
    pub fn set_alt_screen(&self, enable: bool) -> bool {
        if self.is_terminal && !self.legacy_windows {
            self.control(&crate::control::Control::alt_screen(enable));
            self.is_alt_screen
                .store(enable, std::sync::atomic::Ordering::SeqCst);
            return true;
        }
        false
    }

    /// Render `renderable` into `region` of the alternate screen (the whole
    /// screen when `None`). Port of `Console.update_screen`.
    pub fn update_screen(
        &self,
        renderable: &dyn Renderable,
        region: Option<crate::region::Region>,
        options: Option<&ConsoleOptions>,
    ) -> crate::errors::Result<()> {
        if !self.is_alt_screen() {
            return Err(no_alt_screen());
        }
        let render_options = options.cloned().unwrap_or_else(|| self.options());
        let (x, y, render_options) = match region {
            None => {
                let height = render_options
                    .height
                    .filter(|&height| height > 0)
                    .unwrap_or(self.height());
                let width = render_options.max_width;
                (0, 0, render_options.update_dimensions(width, height))
            }
            Some(region) => (
                region.x,
                region.y,
                render_options.update_dimensions(region.width, region.height),
            ),
        };
        let lines = self.render_lines(renderable, &render_options, true);
        self.update_screen_lines(&lines, x, y)
    }

    /// Write rendered `lines` to the alternate screen at column `x`, row `y`.
    /// Port of `Console.update_screen_lines` (through [`ScreenUpdate`]).
    pub fn update_screen_lines(
        &self,
        lines: &[Vec<Segment>],
        x: usize,
        y: usize,
    ) -> crate::errors::Result<()> {
        if !self.is_alt_screen() {
            return Err(no_alt_screen());
        }
        let update = ScreenUpdate::new(lines.to_vec(), x, y);
        let segments = self.render(&update, None);
        self.emit_end(segments, false);
        Ok(())
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
    ///
    /// Each closure capture holds only what was printed inside it. Upstream's
    /// nested `capture()` contexts share one buffer, so the inner one takes
    /// everything printed since the outer one began;
    /// [`begin_capture`](Self::begin_capture) and
    /// [`end_capture`](Self::end_capture) behave that way.
    pub fn capture(&self, f: impl FnOnce(&Console)) -> String {
        let segments = self.capture_segments(f);
        self.segments_to_string(&segments)
    }

    /// Begin capturing this thread's output: what is printed is held until
    /// [`end_capture`](Self::end_capture) instead of written. Port of
    /// `Console.begin_capture`.
    pub fn begin_capture(&self) {
        self.lock_captures()
            .entry(std::thread::current().id())
            .or_default()
            .push(Vec::new());
    }

    /// End the innermost capture begun with
    /// [`begin_capture`](Self::begin_capture) on this thread, returning what
    /// was printed as a terminal string. Port of `Console.end_capture`.
    ///
    /// As upstream, which holds one buffer per thread, everything captured
    /// on the thread so far is returned, including what outer captures hold,
    /// and those are left empty. Without a capture to end, the result is
    /// empty (upstream's console would stop writing altogether).
    pub fn end_capture(&self) -> String {
        let id = std::thread::current().id();
        let segments: Vec<Segment> = {
            let mut captures = self.lock_captures();
            let Some(stack) = captures.get_mut(&id) else {
                return String::new();
            };
            let segments = stack.iter_mut().flat_map(std::mem::take).collect();
            stack.pop();
            if stack.is_empty() {
                captures.remove(&id);
            }
            segments
        };
        self.segments_to_string(&segments)
    }

    /// Like [`capture`](Self::capture) but with all styles stripped, returning
    /// plain text. Port of `Console.export_text(styles=False)`.
    pub fn export_text(&self, f: impl FnOnce(&Console)) -> String {
        let segments = self.capture_segments(f);
        segments_to_plain(&segments)
    }

    /// Like [`export_text`](Self::export_text) with escape codes. Port of
    /// `Console.export_text(styles=True)`: each segment in the console's
    /// colour system (upstream's style keeps the codes printing rendered),
    /// or in truecolor, `Style.render`'s default, without one.
    pub fn export_text_styled(&self, f: impl FnOnce(&Console)) -> String {
        let segments = self.capture_segments(f);
        self.segments_to_styled_text(&segments)
    }

    /// Write what `f` prints to `path` as text, with escape codes when
    /// `styles`. Port of `Console.save_text`.
    pub fn save_text(
        &self,
        path: impl AsRef<std::path::Path>,
        styles: bool,
        f: impl FnOnce(&Console),
    ) -> std::io::Result<()> {
        let text = if styles {
            self.export_text_styled(f)
        } else {
            self.export_text(f)
        };
        std::fs::write(path, text)
    }

    /// Write what `f` prints to `path` as HTML with a stylesheet. Port of
    /// `Console.save_html` (its default `inline_styles=False`).
    pub fn save_html(
        &self,
        path: impl AsRef<std::path::Path>,
        f: impl FnOnce(&Console),
    ) -> std::io::Result<()> {
        std::fs::write(path, self.export_html_classes(f))
    }

    /// Write what `f` prints to `path` as SVG. Port of `Console.save_svg`;
    /// as for [`export_svg`](Self::export_svg), the `unique_id` is given.
    pub fn save_svg(
        &self,
        path: impl AsRef<std::path::Path>,
        title: &str,
        unique_id: &str,
        f: impl FnOnce(&Console),
    ) -> std::io::Result<()> {
        std::fs::write(path, self.export_svg(title, unique_id, f))
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
    /// — the seam upstream exposes as `Console.pager(pager=…)`. Hyperlinks
    /// are stripped, as upstream's default `links=False` does.
    pub fn page_with(
        &self,
        pager: &dyn crate::pager::Pager,
        styles: bool,
        f: impl FnOnce(&Console),
    ) -> std::io::Result<()> {
        self.page_with_links(pager, styles, false, f)
    }

    /// Like [`page_with`](Self::page_with), keeping hyperlinks when `links`
    /// and `styles` are both set. Port of `Console.pager(pager, styles,
    /// links)`: without `styles` everything is plain, and without `links`
    /// each style loses its link (`Segment.strip_links`).
    pub fn page_with_links(
        &self,
        pager: &dyn crate::pager::Pager,
        styles: bool,
        links: bool,
        f: impl FnOnce(&Console),
    ) -> std::io::Result<()> {
        let mut segments = self.capture_segments(f);
        let content = if !styles {
            segments_to_plain(&segments)
        } else {
            if !links {
                for segment in &mut segments {
                    if let Some(style) = segment.style.as_mut() {
                        if style.link().is_some() {
                            *style = style.update_link(None);
                        }
                    }
                }
            }
            self.segments_to_string(&segments)
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
        let segments = self.capture_segments(f);
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
        let segments = self.capture_segments(f);
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
        let segments = self.capture_segments(f);
        crate::svg::export_svg(&segments, theme, title, unique_id, self.width())
    }

    /// Capture output printed inside `f` and export it as HTML with every
    /// option of upstream's `Console.export_html`: `code_format` (default
    /// [`CONSOLE_HTML_FORMAT`](crate::export::CONSOLE_HTML_FORMAT)) and
    /// `inline_styles`. See [`export::export_html_with`](crate::export::export_html_with).
    pub fn export_html_with(
        &self,
        theme: &crate::terminal_theme::TerminalTheme,
        code_format: Option<&str>,
        inline_styles: bool,
        f: impl FnOnce(&Console),
    ) -> Result<String, crate::export::ExportFormatError> {
        let segments = self.capture_segments(f);
        crate::export::export_html_with(&segments, theme, code_format, inline_styles)
    }

    /// Capture output printed inside `f` and export it as SVG with every
    /// option of upstream's `Console.export_svg`: `code_format` (default
    /// [`CONSOLE_SVG_FORMAT`](crate::svg::CONSOLE_SVG_FORMAT)) and
    /// `font_aspect_ratio` (upstream default 0.61). See
    /// [`svg::export_svg_with`](crate::svg::export_svg_with).
    #[allow(clippy::too_many_arguments)]
    pub fn export_svg_with(
        &self,
        theme: &crate::terminal_theme::TerminalTheme,
        title: &str,
        unique_id: &str,
        code_format: Option<&str>,
        font_aspect_ratio: f64,
        f: impl FnOnce(&Console),
    ) -> Result<String, crate::export::ExportFormatError> {
        let segments = self.capture_segments(f);
        crate::svg::export_svg_with(
            &segments,
            theme,
            title,
            unique_id,
            self.width(),
            code_format.unwrap_or(crate::svg::CONSOLE_SVG_FORMAT),
            font_aspect_ratio,
        )
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
        self.capture_segments(f)
    }

    /// Run `f` with this thread's output recorded to a fresh buffer, returning
    /// the captured segments. Captures nest (each pushes its own buffer), are
    /// per thread (upstream's `ConsoleThreadLocals`), and end even if `f`
    /// panics (upstream's `Capture.__exit__` always runs).
    fn capture_segments(&self, f: impl FnOnce(&Console)) -> Vec<Segment> {
        /// Pops this thread's capture buffer on drop, so an unwinding `f`
        /// does not leave the console capturing.
        struct CaptureGuard<'a>(&'a Console);
        impl CaptureGuard<'_> {
            fn pop(&self) -> Vec<Segment> {
                let id = std::thread::current().id();
                let mut captures = self.0.lock_captures();
                let Some(stack) = captures.get_mut(&id) else {
                    return Vec::new();
                };
                let captured = stack.pop().unwrap_or_default();
                if stack.is_empty() {
                    captures.remove(&id);
                }
                captured
            }
        }
        impl Drop for CaptureGuard<'_> {
            fn drop(&mut self) {
                self.pop();
            }
        }
        self.lock_captures()
            .entry(std::thread::current().id())
            .or_default()
            .push(Vec::new());
        let guard = CaptureGuard(self);
        f(self);
        let captured = guard.pop();
        std::mem::forget(guard);
        captured
    }

    /// Whether what is written is recorded. Port of `Console.record`.
    pub fn record(&self) -> bool {
        self.record.load(std::sync::atomic::Ordering::SeqCst)
    }

    /// Start (or stop) recording. Upstream's `Console.record` is a plain
    /// attribute.
    pub fn set_record(&self, record: bool) {
        self.record
            .store(record, std::sync::atomic::Ordering::SeqCst);
    }

    /// The recorded segments, cleared when `clear`; an error when the
    /// console is not recording (upstream's assertion).
    fn take_recorded(&self, clear: bool) -> crate::errors::Result<Vec<Segment>> {
        if !self.record() {
            return Err(crate::errors::RichError::Console(
                "To export console contents set record=True in the constructor or instance"
                    .to_string(),
            ));
        }
        let mut recorded = self.lock_record_buffer();
        Ok(if clear {
            std::mem::take(&mut *recorded)
        } else {
            recorded.clone()
        })
    }

    /// The recorded output as text, with escape codes when `styles`, the
    /// record cleared when `clear`. Port of `Console.export_text(clear=…,
    /// styles=…)` for a console built with
    /// [`record`](ConsoleBuilder::record); upstream's default is
    /// `clear=True, styles=False`. Without plain text, control codes are
    /// left out; with styles they stay, as upstream's.
    ///
    /// The styled form renders each style in the console's colour system,
    /// truecolor without one, as [`export_text_styled`](Self::export_text_styled).
    ///
    /// # Errors
    ///
    /// [`RichError::Console`](crate::errors::RichError::Console) when the
    /// console is not recording (upstream asserts).
    pub fn export_text_recorded(&self, clear: bool, styles: bool) -> crate::errors::Result<String> {
        let segments = self.take_recorded(clear)?;
        Ok(if styles {
            self.segments_to_styled_text(&segments)
        } else {
            segments_to_plain(&segments)
        })
    }

    /// Write [`export_text_recorded`](Self::export_text_recorded) to `path`.
    /// Port of `Console.save_text(path, clear=…, styles=…)`.
    pub fn save_text_recorded(
        &self,
        path: impl AsRef<std::path::Path>,
        clear: bool,
        styles: bool,
    ) -> std::io::Result<()> {
        let text = self
            .export_text_recorded(clear, styles)
            .map_err(std::io::Error::other)?;
        std::fs::write(path, text)
    }

    /// The recorded output as an HTML document. Port of
    /// `Console.export_html(theme=…, clear=…, code_format=…,
    /// inline_styles=…)` for a console built with
    /// [`record`](ConsoleBuilder::record).
    ///
    /// # Errors
    ///
    /// [`RichError::Console`](crate::errors::RichError::Console) when the
    /// console is not recording, or when `code_format` is not a template
    /// this port can fill (see [`export::export_html_with`](crate::export::export_html_with)).
    pub fn export_html_recorded(
        &self,
        options: &ExportHtmlOptions<'_>,
    ) -> crate::errors::Result<String> {
        let segments = self.take_recorded(options.clear)?;
        let theme = options
            .theme
            .unwrap_or(&crate::terminal_theme::DEFAULT_TERMINAL_THEME);
        crate::export::export_html_with(
            &segments,
            theme,
            options.code_format,
            options.inline_styles,
        )
        .map_err(|error| crate::errors::RichError::Console(error.to_string()))
    }

    /// Write [`export_html_recorded`](Self::export_html_recorded) to `path`.
    /// Port of `Console.save_html`.
    pub fn save_html_recorded(
        &self,
        path: impl AsRef<std::path::Path>,
        options: &ExportHtmlOptions<'_>,
    ) -> std::io::Result<()> {
        let html = self
            .export_html_recorded(options)
            .map_err(std::io::Error::other)?;
        std::fs::write(path, html)
    }

    /// The recorded output as an SVG image of a terminal window. Port of
    /// `Console.export_svg(title=…, theme=…, clear=…, code_format=…,
    /// font_aspect_ratio=…, unique_id=…)` for a console built with
    /// [`record`](ConsoleBuilder::record). Control codes are left out, and
    /// the image is as wide as the console is now.
    ///
    /// # Errors
    ///
    /// As for [`export_html_recorded`](Self::export_html_recorded).
    pub fn export_svg_recorded(
        &self,
        options: &ExportSvgOptions<'_>,
    ) -> crate::errors::Result<String> {
        let segments: Vec<Segment> = self
            .take_recorded(options.clear)?
            .into_iter()
            .filter(|segment| !segment.control)
            .collect();
        let theme = options
            .theme
            .unwrap_or(&crate::terminal_theme::SVG_EXPORT_THEME);
        crate::svg::export_svg_with(
            &segments,
            theme,
            options.title,
            options.unique_id,
            self.width(),
            options
                .code_format
                .unwrap_or(crate::svg::CONSOLE_SVG_FORMAT),
            options.font_aspect_ratio,
        )
        .map_err(|error| crate::errors::RichError::Console(error.to_string()))
    }

    /// Write [`export_svg_recorded`](Self::export_svg_recorded) to `path`.
    /// Port of `Console.save_svg`.
    pub fn save_svg_recorded(
        &self,
        path: impl AsRef<std::path::Path>,
        options: &ExportSvgOptions<'_>,
    ) -> std::io::Result<()> {
        let svg = self
            .export_svg_recorded(options)
            .map_err(std::io::Error::other)?;
        std::fs::write(path, svg)
    }

    /// The record buffer, recovering it if a panicking print poisoned the
    /// lock.
    fn lock_record_buffer(&self) -> std::sync::MutexGuard<'_, Vec<Segment>> {
        self.record_buffer
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Segments as text with each style's escape codes, in the console's
    /// colour system (truecolor without one); control codes kept.
    fn segments_to_styled_text(&self, segments: &[Segment]) -> String {
        let system = self.color_system().unwrap_or(ColorSystem::Truecolor);
        segments
            .iter()
            .map(|segment| match &segment.style {
                Some(style) => style.render(&segment.text, Some(system)),
                None => segment.text.to_string(),
            })
            .collect()
    }

    /// The per-thread capture stacks, recovering them if a panicking print
    /// poisoned the lock.
    fn lock_captures(&self) -> std::sync::MutexGuard<'_, CaptureStacks> {
        self.captures
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
        let markup = self.parse_markup(content, self.emoji)?;

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
            self.parse_markup(content, self.emoji)
                .unwrap_or_else(|_| Text::new(self.expand_emoji(content)))
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

    /// Port of `markup.render(content, emoji=emoji, emoji_variant=…)`: a
    /// string with no `[` is only emoji-replaced (with the console's default
    /// variant); otherwise emoji codes are replaced chunk by chunk between the
    /// tags, so error positions refer to the original string.
    fn parse_markup(&self, content: &str, emoji: bool) -> crate::errors::Result<Text> {
        if !content.contains('[') {
            return Ok(Text::new(if emoji {
                crate::emoji::replace_with_variant(content, self.emoji_variant)
            } else {
                content.to_string()
            }));
        }
        if emoji {
            crate::markup::render_emoji(content)
        } else {
            crate::markup::render(content)
        }
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
            self.parse_markup(content, emoji)?
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

/// A string renders as upstream's `Console.render` renders a `str`: through
/// [`Console::render_str`] with the options' `highlight` and `markup`
/// (`None` taking the console's defaults), then as that `Text`. Malformed
/// markup falls back to the literal text, as `render_str` does.
impl Renderable for String {
    fn rich_render(&self, console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
        self.options_text(console, options, options.highlight)
            .rich_render(console, options)
    }

    /// `Measurement.get` of a `str`: `render_str(markup=options.markup,
    /// highlight=False)`, then the text's measurement.
    fn measure(&self, console: &Console, options: &ConsoleOptions) -> crate::measure::Measurement {
        let text = self.options_text(console, options, Some(false));
        crate::measure::Measurement::get(console, options, &text)
    }
}

/// `render_str` for a `str` renderable under `options`.
trait OptionsText {
    fn options_text(
        &self,
        console: &Console,
        options: &ConsoleOptions,
        highlight: Option<bool>,
    ) -> Text;
}

impl OptionsText for String {
    fn options_text(
        &self,
        console: &Console,
        options: &ConsoleOptions,
        highlight: Option<bool>,
    ) -> Text {
        let render_options = RenderStrOptions {
            highlight,
            markup: options.markup,
            ..Default::default()
        };
        console
            .render_str_with(self, &render_options)
            .unwrap_or_else(|_| {
                console
                    .render_str_with(
                        self,
                        &RenderStrOptions {
                            markup: Some(false),
                            ..render_options
                        },
                    )
                    .unwrap_or_else(|_| Text::new(self.as_str()))
            })
    }
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
        // Wrap to the available width; the effective justify is this text's own
        // justify, falling back to the console options' justify.
        let justify = self.get_justify_option().unwrap_or(options.justify);
        // A justified empty line is still padded to the width (upstream's
        // `truncate(width, pad=True)`), so only an unjustified one is bare.
        if self.is_empty() && matches!(justify, Justify::Default | Justify::Full) {
            return vec![Segment::new("", None)];
        }
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

/// The keyword arguments of upstream's `Console.export_html` and
/// `save_html`, for [`Console::export_html_recorded`].
#[derive(Clone, Copy)]
pub struct ExportHtmlOptions<'a> {
    /// `theme=`: the palette, or `None` for
    /// [`DEFAULT_TERMINAL_THEME`](crate::terminal_theme::DEFAULT_TERMINAL_THEME).
    pub theme: Option<&'a crate::terminal_theme::TerminalTheme>,
    /// `clear=`: empty the record afterwards (default `true`).
    pub clear: bool,
    /// `code_format=`: the page template, or `None` for
    /// [`CONSOLE_HTML_FORMAT`](crate::export::CONSOLE_HTML_FORMAT).
    pub code_format: Option<&'a str>,
    /// `inline_styles=`: styles on each span rather than in a stylesheet
    /// (default `false`).
    pub inline_styles: bool,
}

impl Default for ExportHtmlOptions<'_> {
    fn default() -> Self {
        ExportHtmlOptions {
            theme: None,
            clear: true,
            code_format: None,
            inline_styles: false,
        }
    }
}

/// The keyword arguments of upstream's `Console.export_svg` and `save_svg`,
/// for [`Console::export_svg_recorded`].
#[derive(Clone, Copy)]
pub struct ExportSvgOptions<'a> {
    /// `title=`: the window title (default `"Rich"`).
    pub title: &'a str,
    /// `theme=`: the palette, or `None` for
    /// [`SVG_EXPORT_THEME`](crate::terminal_theme::SVG_EXPORT_THEME).
    pub theme: Option<&'a crate::terminal_theme::TerminalTheme>,
    /// `clear=`: empty the record afterwards (default `true`).
    pub clear: bool,
    /// `code_format=`: the image template, or `None` for
    /// [`CONSOLE_SVG_FORMAT`](crate::svg::CONSOLE_SVG_FORMAT).
    pub code_format: Option<&'a str>,
    /// `font_aspect_ratio=`: the font's width to height (default 0.61).
    pub font_aspect_ratio: f64,
    /// `unique_id=`: the prefix of the image's ids and classes. Upstream's
    /// default hashes Python's `repr()` of the segments, which Rust cannot
    /// reproduce ([DIVERGENCES §15]); the default here is `"terminal"`.
    ///
    /// [DIVERGENCES §15]: https://github.com/buchochelliq-labs/rs-rich-cli/blob/main/docs/DIVERGENCES.md#15-exports-done-html-both-forms--svg-svg-needs-an-explicit-unique_id
    pub unique_id: &'a str,
}

impl Default for ExportSvgOptions<'_> {
    fn default() -> Self {
        ExportSvgOptions {
            title: "Rich",
            theme: None,
            clear: true,
            code_format: None,
            font_aspect_ratio: 0.61,
            unique_id: "terminal",
        }
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
    environ: Option<Environ>,
    quiet: bool,
    soft_wrap: bool,
    style: Option<crate::style::StyleType>,
    on_broken_pipe: Option<BrokenPipeHandler>,
    record: bool,
    log_time: bool,
    log_path: bool,
    log_time_format: Option<LogTimeFormat>,
    get_datetime: Option<GetDateTime>,
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
            environ: None,
            quiet: false,
            soft_wrap: false,
            style: None,
            on_broken_pipe: None,
            record: false,
            log_time: true,
            log_path: true,
            log_time_format: None,
            get_datetime: None,
        }
    }

    /// Show the time column in [`Console::log`] (default on). Port of
    /// `Console(log_time=…)`.
    pub fn log_time(mut self, value: bool) -> Self {
        self.log_time = value;
        self
    }

    /// Show the caller's file and line in [`Console::log`] (default on).
    /// Port of `Console(log_path=…)`.
    pub fn log_path(mut self, value: bool) -> Self {
        self.log_path = value;
        self
    }

    /// The `strftime` format of the time column in [`Console::log`]
    /// (default `"[%X]"`). Port of `Console(log_time_format="…")`; the time
    /// is formatted in UTC (see [`get_datetime`](Self::get_datetime)).
    pub fn log_time_format(mut self, format: impl Into<String>) -> Self {
        self.log_time_format = Some(LogTimeFormat::Strftime(format.into()));
        self
    }

    /// Format the time column of [`Console::log`] with `format`. Port of
    /// `Console(log_time_format=callable)`.
    pub fn log_time_formatter(
        mut self,
        format: impl Fn(std::time::SystemTime) -> Text + Send + Sync + 'static,
    ) -> Self {
        self.log_time_format = Some(LogTimeFormat::Callable(std::sync::Arc::new(format)));
        self
    }

    /// Read the time [`Console::log`] shows from `clock` (default
    /// [`SystemTime::now`](std::time::SystemTime::now)). Port of
    /// `Console(get_datetime=…)`. Upstream's `datetime.now()` is local
    /// time; core has no time zone data, so its `strftime` formats the UTC
    /// wall-clock time, and a clock that wants local time adds the offset.
    pub fn get_datetime(
        mut self,
        clock: impl Fn() -> std::time::SystemTime + Send + Sync + 'static,
    ) -> Self {
        self.get_datetime = Some(std::sync::Arc::new(clock));
        self
    }

    /// Record what is written (default off), for
    /// [`Console::export_text_recorded`], [`Console::export_html_recorded`]
    /// and [`Console::export_svg_recorded`]. Port of `Console(record=…)`.
    pub fn record(mut self, value: bool) -> Self {
        self.record = value;
        self
    }

    /// Detect the terminal from `environ` instead of the process
    /// environment: `NO_COLOR`, `COLUMNS`, `LINES`, `TERM`, `COLORTERM`,
    /// `TTY_COMPATIBLE` and `FORCE_COLOR`. Port of `Console(_environ=…)`,
    /// which upstream provides for tests.
    pub fn environ(mut self, environ: Environ) -> Self {
        self.environ = Some(environ);
        self
    }

    /// Suppress all output (default off). Port of `Console(quiet=…)`.
    pub fn quiet(mut self, value: bool) -> Self {
        self.quiet = value;
        self
    }

    /// The default of `print(soft_wrap=…)` (default off): no wrapping and
    /// no cropping. Port of `Console(soft_wrap=…)`.
    pub fn soft_wrap(mut self, value: bool) -> Self {
        self.soft_wrap = value;
        self
    }

    /// A style applied to all output (default none). Port of
    /// `Console(style=…)`.
    pub fn style(mut self, style: impl Into<crate::style::StyleType>) -> Self {
        self.style = Some(style.into());
        self
    }

    /// Call `handler` in place of [`Console::on_broken_pipe`]'s default
    /// (quiet, then exit with status 1) when a write finds the pipe closed.
    /// Upstream overrides `Console.on_broken_pipe` in a subclass.
    pub fn on_broken_pipe(mut self, handler: impl Fn(&Console) + Send + Sync + 'static) -> Self {
        self.on_broken_pipe = Some(std::sync::Arc::new(handler));
        self
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
        let environ = self.environ.map(std::sync::Arc::new);
        let env = environ.as_deref();
        let legacy_windows = self.legacy_windows.unwrap_or(false);
        let is_terminal = self
            .force_terminal
            .unwrap_or_else(|| detect_is_terminal(env));
        // Upstream's rule is `environ.get("NO_COLOR", "") != ""`, so an EMPTY
        // NO_COLOR does not disable colour — only a non-empty value does. That
        // matters because a shell that exports `NO_COLOR=` (a common way to
        // clear it) would otherwise still be treated as opting out.
        let no_color = self
            .no_color
            .unwrap_or_else(|| environ_get(env, "NO_COLOR").is_some_and(|value| !value.is_empty()));
        // `color_system="auto"` (the default) detects; an explicit system,
        // or `None`, is taken as given.
        let color_system = if self.color_system_set {
            self.color_system
        } else if is_terminal && !term_is_dumb(env) {
            Some(detect_color_system(env))
        } else {
            None
        };
        // `width = int(columns) - self.legacy_windows`: `COLUMNS` and
        // `LINES` become the console's own width and height.
        let width = self.width.or_else(|| {
            env_int(env, "COLUMNS")
                .map(|columns| columns.saturating_sub(usize::from(legacy_windows)))
        });
        let height = self.height.or_else(|| env_int(env, "LINES"));
        let terminal_size = detect_terminal_size(legacy_windows);
        Console {
            render_environment: None,
            region_sink: None,
            code_highlighting: None,
            color_system,
            width,
            height,
            terminal_size,
            environ,
            is_terminal,
            no_color,
            quiet: std::sync::atomic::AtomicBool::new(self.quiet),
            soft_wrap: self.soft_wrap,
            style: self.style,
            on_broken_pipe: self.on_broken_pipe,
            get_time: self
                .get_time
                .unwrap_or_else(|| std::sync::Arc::new(monotonic)),
            emoji: self.emoji.unwrap_or(true),
            // Upstream's `Console(highlight=True)` default. Getting this wrong is
            // invisible in the fixtures (every one is captured with
            // highlight=False) but is the first thing a user sees: numbers,
            // paths, booleans and URLs come out plain instead of coloured.
            highlight: self.highlight.unwrap_or(true),
            legacy_windows,
            safe_box: self.safe_box.unwrap_or(true),
            ascii_only: self.ascii_only.unwrap_or(false),
            theme_stack: vec![self.theme.unwrap_or_else(Theme::default_theme)],
            base_style: Style::new(),
            highlighters: Vec::new(),
            markup: self.markup.unwrap_or(true),
            emoji_variant: self.emoji_variant,
            // `tab_size: int = 8`.
            tab_size: self.tab_size.unwrap_or(crate::text::DEFAULT_TAB_SIZE),
            captures: std::sync::Mutex::new(CaptureStacks::new()),
            is_alt_screen: std::sync::atomic::AtomicBool::new(false),
            record: std::sync::atomic::AtomicBool::new(self.record),
            record_buffer: std::sync::Mutex::new(Vec::new()),
            render_hooks: std::sync::Mutex::new(Vec::new()),
            live_stack: std::sync::Mutex::new(Vec::new()),
            log_render: std::sync::Mutex::new(new_log_render(self.log_time, self.log_path)),
            log_time: self.log_time,
            log_path: self.log_path,
            log_time_format: self
                .log_time_format
                .unwrap_or_else(|| LogTimeFormat::Strftime("[%X]".to_string())),
            get_datetime: self
                .get_datetime
                .unwrap_or_else(|| std::sync::Arc::new(std::time::SystemTime::now)),
        }
    }
}

/// Python's `os.path.abspath`: `path` joined to the working directory
/// when it is relative.
fn absolute_path(path: &str) -> String {
    let path = std::path::Path::new(path);
    if path.is_absolute() {
        return path.to_string_lossy().into_owned();
    }
    std::env::current_dir()
        .map(|directory| directory.join(path).to_string_lossy().into_owned())
        .unwrap_or_else(|_| path.to_string_lossy().into_owned())
}

/// Upstream's `LogRender(show_time=log_time, show_path=log_path)`.
fn new_log_render(log_time: bool, log_path: bool) -> crate::log_render::LogRender {
    crate::log_render::LogRender::new()
        .show_time(log_time)
        .show_path(log_path)
}

/// Whether `TERM` names a terminal that cannot render styles (`dumb` or
/// `unknown`, any case). The environment half of `Console.is_dumb_terminal`.
fn term_is_dumb(environ: Option<&Environ>) -> bool {
    environ_get(environ, "TERM")
        .is_some_and(|term| matches!(term.to_lowercase().as_str(), "dumb" | "unknown"))
}

/// Whether standard output is a terminal. Port of the `Console.is_terminal`
/// property without `force_terminal`: `TTY_COMPATIBLE=0` or `1` decides,
/// then a set `FORCE_COLOR` (a terminal unless empty), then `isatty`.
fn detect_is_terminal(environ: Option<&Environ>) -> bool {
    match environ_get(environ, "TTY_COMPATIBLE").as_deref() {
        Some("0") => return false,
        Some("1") => return true,
        _ => {}
    }
    if let Some(force_color) = environ_get(environ, "FORCE_COLOR") {
        return !force_color.is_empty();
    }
    std::io::stdout().is_terminal()
}

/// Upstream's `_TERM_COLORS`: the colour system named by the last
/// hyphenated part of `TERM`.
fn term_colors(colors: &str) -> Option<ColorSystem> {
    match colors {
        "kitty" | "256color" => Some(ColorSystem::EightBit),
        "16color" => Some(ColorSystem::Standard),
        _ => None,
    }
}

/// Detect the terminal color system. Port of `Console._detect_color_system`
/// for a terminal that is not dumb (the caller checks).
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
#[cfg_attr(windows, allow(unused_variables))]
fn detect_color_system(environ: Option<&Environ>) -> ColorSystem {
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
        let color_term = environ_get(environ, "COLORTERM")
            .unwrap_or_default()
            .trim()
            .to_lowercase();
        if matches!(color_term.as_str(), "truecolor" | "24bit") {
            return ColorSystem::Truecolor;
        }
        let term = environ_get(environ, "TERM")
            .unwrap_or_default()
            .trim()
            .to_lowercase();
        // `term.rpartition("-")[2]`: the whole name when it has no hyphen.
        let colors = term.rsplit('-').next().unwrap_or("");
        term_colors(colors).unwrap_or(ColorSystem::Standard)
    }
}

/// The terminal's size, 80x25 where it reports none (or zero), the width
/// one less on a legacy Windows console. The `os.get_terminal_size` half of
/// upstream's `Console.size`.
fn detect_terminal_size(legacy_windows: bool) -> ConsoleDimensions {
    let (width, height) = match terminal_size::terminal_size() {
        Some((terminal_size::Width(w), terminal_size::Height(h))) => (w as usize, h as usize),
        None => (0, 0),
    };
    let width = if width == 0 { DEFAULT_WIDTH } else { width };
    let height = if height == 0 { DEFAULT_HEIGHT } else { height };
    ConsoleDimensions {
        width: width - usize::from(legacy_windows),
        height,
    }
}

impl crate::protocol::ConsoleCodeHighlighting for Console {
    fn set_code_highlighting(&mut self, value: Option<crate::protocol::CodeHighlighting>) {
        self.code_highlighting = value;
    }
    fn code_highlighting(&self) -> Option<&crate::protocol::CodeHighlighting> {
        self.code_highlighting.as_ref()
    }
}

impl crate::protocol::ConsoleRegions for Console {
    fn set_region_sink(&mut self, value: Option<std::sync::Arc<dyn crate::protocol::RegionSink>>) {
        self.region_sink = value;
    }
    fn region_sink(&self) -> Option<&dyn crate::protocol::RegionSink> {
        self.region_sink.as_deref()
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

/// One object passed to [`Console::print_objects`]: a string (rendered as
/// console markup), a `Text`, or any other renderable.
pub enum PrintObject<'a> {
    /// A string, through `render_str` with the call's `emoji`, `markup` and
    /// `highlight`.
    Str(&'a str),
    /// A `Text`, joined with the strings around it.
    Text(&'a Text),
    /// Any other renderable, which stands alone.
    Renderable(&'a dyn Renderable),
}

/// The keyword arguments of upstream's `Console.print`, after defaults.
#[derive(Clone)]
pub struct PrintOptions<'a> {
    pub sep: &'a str,
    pub end: &'a str,
    pub style: Option<crate::style::StyleType>,
    pub justify: Option<Justify>,
    pub overflow: Option<Overflow>,
    pub no_wrap: Option<bool>,
    pub emoji: Option<bool>,
    pub markup: Option<bool>,
    pub highlight: Option<bool>,
    pub width: Option<usize>,
    pub height: Option<usize>,
    pub crop: bool,
    pub soft_wrap: Option<bool>,
    pub new_line_start: bool,
}

impl Default for PrintOptions<'_> {
    fn default() -> Self {
        PrintOptions {
            sep: " ",
            end: "\n",
            style: None,
            justify: None,
            overflow: None,
            no_wrap: None,
            emoji: None,
            markup: None,
            highlight: None,
            width: None,
            height: None,
            crop: true,
            soft_wrap: None,
            new_line_start: false,
        }
    }
}

/// One object passed to [`Console::log`]: a string (rendered as console
/// markup), a `Text`, or any other renderable. A log keeps its renderables
/// in the message column of a table, so they are shared rather than
/// borrowed.
#[derive(Clone)]
pub enum LogObject<'a> {
    /// A string, through `render_str` with the call's `emoji`, `markup` and
    /// `highlight`.
    Str(&'a str),
    /// A `Text`, joined with the strings around it.
    Text(&'a Text),
    /// Any other renderable, which stands alone.
    Renderable(std::sync::Arc<dyn Renderable + Send + Sync>),
}

/// The keyword arguments of upstream's `Console.log`, after defaults.
/// `log_locals` is not ported ([DIVERGENCES §43]).
///
/// [DIVERGENCES §43]: https://github.com/buchochelliq-labs/rs-rich-cli/blob/main/docs/DIVERGENCES.md#43-console-features-tied-to-python-objects-or-jupyter
#[derive(Clone)]
pub struct LogOptions<'a> {
    pub sep: &'a str,
    pub end: &'a str,
    pub style: Option<crate::style::StyleType>,
    pub justify: Option<Justify>,
    pub emoji: Option<bool>,
    pub markup: Option<bool>,
    pub highlight: Option<bool>,
}

impl Default for LogOptions<'_> {
    fn default() -> Self {
        LogOptions {
            sep: " ",
            end: "\n",
            style: None,
            justify: None,
            emoji: None,
            markup: None,
            highlight: None,
        }
    }
}

/// A [`Console::log`] call that owns its arguments, so that it can be sent
/// to the thread that owns the console (a live display's refresh thread).
pub(crate) struct OwnedLog {
    objects: Vec<OwnedLogObject>,
    sep: String,
    end: String,
    style: Option<crate::style::StyleType>,
    justify: Option<Justify>,
    emoji: Option<bool>,
    markup: Option<bool>,
    highlight: Option<bool>,
    file: String,
    line: u32,
}

enum OwnedLogObject {
    Str(String),
    Text(Text),
    Renderable(std::sync::Arc<dyn Renderable + Send + Sync>),
}

impl OwnedLog {
    /// Copy `objects` and `options`, logged as from `file` and `line`.
    pub(crate) fn new(
        objects: &[LogObject<'_>],
        options: &LogOptions<'_>,
        file: &str,
        line: u32,
    ) -> Self {
        OwnedLog {
            objects: objects
                .iter()
                .map(|object| match object {
                    LogObject::Str(content) => OwnedLogObject::Str((*content).to_string()),
                    LogObject::Text(text) => OwnedLogObject::Text((*text).clone()),
                    LogObject::Renderable(renderable) => {
                        OwnedLogObject::Renderable(renderable.clone())
                    }
                })
                .collect(),
            sep: options.sep.to_string(),
            end: options.end.to_string(),
            style: options.style.clone(),
            justify: options.justify,
            emoji: options.emoji,
            markup: options.markup,
            highlight: options.highlight,
            file: file.to_string(),
            line,
        }
    }

    /// [`Console::log_at`] on `console`.
    pub(crate) fn log(&self, console: &Console) -> crate::errors::Result<()> {
        let objects: Vec<LogObject<'_>> = self
            .objects
            .iter()
            .map(|object| match object {
                OwnedLogObject::Str(content) => LogObject::Str(content),
                OwnedLogObject::Text(text) => LogObject::Text(text),
                OwnedLogObject::Renderable(renderable) => LogObject::Renderable(renderable.clone()),
            })
            .collect();
        let options = LogOptions {
            sep: &self.sep,
            end: &self.end,
            style: self.style.clone(),
            justify: self.justify,
            emoji: self.emoji,
            markup: self.markup,
            highlight: self.highlight,
        };
        console.log_at(&objects, &options, &self.file, self.line)
    }
}

/// One renderable `_collect_renderables` hands [`Console::log`], with the
/// wrappers upstream puts round it: `Align` for `justify`, `Styled` for the
/// console's style and then for the call's. A joined text carries its `end`.
struct LogItem {
    renderable: std::sync::Arc<dyn Renderable + Send + Sync>,
    align: Option<crate::align::HorizontalAlign>,
    /// The text's `end` after its lines, less the trailing line break this
    /// port's lines do not end with. What runs past the column's width is
    /// cropped, as upstream's.
    end: Option<String>,
    styles: Vec<Style>,
}

impl Renderable for LogItem {
    fn rich_render(&self, console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
        let mut segments = match self.align {
            Some(align) => {
                crate::align::Align::render_child(&*self.renderable, align, console, options)
            }
            None => self.renderable.rich_render(console, options),
        };
        if let Some(end) = self.end.as_deref().filter(|end| !end.is_empty()) {
            segments.push(Segment::new(end, None));
            if end.ends_with('\n') {
                // A line break ending the text still ends a (blank) line.
                segments.push(Segment::new("", None));
            }
        }
        for style in &self.styles {
            segments = Segment::apply_style_with(&segments, Some(style), None);
        }
        segments
    }

    fn measure(&self, console: &Console, options: &ConsoleOptions) -> crate::measure::Measurement {
        crate::measure::Measurement::get(console, options, &*self.renderable)
    }
}

/// The upstream `Console` methods built on its print pipeline.
impl Console {
    /// Log `objects` with the time and the caller's file and line. Port of
    /// `Console.log(*objects, sep=…, end=…, style=…, justify=…, emoji=…,
    /// markup=…, highlight=…)`: the objects are joined as for `print` and
    /// laid out by upstream's `LogRender` table, with the time (blank when
    /// it repeats the last one) and `file:line` linked to the file.
    ///
    /// The caller is the code calling `log`, found with `#[track_caller]`
    /// (upstream inspects the stack). [`log_at`](Self::log_at) names it.
    #[track_caller]
    pub fn log(
        &self,
        objects: &[LogObject<'_>],
        options: &LogOptions<'_>,
    ) -> crate::errors::Result<()> {
        let caller = std::panic::Location::caller();
        self.log_at(objects, options, caller.file(), caller.line())
    }

    /// [`log`](Self::log) with the caller's `file` and `line` given:
    /// upstream's `_caller_frame_info`. The path column shows the file's
    /// name, linked to its absolute path unless it starts with `<`.
    pub fn log_at(
        &self,
        objects: &[LogObject<'_>],
        options: &LogOptions<'_>,
        file: &str,
        line: u32,
    ) -> crate::errors::Result<()> {
        let mut styles = Vec::new();
        if let Some(style) = &self.style {
            styles.push(self.get_style(style)?);
        }
        if let Some(style) = &options.style {
            styles.push(self.get_style(style)?);
        }
        styles.retain(|style| !style.is_null());
        let align = match options.justify {
            Some(Justify::Left) => Some(crate::align::HorizontalAlign::Left),
            Some(Justify::Center) => Some(crate::align::HorizontalAlign::Center),
            Some(Justify::Right) => Some(crate::align::HorizontalAlign::Right),
            _ => None,
        };
        let item = |renderable: std::sync::Arc<dyn Renderable + Send + Sync>,
                    end: Option<String>|
         -> std::sync::Arc<dyn Renderable + Send + Sync> {
            std::sync::Arc::new(LogItem {
                renderable,
                align,
                end,
                styles: styles.clone(),
            })
        };
        // Upstream's lines end with a break where this port's are joined by
        // one (`Renderables`), so the end loses one trailing break.
        let end = options
            .end
            .strip_suffix('\n')
            .unwrap_or(options.end)
            .to_string();
        let mut renderables: Vec<std::sync::Arc<dyn Renderable + Send + Sync>> = Vec::new();
        if objects.is_empty() {
            // `objects = (NewLine(),)`: one blank line.
            renderables.push(item(std::sync::Arc::new(Text::new("")), None));
        }
        let mut texts: Vec<Text> = Vec::new();
        let flush =
            |texts: &mut Vec<Text>,
             renderables: &mut Vec<std::sync::Arc<dyn Renderable + Send + Sync>>| {
                if !texts.is_empty() {
                    let mut separator = Text::new(options.sep);
                    separator.set_justify_option(options.justify);
                    let joined = separator.join(texts);
                    renderables.push(item(std::sync::Arc::new(joined), Some(end.clone())));
                    texts.clear();
                }
            };
        for object in objects {
            match object {
                LogObject::Str(content) => {
                    let render_str = RenderStrOptions {
                        emoji: options.emoji,
                        markup: options.markup,
                        highlight: options.highlight,
                        ..Default::default()
                    };
                    texts.push(self.render_str_with(content, &render_str)?);
                }
                LogObject::Text(text) => texts.push((*text).clone()),
                LogObject::Renderable(renderable) => {
                    flush(&mut texts, &mut renderables);
                    renderables.push(item(renderable.clone(), None));
                }
            }
        }
        flush(&mut texts, &mut renderables);

        let time = self.log_time.then(|| {
            let now = (self.get_datetime)();
            match &self.log_time_format {
                LogTimeFormat::Strftime(format) => {
                    Text::new(crate::log_render::strftime(now, format))
                }
                LogTimeFormat::Callable(format) => format(now),
            }
        });
        // `path = filename.rpartition(os.sep)[-1]`, and the link is the
        // absolute path unless the name is a pseudo-file such as `<stdin>`.
        let path = file
            .rsplit(std::path::MAIN_SEPARATOR)
            .next()
            .unwrap_or(file);
        let link_path = (!file.starts_with('<')).then(|| absolute_path(file));
        let table = self
            .log_render
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .render_renderables(
                self,
                renderables,
                time,
                Text::new(""),
                Some(path),
                Some(line),
                link_path.as_deref(),
            );
        let mut segments = self.render(&table, None);
        if !segments.is_empty() {
            segments.push(Segment::line());
        }
        let segments = self
            .process_render_hooks(vec![segments], &self.options())
            .concat();
        let segments = Segment::split_and_crop_lines(&segments, self.width(), None, false, true)
            .into_iter()
            .flatten()
            .collect();
        self.emit_end(segments, false);
        Ok(())
    }

    /// Print `objects` as upstream's `Console.print(*objects, sep=…, end=…,
    /// style=…, justify=…, overflow=…, no_wrap=…, emoji=…, markup=…,
    /// highlight=…, width=…, height=…, crop=…, soft_wrap=…,
    /// new_line_start=…)` does. Strings and texts in a row join into one
    /// `Text` with `sep` and end with `end`; other renderables stand alone.
    /// Malformed markup is an error, as upstream's `MarkupError`.
    pub fn print_objects(
        &self,
        objects: &[PrintObject<'_>],
        options: &PrintOptions<'_>,
    ) -> crate::errors::Result<()> {
        let segments = self.render_objects(objects, options)?;
        self.emit_end(segments, false);
        Ok(())
    }

    /// The segments [`print_objects`](Self::print_objects) writes, ending
    /// as upstream's do (a trailing newline included).
    pub fn render_objects(
        &self,
        objects: &[PrintObject<'_>],
        options: &PrintOptions<'_>,
    ) -> crate::errors::Result<Vec<Segment>> {
        let (mut no_wrap, mut overflow, mut crop) =
            (options.no_wrap, options.overflow, options.crop);
        // `soft_wrap=True`: no wrapping, nothing cropped. `None` takes the
        // console's default.
        if options.soft_wrap.unwrap_or(self.soft_wrap) {
            no_wrap = no_wrap.or(Some(true));
            overflow = overflow.or(Some(Overflow::Ignore));
            crop = false;
        }
        let align = match options.justify {
            Some(Justify::Left) => Some(crate::align::HorizontalAlign::Left),
            Some(Justify::Center) => Some(crate::align::HorizontalAlign::Center),
            Some(Justify::Right) => Some(crate::align::HorizontalAlign::Right),
            _ => None,
        };
        let mut render_options = self.options();
        if let Some(justify) = options.justify {
            render_options.justify = justify;
        }
        render_options.overflow = overflow;
        render_options.no_wrap = no_wrap;
        render_options.markup = options.markup;
        render_options.highlight = options.highlight;
        if let Some(width) = options.width {
            render_options = render_options.update_width(width.min(self.width()));
        }
        if options.height.is_some() {
            render_options.height = options.height;
        }
        let style = match &options.style {
            Some(style) => Some(self.get_style(style)?),
            None => None,
        };
        // `Console(style=…)`: `_collect_renderables` wraps every renderable
        // in `Styled`, its line ends included.
        let console_style = match &self.style {
            Some(style) => Some(self.get_style(style)?).filter(|style| !style.is_null()),
            None => None,
        };

        // Each renderable `_collect_renderables` gives, rendered with its end.
        let mut items: Vec<Vec<Segment>> = Vec::new();
        let mut emit = |renderable: &dyn Renderable, end: Option<&str>| {
            let mut rendered = if render_options.max_width < 1 {
                Vec::new()
            } else {
                match align {
                    Some(align) => {
                        crate::align::Align::render_child(renderable, align, self, &render_options)
                    }
                    None => renderable.rich_render(self, &render_options),
                }
            };
            // This port's renderables separate their lines; upstream's end
            // each one, a text with its `end`.
            match end {
                Some(end) if !end.is_empty() => rendered.push(Segment::new(end, None)),
                Some(_) => {}
                // A renderable that ends without a line break upstream
                // (`Screen`, `LiveRender`, a rule with an open `end`) keeps
                // its last line open, as `print` does.
                None if !rendered.is_empty()
                    && Self::print_ends_line(renderable, &render_options) =>
                {
                    rendered.push(Segment::line())
                }
                None => {}
            }
            if let Some(console_style) = &console_style {
                rendered = Segment::apply_style_with(&rendered, Some(console_style), None);
            }
            items.push(rendered);
        };

        // An empty print is a new line (`NewLine()`); with another `end`, an
        // empty string.
        let blank = [PrintObject::Str("")];
        let objects = if objects.is_empty() && options.end != "\n" {
            &blank[..]
        } else {
            objects
        };
        if objects.is_empty() {
            emit(&Text::new(""), Some("\n"));
        }
        let mut texts: Vec<Text> = Vec::new();
        let flush = |texts: &mut Vec<Text>, emit: &mut dyn FnMut(&dyn Renderable, Option<&str>)| {
            if !texts.is_empty() {
                let mut separator = Text::new(options.sep);
                separator.set_justify_option(options.justify);
                emit(&separator.join(texts), Some(options.end));
                texts.clear();
            }
        };
        for object in objects {
            match object {
                PrintObject::Str(content) => {
                    let render_str = RenderStrOptions {
                        emoji: options.emoji,
                        markup: options.markup,
                        highlight: options.highlight,
                        ..Default::default()
                    };
                    texts.push(self.render_str_with(content, &render_str)?);
                }
                PrintObject::Text(text) => texts.push((*text).clone()),
                PrintObject::Renderable(renderable) => {
                    flush(&mut texts, &mut emit);
                    emit(*renderable, None);
                }
            }
        }
        flush(&mut texts, &mut emit);

        let mut segments: Vec<Segment> = Vec::new();
        for mut rendered in self.process_render_hooks(items, &render_options) {
            // The style goes on each line, never on the newlines.
            if let Some(style) = &style {
                let mut styled = Vec::with_capacity(rendered.len());
                for (line, terminated) in Segment::split_lines_terminator(&rendered) {
                    styled.extend(Segment::apply_style_with(&line, Some(style), None));
                    if terminated {
                        styled.push(Segment::line());
                    }
                }
                rendered = styled;
            }
            segments.extend(rendered);
        }

        if options.new_line_start {
            let plain: String = segments
                .iter()
                .map(|segment| segment.text.as_str())
                .collect();
            if plain.lines().count() > 1 {
                segments.insert(0, Segment::line());
            }
        }
        if crop {
            segments = Segment::split_and_crop_lines(&segments, self.width(), None, false, true)
                .into_iter()
                .flatten()
                .collect();
        }
        Ok(segments)
    }

    /// Write `count` blank lines. Port of `Console.line`.
    pub fn line(&self, count: usize) {
        if count > 0 {
            self.emit_end(vec![Segment::new("\n".repeat(count), None)], false);
        }
    }

    /// Draw a rule with an optional title. Port of `Console.rule(title,
    /// characters=…, style=…, align=…)`.
    pub fn rule(
        &self,
        title: &str,
        characters: &str,
        style: impl Into<crate::style::StyleType>,
        align: crate::align::HorizontalAlign,
    ) {
        let rule = crate::rule::Rule::new(title)
            .characters(characters)
            .style(style)
            .align(align);
        self.print(&rule);
    }

    /// Write `objects` joined by `sep` with no markup, emoji, wrapping or
    /// cropping. Port of `Console.out`.
    pub fn out(
        &self,
        objects: &[&str],
        sep: &str,
        end: &str,
        style: Option<crate::style::StyleType>,
        highlight: Option<bool>,
    ) -> crate::errors::Result<()> {
        let raw = objects.join(sep);
        self.print_objects(
            &[PrintObject::Str(&raw)],
            &PrintOptions {
                end,
                style,
                highlight,
                emoji: Some(false),
                markup: Some(false),
                no_wrap: Some(true),
                overflow: Some(Overflow::Ignore),
                crop: false,
                ..Default::default()
            },
        )
    }

    /// Pretty print a JSON string. Port of `Console.print_json`: a
    /// [`Json`](crate::json::Json) printed with `soft_wrap=True`.
    pub fn print_json(
        &self,
        json: &str,
        options: &crate::json::JsonOptions,
    ) -> crate::errors::Result<()> {
        let renderable = crate::json::Json::with_options(json, options)?;
        self.print_objects(
            &[PrintObject::Renderable(&renderable)],
            &PrintOptions {
                soft_wrap: Some(true),
                ..Default::default()
            },
        )
    }

    /// The width `renderable` needs under `options` (the console's when
    /// `None`). Port of `Console.measure`.
    pub fn measure(
        &self,
        renderable: &dyn Renderable,
        options: Option<&ConsoleOptions>,
    ) -> crate::measure::Measurement {
        let default_options;
        let options = match options {
            Some(options) => options,
            None => {
                default_options = self.options();
                &default_options
            }
        };
        crate::measure::Measurement::get(self, options, renderable)
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
        #[cfg(feature = "markdown")]
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

        // Links go unless `links` is set too (rich 15.0.0's
        // `console.pager(styles=True, links=…)`, less the random `id=`).
        let linked = |links: bool| {
            let recorder = Recorder::default();
            console
                .page_with_links(&recorder, true, links, |c| {
                    c.print_str("[bold link=https://x.io]hi[/] there")
                })
                .unwrap();
            let out = recorder.0.lock().unwrap().clone();
            out
        };
        assert_eq!(linked(false), "\x1b[1mhi\x1b[0m there\n");
        assert_eq!(
            linked(true),
            "\x1b]8;;https://x.io\x1b\\\x1b[1mhi\x1b[0m\x1b]8;;\x1b\\ there\n"
        );
    }

    #[test]
    fn export_text_styled_renders_truecolor() {
        // rich 15.0.0, recording `[#ff8800]x[/] y`, `export_text(styles=True)`:
        // `\x1b[91mx…` on a `standard` console, truecolor with no colour.
        let console = |system| {
            Console::builder()
                .force_terminal(true)
                .color_system(system)
                .width(20)
                .no_color(false)
                .build()
        };
        let export =
            |console: Console| console.export_text_styled(|c| c.print_str("[#ff8800]x[/] y"));
        assert_eq!(
            export(console(Some(ColorSystem::Standard))),
            "\x1b[91mx\x1b[0m y\n"
        );
        assert_eq!(export(console(None)), "\x1b[38;2;255;136;0mx\x1b[0m y\n");
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

    /// Upstream's `render_lines` pads with its `style` argument as given
    /// (default `None`), so the pad segments carry no style.
    #[test]
    fn render_lines_pads_with_the_given_style() {
        let console = test_console();
        let options = console.options().update_width(5).update_height(2);
        let lines = console.render_lines(&Text::new("hi"), &options, true);
        assert_eq!(lines.len(), 2);
        let width_pad = lines[0].last().unwrap();
        assert_eq!((width_pad.text.as_str(), &width_pad.style), ("   ", &None));
        assert_eq!(lines[1], vec![Segment::new("     ", None)]);

        let red = Style::parse("red").unwrap();
        let lines = console.render_lines_styled(&Text::new("hi"), &options, Some(&red), true);
        assert_eq!(lines[0].last().unwrap().style, Some(red.clone()));
        assert_eq!(lines[1], vec![Segment::new("     ", Some(red))]);
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
    fn print_objects_keeps_a_renderable_s_open_last_line() {
        // A `Screen` ends without a line break when printed
        // (`Renderable::print_end_line`); `print_objects` must not add one.
        let console = Console::builder()
            .force_terminal(true)
            .color_system(Some(crate::color::ColorSystem::Truecolor))
            .width(4)
            .height(2)
            .build();
        let screen = crate::screen::Screen::new(Box::new(Text::new("hi")));
        let printed = console.capture(|c| c.print(&screen));
        let objects = console.capture(|c| {
            c.print_objects(
                &[PrintObject::Renderable(&screen)],
                &PrintOptions::default(),
            )
            .unwrap()
        });
        assert_eq!(objects, printed);
        assert!(!objects.ends_with('\n'), "{objects:?}");
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
