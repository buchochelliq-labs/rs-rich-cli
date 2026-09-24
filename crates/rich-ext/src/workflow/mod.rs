//! Workflow renderables: commands, task trees and completion summaries.
//!
//! Build, deploy and install tools share one shape: run some commands, track
//! a tree of tasks while they run, then say how it went. This module gives
//! each step a data model that knows nothing about terminals, and a view that
//! renders it:
//!
//! * [`command`] — a [`CommandRecord`] of one process run (argv, cwd, stdout
//!   and stderr interleaved, exit status, duration), filled by
//!   [`CommandRunner`] or by hand, and a [`CommandView`] with folded output,
//!   a live "running" state and a diagnostic on failure.
//! * [`tasks`] — a [`TaskTree`] of nested tasks with timing from an injected
//!   [`Clock`], aggregate status, progress and per-task cancellation.
//! * [`summary`] — a [`CompletionSummary`] for the end of a command: overall
//!   status, counts, duration, the items that need attention and next steps.
//!
//! Every view marks status with a word or tag from
//! [`a11y::SymbolSet`](crate::a11y::SymbolSet), so output reads the same
//! without colour, and running spinners become a static marker under
//! reduced motion.
//!
//! ```
//! use std::time::Duration;
//! use rich::Console;
//! use rich_ext::workflow::{CompletionSummary, ManualClock, TaskTree};
//!
//! let clock = ManualClock::new();
//! let mut tree = TaskTree::with_clock(clock.clone()).title("Build");
//! let compile = tree.add(None, "compile");
//! let test = tree.add(None, "test");
//! tree.start(compile);
//! clock.advance(Duration::from_millis(1200));
//! tree.succeed(compile).start(test);
//! clock.advance(Duration::from_millis(800));
//! tree.warn(test, "1 test ignored");
//!
//! let console = Console::builder().width(50).build();
//! assert_eq!(
//!     console.render_to_string(&tree),
//!     "Build\n├── ✔ ok compile  1.2s\n└── ⚠ warning test  800ms  1 test ignored",
//! );
//! let summary = CompletionSummary::from(&tree).next_step("review the ignored test");
//! assert!(console.render_to_string(&summary).starts_with("⚠ warning Build  2.0s\n"));
//! ```

pub mod command;
pub mod summary;
pub mod tasks;

pub use command::{CommandRecord, CommandRunner, CommandStatus, CommandView, OutputLine, Stream};
pub use summary::{CompletionSummary, SummaryItem};
pub use tasks::{TaskId, TaskTree, TaskTreeView};

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use rich::{Console, Style, StyleType, Text};

use crate::a11y::{AccessibilityPolicy, Status, SymbolSet};

/// Theme keys the workflow views use, as `(name, style)`; chained into
/// [`extended_theme`](crate::theme::extended_theme). Each view falls back to
/// these styles when a console's theme lacks the key.
pub const STYLES: &[(&str, &str)] = &[
    ("workflow.prompt", "dim"),
    ("workflow.command", "bold"),
    ("workflow.cwd", "dim"),
    ("workflow.stdout", "none"),
    ("workflow.stderr", "yellow"),
    ("workflow.gutter", "dim"),
    ("workflow.hidden", "dim italic"),
    ("workflow.duration", "dim"),
    ("workflow.guide", "dim"),
    ("workflow.status.pending", "dim"),
    ("workflow.status.running", "bold cyan"),
    ("workflow.status.ok", "bold green"),
    ("workflow.status.warning", "bold yellow"),
    ("workflow.status.error", "bold red"),
    ("workflow.status.skipped", "dim"),
    ("workflow.status.cancelled", "bold magenta"),
    ("workflow.task.label", "none"),
    ("workflow.task.running", "bold"),
    ("workflow.task.progress", "cyan"),
    ("workflow.task.note", "italic"),
    ("workflow.summary.title", "bold"),
    ("workflow.summary.counts", "none"),
    ("workflow.summary.next", "bold"),
];

