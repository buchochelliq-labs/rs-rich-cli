//! Download and upload progress: byte counts with a smoothed rate, an ETA,
//! retries and cancellation.
//!
//! A [`Transfer`] is the state of one transfer: its name, [`Direction`],
//! optional total, bytes done, attempt count and [`TransferState`]. It is also
//! a renderable one-line summary:
//!
//! ```text
//! ↓ release.tar.gz  ━━━━━━━━━━━╺━━━━━━━━━━━━━━  12.3/45.6 MB  2.4 MB/s  ETA 0:00:14
//! ```
//!
//! [`Transfers`] renders several, with their columns aligned and an optional
//! totals line. [`TransferReader`] and [`TransferWriter`] count bytes into a
//! shared transfer as they pass and stop with a [`cancelled`] error when a
//! [`CancelToken`] is cancelled. To drive the core [`Progress`] display
//! instead, use [`transfer_columns`] and [`Transfer::task_update`].
//!
//! Time is never read here behind your back: every update takes `now`, a
//! [`Duration`] since an origin of your choosing (usually
//! `start.elapsed()` of one `Instant`), so tests can use fixed times.
//!
//! Without colour the bar is drawn in brackets (`[#####.....]`) and every
//! state is spelled out, so nothing depends on colour.
//!
//! ```
//! use std::time::Duration;
//! use rich::Console;
//! use rich_ext::transfer::{Direction, Transfer};
//!
//! let secs = Duration::from_secs;
//! let mut t = Transfer::new("data.bin", Direction::Download).total(10_000_000);
//! t.advance(2_000_000, secs(1));
//! t.advance(2_000_000, secs(2));
//! assert_eq!(t.rate(), Some(2_000_000.0));
//! assert_eq!(t.eta(), Some(secs(3)));
//!
//! let console = Console::builder().width(60).build();
//! assert_eq!(
//!     console.render_to_string(&t),
//!     "↓ data.bin  [####......]  4.0/10.0 MB  2.0 MB/s  ETA 0:00:03"
//! );
//! ```
//!
//! [`Progress`]: rich::Progress

use std::collections::VecDeque;
use std::io::{self, Read, Write};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use rich::cells::cell_len;
use rich::progress::{BarColumn, ProgressColumn, TaskUpdate};
use rich::{Console, ConsoleOptions, ProgressBar, Renderable, Segment, Style};

use crate::a11y::{Status, SymbolSet};
use crate::cancel::CancelToken;
use crate::format;
use crate::layout::{fit_segments, OverflowPolicy};

/// The default styles for transfer keys. [`extended_theme`] includes them;
/// renderers fall back to them when a theme lacks a key.
///
/// [`extended_theme`]: crate::theme::extended_theme
pub const STYLES: &[(&str, &str)] = &[
    ("transfer.direction", "cyan"),
    ("transfer.name", "bold"),
    ("transfer.size", "green"),
    ("transfer.rate", "red"),
    ("transfer.eta", "cyan"),
    ("transfer.retry", "yellow"),
    ("transfer.paused", "dim"),
    ("transfer.done", "green"),
    ("transfer.failed", "bold red"),
    ("transfer.cancelled", "dim"),
    ("transfer.summary", "bold"),
];

/// The console theme's style for `key`, else its default from `table`.
pub(crate) fn keyed_style(console: &Console, table: &[(&str, &str)], key: &str) -> Style {
    let fallback = table
        .iter()
        .find(|(name, _)| *name == key)
        .map_or("none", |(_, spec)| spec);
    crate::event::theme_style(console, key, fallback)
}

fn style(console: &Console, key: &str) -> Style {
    keyed_style(console, STYLES, key)
}

/// Whether `console` shows colour; without it renderers use plain fallbacks.
pub(crate) fn has_color(console: &Console) -> bool {
    !console.no_color() && console.color_system().is_some()
}

/// `set`, downgraded to ASCII tags on an ASCII-only console.
pub(crate) fn effective_symbols(console: &Console, set: SymbolSet) -> SymbolSet {
    if set == SymbolSet::Unicode && console.ascii_only() {
        SymbolSet::Ascii
    } else {
        set
    }
}