/// Where a command, task or whole workflow stands.
///
/// Ordered by how much attention a state needs, most first, so the worst of
/// several is their minimum.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum State {
    /// Failed.
    Failed,
    /// Stopped before it finished.
    Cancelled,
    /// Finished, with something to look at.
    Warning,
    /// Started and not finished.
    Running,
    /// Not started.
    Pending,
    /// Finished successfully.
    Succeeded,
    /// Deliberately not run.
    Skipped,
}

impl State {
    /// Every state, in [`Ord`] order.
    pub const ALL: [State; 7] = [
        State::Failed,
        State::Cancelled,
        State::Warning,
        State::Running,
        State::Pending,
        State::Succeeded,
        State::Skipped,
    ];

    /// Whether the state is final.
    pub fn is_finished(self) -> bool {
        !matches!(self, State::Running | State::Pending)
    }

    /// Whether the state is a failure or cancellation.
    pub fn is_problem(self) -> bool {
        matches!(self, State::Failed | State::Cancelled)
    }

    /// The accessibility [`Status`] this state maps to; `None` for running
    /// and cancelled, which that set does not have.
    pub fn status(self) -> Option<Status> {
        match self {
            State::Failed => Some(Status::Error),
            State::Warning => Some(Status::Warning),
            State::Pending => Some(Status::Pending),
            State::Succeeded => Some(Status::Ok),
            State::Skipped => Some(Status::Skipped),
            State::Running | State::Cancelled => None,
        }
    }

    /// The word shown in markers and style keys: `ok`, `error`, `running`, …
    pub fn word(self) -> &'static str {
        match self.status() {
            Some(status) => status.word(),
            None if self == State::Running => "running",
            None => "cancelled",
        }
    }

    /// The static marker in `set`, as [`Status::symbol`] with `running` and
    /// `cancelled` added: `▶ running` / `[RUN]` / `running:` and
    /// `⊘ cancelled` / `[CANCEL]` / `cancelled:`.
    pub fn marker(self, set: SymbolSet) -> &'static str {
        if let Some(status) = self.status() {
            return status.symbol(set);
        }
        match (self, set) {
            (State::Running, SymbolSet::Unicode) => "▶ running",
            (State::Running, SymbolSet::Ascii) => "[RUN]",
            (State::Running, SymbolSet::Words) => "running:",
            (_, SymbolSet::Unicode) => "⊘ cancelled",
            (_, SymbolSet::Ascii) => "[CANCEL]",
            (_, SymbolSet::Words) => "cancelled:",
        }
    }

    /// The theme key for this state: `workflow.status.<word>`.
    pub fn style_key(self) -> &'static str {
        match self {
            State::Failed => "workflow.status.error",
            State::Cancelled => "workflow.status.cancelled",
            State::Warning => "workflow.status.warning",
            State::Running => "workflow.status.running",
            State::Pending => "workflow.status.pending",
            State::Succeeded => "workflow.status.ok",
            State::Skipped => "workflow.status.skipped",
        }
    }

    /// `count` things in this state, in words: `3 succeeded`, `1 failed`,
    /// `2 warnings`.
    pub fn count_label(self, count: usize) -> String {
        let word = match (self, count) {
            (State::Failed, _) => "failed",
            (State::Cancelled, _) => "cancelled",
            (State::Warning, 1) => "warning",
            (State::Warning, _) => "warnings",
            (State::Running, _) => "running",
            (State::Pending, _) => "pending",
            (State::Succeeded, _) => "succeeded",
            (State::Skipped, _) => "skipped",
        };
        format!("{count} {word}")
    }
}

/// A source of "now" for task timing, as time since an arbitrary origin.
///
/// Inject a [`ManualClock`] in tests and screenshots so durations are exact;
/// [`SystemClock`] is the default.
pub trait Clock: Send + Sync {
    /// Time elapsed since this clock's origin.
    fn now(&self) -> Duration;
}

/// Wall-clock time since the clock was created.
#[derive(Clone, Copy, Debug)]
pub struct SystemClock {
    origin: Instant,
}