/// A bar `width` cells wide filled to `ratio` (0..=1), or pulsing at `time`
/// when `ratio` is `None`. With colour it is the core [`ProgressBar`] using
/// the `bar.*` styles, or `complete` for the filled part; without
/// colour it is `[###...]`, or `[------]` when the amount is unknown.
pub(crate) fn bar_segments(
    console: &Console,
    options: &ConsoleOptions,
    width: usize,
    ratio: Option<f64>,
    time: f64,
    complete: Option<Style>,
) -> Vec<Segment> {
    if !has_color(console) {
        let inner = width.saturating_sub(2);
        let body = match ratio {
            Some(ratio) => {
                let filled = ((inner as f64) * ratio.clamp(0.0, 1.0)).floor() as usize;
                format!("{}{}", "#".repeat(filled), ".".repeat(inner - filled))
            }
            None => "-".repeat(inner),
        };
        return vec![Segment::new(format!("[{body}]"), None)];
    }
    let mut bar = match ratio {
        Some(ratio) => ProgressBar::new(1000.0, (ratio.clamp(0.0, 1.0) * 1000.0).floor()),
        None => ProgressBar::indeterminate(),
    }
    .width(width)
    .animation_time(time);
    if let Some(style) = complete {
        bar = bar.complete_style(style.clone()).finished_style(style);
    }
    bar.rich_render(console, &options.update_width(width))
}

/// Which way the bytes go.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Direction {
    /// Receiving: `↓`, or `v` in ASCII.
    #[default]
    Download,
    /// Sending: `↑`, or `^` in ASCII.
    Upload,
}

impl Direction {
    /// The marker for this direction: an arrow, an ASCII caret, or a word.
    pub fn symbol(self, set: SymbolSet) -> &'static str {
        match (set, self) {
            (SymbolSet::Unicode, Direction::Download) => "↓",
            (SymbolSet::Unicode, Direction::Upload) => "↑",
            (SymbolSet::Ascii, Direction::Download) => "v",
            (SymbolSet::Ascii, Direction::Upload) => "^",
            (SymbolSet::Words, Direction::Download) => "down",
            (SymbolSet::Words, Direction::Upload) => "up",
        }
    }
}

/// Where a transfer is in its life.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum TransferState {
    /// Moving bytes (or about to).
    #[default]
    Active,
    /// Stopped for now; the rate and ETA are hidden.
    Paused,
    /// The last attempt failed and another is coming.
    Retrying,
    /// Every byte arrived.
    Done,
    /// Gave up.
    Failed,
    /// Stopped on request.
    Cancelled,
}

impl TransferState {
    /// The accessibility status that matches this state.
    pub fn status(self) -> Status {
        match self {
            TransferState::Active => Status::Info,
            TransferState::Paused => Status::Pending,
            TransferState::Retrying => Status::Warning,
            TransferState::Done => Status::Ok,
            TransferState::Failed => Status::Error,
            TransferState::Cancelled => Status::Skipped,
        }
    }

    /// Whether the transfer has ended (done, failed or cancelled).
    pub fn is_finished(self) -> bool {
        matches!(
            self,
            TransferState::Done | TransferState::Failed | TransferState::Cancelled
        )
    }

    fn marker(self, set: SymbolSet) -> (&'static str, &'static str) {
        let (unicode, ascii, word) = match self {
            TransferState::Active => ("", "", ""),
            TransferState::Paused => ("‖ paused", "[PAUSED]", "paused"),
            TransferState::Retrying => ("↻ retrying", "[RETRY]", "retrying"),
            TransferState::Done => ("✔ done", "[DONE]", "done"),
            TransferState::Failed => ("✖ failed", "[FAILED]", "failed"),
            TransferState::Cancelled => ("↷ cancelled", "[CANCELLED]", "cancelled"),
        };
        let text = match set {
            SymbolSet::Unicode => unicode,
            SymbolSet::Ascii => ascii,
            SymbolSet::Words => word,
        };
        let key = match self {
            TransferState::Active => "transfer.eta",
            TransferState::Paused => "transfer.paused",
            TransferState::Retrying => "transfer.retry",
            TransferState::Done => "transfer.done",
            TransferState::Failed => "transfer.failed",
            TransferState::Cancelled => "transfer.cancelled",
        };
        (text, key)
    }
}

/// A byte rate over a moving time window.
///
/// Samples are `(time, cumulative bytes)`. The rate is the bytes gained
/// between the oldest sample still in the window and the newest, divided by
/// the time between them, so a burst or a stall fades out after `window`.
#[derive(Clone, Debug)]
pub struct RateMeter {
    window: Duration,
    samples: VecDeque<(Duration, u64)>,
}

impl Default for RateMeter {
    fn default() -> Self {
        RateMeter::new(Duration::from_secs(5))
    }
}

impl RateMeter {
    /// A meter averaging over `window` (at least one millisecond).
    pub fn new(window: Duration) -> Self {
        RateMeter {
            window: window.max(Duration::from_millis(1)),
            samples: VecDeque::new(),
        }
    }

    /// Record that `total` bytes had arrived by `now`. A total lower than the
    /// last one, or a time earlier than the last one, starts over.
    pub fn record(&mut self, now: Duration, total: u64) {
        if self
            .samples
            .back()
            .is_some_and(|&(t, bytes)| now < t || total < bytes)
        {
            self.samples.clear();
        }
        self.samples.push_back((now, total));
        // Keep one sample at or before the window's start as the anchor.
        let start = now.saturating_sub(self.window);
        while self.samples.len() > 2 && self.samples[1].0 <= start {
            self.samples.pop_front();
        }
    }

    /// Bytes per second over the window, or `None` before two samples at
    /// different times.
    pub fn rate(&self) -> Option<f64> {
        let (&(t0, b0), &(t1, b1)) = (self.samples.front()?, self.samples.back()?);
        let span = t1.checked_sub(t0)?.as_secs_f64();
        (span > 0.0).then(|| (b1 - b0) as f64 / span)
    }

    /// Forget every sample.
    pub fn reset(&mut self) {
        self.samples.clear();
    }
}

/// One download or upload. See the [module docs](self).
#[derive(Clone, Debug)]
pub struct Transfer {
    name: String,
    direction: Direction,
    total: Option<u64>,
    completed: u64,
    attempt: u32,
    max_attempts: Option<u32>,
    state: TransferState,
    error: Option<String>,
    meter: RateMeter,
    now: Duration,
    bar_width: usize,
    symbols: SymbolSet,
}

impl Transfer {
    /// A new active transfer on its first attempt, total unknown.
    pub fn new(name: impl Into<String>, direction: Direction) -> Self {
        Transfer {
            name: name.into(),
            direction,
            total: None,
            completed: 0,
            attempt: 1,
            max_attempts: None,
            state: TransferState::Active,
            error: None,
            meter: RateMeter::default(),
            now: Duration::ZERO,
            bar_width: 24,
            symbols: SymbolSet::Unicode,
        }
    }

    /// A download named `name`.
    pub fn download(name: impl Into<String>) -> Self {
        Transfer::new(name, Direction::Download)
    }

    /// An upload named `name`.
    pub fn upload(name: impl Into<String>) -> Self {
        Transfer::new(name, Direction::Upload)
    }

    /// Set the total size in bytes.
    pub fn total(mut self, bytes: u64) -> Self {
        self.total = Some(bytes);
        self
    }

    /// Show the attempt as `n/max` and stop counting at `max`.
    pub fn max_attempts(mut self, max: u32) -> Self {
        self.max_attempts = Some(max.max(1));
        self
    }

    /// Average the rate over `window` (default 5 seconds).
    pub fn rate_window(mut self, window: Duration) -> Self {
        self.meter = RateMeter::new(window);
        self
    }

    /// The widest the bar may grow (default 24 cells); it shrinks to fit.
    pub fn bar_width(mut self, width: usize) -> Self {
        self.bar_width = width;
        self
    }

    /// How the direction and state are marked (default Unicode; an ASCII-only
    /// console uses ASCII tags regardless).
    pub fn symbols(mut self, set: SymbolSet) -> Self {
        self.symbols = set;
        self
    }

    /// Set or change the total size (a server may only say once headers arrive).
    pub fn set_total(&mut self, bytes: Option<u64>) {
        self.total = bytes;
    }

    /// Count `bytes` more as arrived at `now`. A paused or retrying transfer
    /// becomes active again; a finished one ignores the call.
    pub fn advance(&mut self, bytes: u64, now: Duration) {
        self.set_completed(self.completed.saturating_add(bytes), now);
    }

    /// Set the bytes done so far at `now`. Going backwards (a restart from
    /// zero) resets the rate.
    pub fn set_completed(&mut self, bytes: u64, now: Duration) {
        if self.state.is_finished() {
            return;
        }
        if matches!(self.state, TransferState::Paused | TransferState::Retrying) {
            self.state = TransferState::Active;
        }
        self.completed = bytes;
        self.now = now;
        self.meter.record(now, bytes);
    }

    /// Mark the transfer started at `now`: its rate is measured from here.
    pub fn start(&mut self, now: Duration) {
        self.set_completed(self.completed, now);
    }