impl SystemClock {
    /// A clock whose origin is now.
    pub fn new() -> Self {
        SystemClock {
            origin: Instant::now(),
        }
    }
}

impl Default for SystemClock {
    fn default() -> Self {
        Self::new()
    }
}

impl Clock for SystemClock {
    fn now(&self) -> Duration {
        self.origin.elapsed()
    }
}

/// A clock that moves only when told to. Clones share one time, so a test
/// keeps a clone and advances it while the tree holds another.
#[derive(Clone, Debug, Default)]
pub struct ManualClock {
    nanos: Arc<AtomicU64>,
}

impl ManualClock {
    /// A clock at zero.
    pub fn new() -> Self {
        Self::default()
    }

    /// Move the clock forward by `by`.
    pub fn advance(&self, by: Duration) {
        self.nanos.fetch_add(saturating_nanos(by), Ordering::SeqCst);
    }

    /// Set the clock to `to`.
    pub fn set(&self, to: Duration) {
        self.nanos.store(saturating_nanos(to), Ordering::SeqCst);
    }
}

fn saturating_nanos(d: Duration) -> u64 {
    u64::try_from(d.as_nanos()).unwrap_or(u64::MAX)
}

impl Clock for ManualClock {
    fn now(&self) -> Duration {
        Duration::from_nanos(self.nanos.load(Ordering::SeqCst))
    }
}

/// How a view presents status: the marker set and whether running work
/// animates. Shared by every view in this module.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Look {
    pub(crate) symbols: SymbolSet,
    pub(crate) animate: bool,
}

impl Default for Look {
    fn default() -> Self {
        Look {
            symbols: SymbolSet::Unicode,
            animate: true,
        }
    }
}

impl Look {
    pub(crate) fn from_policy(policy: &AccessibilityPolicy) -> Self {
        Look {
            symbols: policy.status_symbols,
            animate: !(policy.no_animation || policy.reduced_motion || policy.screen_reader),
        }
    }

    pub(crate) fn ascii(self) -> bool {
        self.symbols != SymbolSet::Unicode
    }

    /// `state`'s marker; a running state animates as a `dots` spinner frame
    /// chosen by `elapsed`, so a redraw moves it and a test can pin it.
    pub(crate) fn marker(self, state: State, elapsed: Duration) -> String {
        if state == State::Running && self.animate && !self.ascii() {
            let spinner = rich::spinner::Spinner::new("dots");
            spinner.render(0.0);
            let frame = spinner.render(elapsed.as_secs_f64());
            return format!("{} running", frame.plain());
        }
        state.marker(self.symbols).to_string()
    }

    pub(crate) fn duration(self, d: Duration) -> String {
        crate::format::duration_with(d, self.ascii())
    }

    pub(crate) fn ellipsis(self) -> &'static str {
        if self.ascii() {
            "..."
        } else {
            "…"
        }
    }
}

/// The theme style `key`, or its [`STYLES`] default.
pub(crate) fn style(console: &Console, key: &str) -> Style {
    let fallback = STYLES
        .iter()
        .find(|(name, _)| *name == key)
        .map_or("none", |(_, spec)| spec);
    crate::event::theme_style(console, key, fallback)
}

/// `Some(style)` for [`Text::append`], dropping null styles.
pub(crate) fn span(console: &Console, key: &str) -> Option<StyleType> {
    let style = style(console, key);
    (!style.is_null()).then(|| style.into())
}

/// Render text lines one after another, wrapped to the options' width.
pub(crate) fn render_texts(
    console: &Console,
    options: &rich::ConsoleOptions,
    lines: &[Text],
) -> Vec<rich::Segment> {
    if options.max_width == 0 || options.height == Some(0) {
        return Vec::new();
    }
    // Each line takes the rows it needs; a height would pad every one of them.
    let mut unbounded = options.clone();
    unbounded.height = None;
    let mut rows = Vec::new();
    for line in lines {
        rows.extend(console.render_lines(line, &unbounded, false));
    }
    if let Some(height) = options.height {
        rows.truncate(height);
    }
    crate::event::flatten(rows)
}