    /// Pause at `now`: the rate is forgotten, so it restarts cleanly.
    pub fn pause(&mut self, now: Duration) {
        if !self.state.is_finished() {
            self.state = TransferState::Paused;
            self.now = now;
            self.meter.reset();
        }
    }

    /// The current attempt failed with `reason`. If attempts remain, the
    /// transfer is retrying on the next attempt and `true` is returned (it
    /// resumes from the bytes it has; call [`set_completed`](Self::set_completed)
    /// with 0 to restart). Otherwise it has failed and `false` is returned.
    pub fn retry(&mut self, reason: impl Into<String>) -> bool {
        if self.state.is_finished() {
            return false;
        }
        self.error = Some(reason.into());
        self.meter.reset();
        if self.max_attempts.is_some_and(|max| self.attempt >= max) {
            self.state = TransferState::Failed;
            return false;
        }
        self.attempt += 1;
        self.state = TransferState::Retrying;
        true
    }

    /// Mark the transfer complete at `now`; a known total counts as fully
    /// transferred.
    pub fn finish(&mut self, now: Duration) {
        if let Some(total) = self.total {
            self.set_completed(total, now);
        } else {
            self.now = now;
        }
        if !self.state.is_finished() {
            self.state = TransferState::Done;
        }
    }

    /// Mark the transfer failed with `reason`.
    pub fn fail(&mut self, reason: impl Into<String>) {
        if !self.state.is_finished() {
            self.error = Some(reason.into());
            self.state = TransferState::Failed;
        }
    }

    /// Mark the transfer cancelled.
    pub fn cancel(&mut self) {
        if !self.state.is_finished() {
            self.state = TransferState::Cancelled;
        }
    }

    /// The name shown.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Download or upload.
    pub fn direction(&self) -> Direction {
        self.direction
    }

    /// Total bytes, if known.
    pub fn total_bytes(&self) -> Option<u64> {
        self.total
    }

    /// Bytes done so far.
    pub fn completed(&self) -> u64 {
        self.completed
    }

    /// The attempt in progress, from 1.
    pub fn attempt(&self) -> u32 {
        self.attempt
    }

    /// The state.
    pub fn state(&self) -> TransferState {
        self.state
    }

    /// Why the last attempt failed, if one did.
    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    /// Done as a fraction of the total, if the total is known and non-zero.
    pub fn fraction(&self) -> Option<f64> {
        match self.total {
            Some(0) => Some(1.0),
            Some(total) => Some((self.completed as f64 / total as f64).min(1.0)),
            None => None,
        }
    }

    /// The smoothed rate in bytes per second, while active.
    pub fn rate(&self) -> Option<f64> {
        if self.state == TransferState::Active {
            self.meter.rate()
        } else {
            None
        }
    }

    /// The estimated time left, when the total and the rate are known.
    pub fn eta(&self) -> Option<Duration> {
        let rate = self.rate().filter(|r| *r > 0.0)?;
        let left = self.total?.saturating_sub(self.completed);
        Some(Duration::from_secs((left as f64 / rate).ceil() as u64))
    }

    /// A [`TaskUpdate`] mirroring this transfer onto a core
    /// [`Progress`](rich::Progress) task: total, completed, and the name with
    /// any state marker as the description. Pair with [`transfer_columns`].
    pub fn task_update(&self) -> TaskUpdate {
        let mut update = TaskUpdate::default().completed(self.completed as f64);
        if let Some(total) = self.total {
            update = update.total(total as f64);
        }
        let (marker, _) = self.state.marker(SymbolSet::Words);
        let description = match (self.state, marker) {
            (TransferState::Retrying, _) => {
                format!("{} (retry {})", self.name, self.attempt_label())
            }
            (_, "") => self.name.clone(),
            (_, marker) => format!("{} ({marker})", self.name),
        };
        update.description(rich::markup::escape(&description))
    }

    fn attempt_label(&self) -> String {
        match self.max_attempts {
            Some(max) => format!("{}/{max}", self.attempt),
            None => self.attempt.to_string(),
        }
    }

    fn sizes(&self) -> String {
        match self.total {
            Some(total) => {
                const UNITS: &[&str] = &["bytes", "kB", "MB", "GB", "TB", "PB", "EB", "ZB", "YB"];
                let (unit, suffix) = rich::filesize::pick_unit_and_suffix(total, UNITS, 1000);
                let scale = |n: u64| {
                    if unit == 1 {
                        format::number(n.min(i64::MAX as u64) as i64)
                    } else {
                        format!("{:.1}", n as f64 / unit as f64)
                    }
                };
                format!("{}/{} {suffix}", scale(self.completed), scale(total))
            }
            None => format::bytes(self.completed),
        }
    }

    fn tail(&self, set: SymbolSet) -> (String, &'static str) {
        let (marker, key) = self.state.marker(set);
        match self.state {
            TransferState::Active => {
                let eta = self.eta().map_or("-:--:--".to_string(), format::clock);
                let mut tail = format!("ETA {eta}");
                if self.attempt > 1 {
                    tail.push_str(&format!(" (attempt {})", self.attempt_label()));
                }
                (tail, key)
            }
            TransferState::Retrying => (format!("{marker} {}", self.attempt_label()), key),
            TransferState::Failed => match &self.error {
                Some(error) => (format!("{marker}: {error}"), key),
                None => (marker.to_string(), key),
            },
            _ => (marker.to_string(), key),
        }
    }

    fn cells(&self, set: SymbolSet) -> Cells {
        let (tail, tail_key) = self.tail(set);
        Cells {
            arrow: self.direction.symbol(set),
            sizes: self.sizes(),
            rate: self.rate().map_or("-".to_string(), format::rate),
            tail,
            tail_key,
        }
    }

    fn render_line(
        &self,
        console: &Console,
        options: &ConsoleOptions,
        widths: Widths,
        set: SymbolSet,
    ) -> Vec<Segment> {
        let cells = self.cells(set);
        let pad = |text: &str, width: usize, right: bool| {
            let fill = " ".repeat(width.saturating_sub(cell_len(text)));
            if right {
                format!("{fill}{text}")
            } else {
                format!("{text}{fill}")
            }
        };
        let name = pad(&self.name, widths.name, false);
        let sizes = pad(&cells.sizes, widths.sizes, true);
        let rate = pad(&cells.rate, widths.rate, true);
        let fixed = cell_len(cells.arrow)
            + 1
            + cell_len(&name)
            + 2
            + cell_len(&sizes)
            + 2
            + cell_len(&rate)
            + 2
            + widths.tail.max(cell_len(&cells.tail));
        let room = options.max_width.saturating_sub(fixed + 2);
        let bar_width = self.bar_width.min(room);

        let mut line = vec![
            Segment::new(cells.arrow, Some(style(console, "transfer.direction"))),
            Segment::new(" ", None),
            Segment::new(name, Some(style(console, "transfer.name"))),
            Segment::new("  ", None),
        ];
        if bar_width >= 8 {
            let ratio = match self.state {
                TransferState::Done => Some(1.0),
                _ => self.fraction(),
            };
            let complete = match self.state {
                TransferState::Failed => Some("transfer.failed"),
                TransferState::Paused | TransferState::Cancelled => Some("transfer.paused"),
                TransferState::Retrying => Some("transfer.retry"),
                _ => None,
            }
            .map(|key| style(console, key));
            line.extend(bar_segments(
                console,
                options,
                bar_width,
                ratio,
                self.now.as_secs_f64(),
                complete,
            ));
            line.push(Segment::new("  ", None));
        }
        line.extend([
            Segment::new(sizes, Some(style(console, "transfer.size"))),
            Segment::new("  ", None),
            Segment::new(rate, Some(style(console, "transfer.rate"))),
            Segment::new("  ", None),
            Segment::new(cells.tail, Some(style(console, cells.tail_key))),
        ]);
        finish_line(line, options.max_width)
    }
}

/// Crop `line` to `width` cells. Like the core's renderables, a line has no
/// trailing newline; [`join_lines`] puts them between lines.
pub(crate) fn finish_line(line: Vec<Segment>, width: usize) -> Vec<Segment> {
    fit_segments(&line, width.max(1), OverflowPolicy::Crop)
        .into_iter()
        .next()
        .unwrap_or_default()
}

/// `lines` with a newline between each (none after the last).
pub(crate) fn join_lines(lines: impl IntoIterator<Item = Vec<Segment>>) -> Vec<Segment> {
    let mut out = Vec::new();
    for (i, line) in lines.into_iter().enumerate() {
        if i > 0 {
            out.push(Segment::line());
        }
        out.extend(line);
    }
    out
}

struct Cells {
    arrow: &'static str,
    sizes: String,
    rate: String,
    tail: String,
    tail_key: &'static str,
}

#[derive(Clone, Copy, Default)]
struct Widths {
    name: usize,
    sizes: usize,
    rate: usize,
    tail: usize,
}

impl Widths {
    fn of<'a>(transfers: impl IntoIterator<Item = &'a Transfer>, set: SymbolSet) -> Self {
        let mut widths = Widths::default();
        for transfer in transfers {
            let cells = transfer.cells(set);
            widths.name = widths.name.max(cell_len(&transfer.name));
            widths.sizes = widths.sizes.max(cell_len(&cells.sizes));
            widths.rate = widths.rate.max(cell_len(&cells.rate));
            widths.tail = widths.tail.max(cell_len(&cells.tail));
        }
        widths
    }
}

impl Renderable for Transfer {
    fn rich_render(&self, console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
        let set = effective_symbols(console, self.symbols);
        self.render_line(console, options, Widths::of([self], set), set)
    }
}

/// Several transfers, one line each with aligned columns, and an optional
/// totals line.
///
/// ```
/// use std::time::Duration;
/// use rich::Console;
/// use rich_ext::transfer::{Transfer, Transfers};
///
/// let mut group = Transfers::new().summary(true);
/// group.push(Transfer::download("a.iso").total(4_000));
/// let b = group.push(Transfer::upload("notes.txt").total(1_000));
/// group[b].finish(Duration::from_secs(1));
/// let out = Console::builder().width(56).build().render_to_string(&group);
/// assert_eq!(
///     out,
///     concat!(
///         "↓ a.iso      [.............]  0.0/4.0 kB  -  ETA -:--:--\n",
///         "↑ notes.txt  [#############]  1.0/1.0 kB  -  ✔ done\n",
///         "2 transfers, 1 done  1.0/5.0 kB",
///     )
/// );
/// ```
#[derive(Clone, Debug, Default)]
pub struct Transfers {
    items: Vec<Transfer>,
    summary: bool,
    symbols: SymbolSet,
}

impl Transfers {
    /// An empty group.
    pub fn new() -> Self {
        Transfers::default()
    }

    /// Add a totals line under the transfers.
    pub fn summary(mut self, summary: bool) -> Self {
        self.summary = summary;
        self
    }

    /// How directions and states are marked, for every transfer in the group.
    pub fn symbols(mut self, set: SymbolSet) -> Self {
        self.symbols = set;
        self
    }

    /// Add a transfer; returns its index.
    pub fn push(&mut self, transfer: Transfer) -> usize {
        self.items.push(transfer);
        self.items.len() - 1
    }

    /// The transfers, in the order added.
    pub fn iter(&self) -> std::slice::Iter<'_, Transfer> {
        self.items.iter()
    }

    /// The transfers, mutably.
    pub fn iter_mut(&mut self) -> std::slice::IterMut<'_, Transfer> {
        self.items.iter_mut()
    }

    /// How many transfers there are.
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// Whether the group is empty.
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Whether every transfer has finished (vacuously true when empty).
    pub fn finished(&self) -> bool {
        self.items.iter().all(|t| t.state.is_finished())
    }

    /// The sum of the active transfers' rates.
    pub fn rate(&self) -> Option<f64> {
        let rates: Vec<f64> = self.items.iter().filter_map(Transfer::rate).collect();
        (!rates.is_empty()).then(|| rates.iter().sum())
    }

    fn summary_line(&self, console: &Console, width: usize) -> Vec<Segment> {
        let count = self.items.len();
        let mut words = vec![format!(
            "{count} transfer{}",
            if count == 1 { "" } else { "s" }
        )];
        for state in [
            TransferState::Done,
            TransferState::Retrying,
            TransferState::Failed,
            TransferState::Cancelled,
        ] {
            let n = self.items.iter().filter(|t| t.state == state).count();
            if n > 0 {
                let (word, _) = state.marker(SymbolSet::Words);
                words.push(format!("{n} {word}"));
            }
        }
        let completed: u64 = self.items.iter().map(|t| t.completed).sum();
        let totals: Option<u64> = self.items.iter().map(|t| t.total).sum();
        let mut aggregate = Transfer::download("");
        aggregate.completed = completed;
        aggregate.total = totals;
        let mut line = vec![
            Segment::new(words.join(", "), Some(style(console, "transfer.summary"))),
            Segment::new("  ", None),
            Segment::new(aggregate.sizes(), Some(style(console, "transfer.size"))),
        ];
        if let Some(rate) = self.rate() {
            line.push(Segment::new("  ", None));
            line.push(Segment::new(
                format::rate(rate),
                Some(style(console, "transfer.rate")),
            ));
        }
        finish_line(line, width)
    }
}

impl std::ops::Index<usize> for Transfers {
    type Output = Transfer;
    fn index(&self, index: usize) -> &Transfer {
        &self.items[index]
    }
}

impl std::ops::IndexMut<usize> for Transfers {
    fn index_mut(&mut self, index: usize) -> &mut Transfer {
        &mut self.items[index]
    }
}

impl Renderable for Transfers {
    fn rich_render(&self, console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
        let set = effective_symbols(console, self.symbols);
        let widths = Widths::of(&self.items, set);
        let mut lines: Vec<Vec<Segment>> = self
            .items
            .iter()
            .map(|transfer| transfer.render_line(console, options, widths, set))
            .collect();
        if self.summary && !self.items.is_empty() {
            lines.push(self.summary_line(console, options.max_width));
        }
        join_lines(lines)
    }
}

/// Columns for a core [`Progress`](rich::Progress) showing transfers:
/// description, a 30-cell bar, `done/total unit`, speed and time remaining.
///
/// ```
/// use rich::Progress;
/// use rich_ext::transfer::{transfer_columns, Transfer};
///
/// let mut progress = Progress::new().columns(transfer_columns());
/// let mut transfer = Transfer::download("data.bin").total(2_000_000);
/// let task = progress.add_task("data.bin", 2_000_000.0, 0.0);
/// transfer.advance(500_000, std::time::Duration::from_secs(1));
/// progress.update(task, transfer.task_update());
/// assert_eq!(progress.task(task).unwrap().completed(), 500_000.0);
/// ```
pub fn transfer_columns() -> Vec<ProgressColumn> {
    vec![
        ProgressColumn::Description,
        ProgressColumn::BarWith(BarColumn::new().bar_width(Some(30))),
        ProgressColumn::Download,
        ProgressColumn::TransferSpeed,
        ProgressColumn::time_remaining(),
    ]
}

/// A [`Transfer`] shared between the thread moving bytes and the one
/// drawing them.
pub type SharedTransfer = Arc<Mutex<Transfer>>;

/// A clock for the I/O wrappers: time since an origin.
pub type Clock = Arc<dyn Fn() -> Duration + Send + Sync>;

/// The error the I/O wrappers return once their token is cancelled.
///
/// Its kind is [`io::ErrorKind::Other`], not `Interrupted`: `io::copy` and
/// `read_to_end` retry `Interrupted` errors forever. Test for it with
/// [`is_cancelled`].
pub fn cancelled() -> io::Error {
    io::Error::other(Cancelled)
}

/// Whether `error` is the wrappers' [`cancelled`] error.
pub fn is_cancelled(error: &io::Error) -> bool {
    error
        .get_ref()
        .is_some_and(|inner| inner.downcast_ref::<Cancelled>().is_some())
}

/// The payload of the [`cancelled`] error.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cancelled;

impl std::fmt::Display for Cancelled {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("transfer cancelled")
    }
}

impl std::error::Error for Cancelled {}

/// The counting shared by [`TransferReader`] and [`TransferWriter`].
struct Meter {
    transfer: SharedTransfer,
    cancel: Option<CancelToken>,
    clock: Clock,
}

impl Meter {
    fn new(transfer: SharedTransfer) -> Self {
        let origin = Instant::now();
        Meter {
            transfer,
            cancel: None,
            clock: Arc::new(move || origin.elapsed()),
        }
    }

    fn lock(&self) -> MutexGuard<'_, Transfer> {
        self.transfer.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn check(&self) -> io::Result<()> {
        if self.cancel.as_ref().is_some_and(CancelToken::is_cancelled) {
            self.lock().cancel();
            return Err(cancelled());
        }
        Ok(())
    }

    fn count(&self, result: io::Result<usize>, eof_finishes: bool) -> io::Result<usize> {
        let now = (self.clock)();
        let mut transfer = self.lock();
        match &result {
            Ok(0) if eof_finishes => transfer.finish(now),
            Ok(n) => transfer.advance(*n as u64, now),
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) => transfer.fail(e.to_string()),
        }
        result
    }
}

/// A [`Read`] that counts bytes into a [`SharedTransfer`], marks it done at
/// end of file or failed on an error, and returns [`cancelled`] once its
/// token is cancelled.
///
/// ```
/// use std::io::Read;
/// use std::sync::{Arc, Mutex};
/// use rich_ext::cancel::CancelToken;
/// use rich_ext::transfer::{is_cancelled, Transfer, TransferReader, TransferState};
///
/// let shared = Arc::new(Mutex::new(Transfer::download("blob").total(5)));
/// let token = CancelToken::new();
/// let mut reader = TransferReader::new(&b"hello"[..], shared.clone()).cancel(token.clone());
/// let mut out = Vec::new();
/// reader.read_to_end(&mut out).unwrap();
/// assert_eq!(shared.lock().unwrap().state(), TransferState::Done);
///
/// let shared = Arc::new(Mutex::new(Transfer::download("blob")));
/// let mut reader = TransferReader::new(&b"hello"[..], shared.clone()).cancel(token.clone());
/// token.cancel();
/// assert!(is_cancelled(&reader.read(&mut [0; 4]).unwrap_err()));
/// assert_eq!(shared.lock().unwrap().state(), TransferState::Cancelled);
/// ```
pub struct TransferReader<R> {
    inner: R,
    meter: Meter,
}

impl<R> TransferReader<R> {
    /// Wrap `inner`, counting into `transfer` on a clock started now.
    pub fn new(inner: R, transfer: SharedTransfer) -> Self {
        TransferReader {
            inner,
            meter: Meter::new(transfer),
        }
    }

    /// Stop with [`cancelled`] once `token` is cancelled.
    pub fn cancel(mut self, token: CancelToken) -> Self {
        self.meter.cancel = Some(token);
        self
    }

    /// Read times from `clock` instead of a real clock.
    pub fn clock(mut self, clock: Clock) -> Self {
        self.meter.clock = clock;
        self
    }

    /// The shared transfer.
    pub fn transfer(&self) -> &SharedTransfer {
        &self.meter.transfer
    }

    /// The wrapped reader.
    pub fn into_inner(self) -> R {
        self.inner
    }
}

impl<R: Read> Read for TransferReader<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.meter.check()?;
        let result = self.inner.read(buf);
        // A zero read into an empty buffer is not end of file.
        self.meter.count(result, !buf.is_empty())
    }
}

/// A [`Write`] that counts bytes into a [`SharedTransfer`], marks it failed
/// on an error, and returns [`cancelled`] once its token is cancelled. Call
/// [`Transfer::finish`] yourself when the upload is complete.
pub struct TransferWriter<W> {
    inner: W,
    meter: Meter,
}

impl<W> TransferWriter<W> {
    /// Wrap `inner`, counting into `transfer` on a clock started now.
    pub fn new(inner: W, transfer: SharedTransfer) -> Self {
        TransferWriter {
            inner,
            meter: Meter::new(transfer),
        }
    }

    /// Stop with [`cancelled`] once `token` is cancelled.
    pub fn cancel(mut self, token: CancelToken) -> Self {
        self.meter.cancel = Some(token);
        self
    }

    /// Read times from `clock` instead of a real clock.
    pub fn clock(mut self, clock: Clock) -> Self {
        self.meter.clock = clock;
        self
    }

    /// The shared transfer.
    pub fn transfer(&self) -> &SharedTransfer {
        &self.meter.transfer
    }

    /// The wrapped writer.
    pub fn into_inner(self) -> W {
        self.inner
    }
}

impl<W: Write> Write for TransferWriter<W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.meter.check()?;
        let result = self.inner.write(buf);
        self.meter.count(result, false)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.meter.check()?;
        self.inner.flush()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn meter_keeps_an_anchor_at_the_window_start() {
        let s = Duration::from_secs;
        let mut meter = RateMeter::new(s(2));
        assert_eq!(meter.rate(), None);
        meter.record(s(0), 0);
        meter.record(s(1), 100);
        meter.record(s(2), 200);
        meter.record(s(3), 1200);
        // Window [1, 3]: 1100 bytes over 2 seconds.
        assert_eq!(meter.rate(), Some(550.0));
        meter.record(s(4), 100);
        assert_eq!(meter.rate(), None, "going backwards starts over");
    }

    #[test]
    fn retry_counts_attempts_until_the_limit() {
        let mut t = Transfer::download("x").max_attempts(2);
        assert!(t.retry("reset"));
        assert_eq!((t.attempt(), t.state()), (2, TransferState::Retrying));
        t.advance(1, Duration::ZERO);
        assert_eq!(t.state(), TransferState::Active);
        assert!(!t.retry("reset again"));
        assert_eq!(t.state(), TransferState::Failed);
        assert_eq!(t.error(), Some("reset again"));
    }
}
