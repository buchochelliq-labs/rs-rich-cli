//! Progress displays.
//!
//! Port of `rich/progress.py`'s display and task model: a grid of tasks, one
//! row each, whose cells come from a list of [`ProgressColumn`]s. Upstream builds
//! a `Table.grid` (`padding=(0, 1)`); we render the equivalent inline — fixed
//! columns take their widest cell, the bar column flexes to fill (capped at 40),
//! and columns are separated by a single unstyled space.
//!
//! Time is read from an injectable clock ([`Progress::clock`], upstream's
//! `get_time`), so elapsed time, speed, ETA and spinner frames are
//! deterministic under test. The default clock is monotonic.
//!
//! Not ported yet (see docs/DIVERGENCES.md §16): the auto-refreshing `Live`
//! integration and `track()`, the pulsing bar for unstarted or indeterminate
//! tasks, `RenderableColumn`, per-task custom `fields` and table-column options.

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;

use crate::cells::cell_len;
use crate::console::{Console, ConsoleOptions};
use crate::filesize;
use crate::progress_bar::ProgressBar;
use crate::protocol::Renderable;
use crate::segment::Segment;
use crate::spinner::Spinner;
use crate::style::{Style, StyleType};
use crate::text::Text;

/// The default `BarColumn` width (upstream `bar_width=40`); the bar shrinks below
/// this to fit, and never grows past it.
const BAR_MAX_WIDTH: usize = 40;

/// Upstream keeps at most this many speed samples per task (`deque(maxlen=1000)`).
const MAX_SAMPLES: usize = 1000;

/// A source of the current time in seconds. Upstream's `GetTimeCallable`.
pub type GetTime = Arc<dyn Fn() -> f64 + Send + Sync>;

/// The default clock: seconds on a monotonic clock (upstream `time.monotonic`).
fn monotonic() -> f64 {
    static START: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
    START
        .get_or_init(std::time::Instant::now)
        .elapsed()
        .as_secs_f64()
}

/// Identifies a task within one [`Progress`]. Upstream's `TaskID`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TaskId(pub usize);

/// Estimated time remaining. Port of `TimeRemainingColumn`, including its
/// half-second render cache (`max_refresh = 0.5`).
pub struct TimeRemainingColumn {
    compact: bool,
    elapsed_when_finished: bool,
    cache: RefCell<HashMap<TaskId, (f64, Text)>>,
}

impl TimeRemainingColumn {
    /// `compact` drops the hours when there are none (`05:03`);
    /// `elapsed_when_finished` shows the elapsed time once a task finishes.
    pub fn new(compact: bool, elapsed_when_finished: bool) -> Self {
        TimeRemainingColumn {
            compact,
            elapsed_when_finished,
            cache: RefCell::new(HashMap::new()),
        }
    }
}

/// An animated spinner. Port of `SpinnerColumn`: one spinner shared by every
/// row, whose animation starts at its first render.
pub struct SpinnerColumn {
    spinner: Spinner,
    style: StyleType,
    finished_text: String,
    start: Cell<Option<f64>>,
}

impl SpinnerColumn {
    /// A spinner by name (e.g. `"dots"`), styled `progress.spinner`, showing
    /// `finished_text` (console markup) once a task finishes.
    pub fn new(name: &str, finished_text: impl Into<String>) -> Self {
        SpinnerColumn {
            spinner: Spinner::new(name),
            style: StyleType::Name("progress.spinner".to_string()),
            finished_text: finished_text.into(),
            start: Cell::new(None),
        }
    }

    /// Animation speed multiplier (default 1.0).
    pub fn speed(mut self, speed: f64) -> Self {
        self.spinner = self.spinner.speed(speed);
        self
    }

    /// Style of the spinner frame (default `progress.spinner`).
    pub fn style(mut self, style: impl Into<StyleType>) -> Self {
        self.style = style.into();
        self
    }
}

/// A column in a [`Progress`] display. Mirrors upstream's `ProgressColumn`s.
pub enum ProgressColumn {
    /// The task description as console markup
    /// (`TextColumn("[progress.description]{task.description}")`).
    Description,
    /// A static text cell with an explicit style (a simplified `TextColumn`).
    Text(String, Style),
    /// The flexing progress bar (`BarColumn`).
    Bar,
    /// The completion percentage `"{pct:>3}%"` (default `TaskProgressColumn`).
    Percentage,
    /// `TaskProgressColumn(show_speed=…)`: the percentage, or for a task with no
    /// total and `show_speed`, the rate in `it/s`.
    TaskProgress { show_speed: bool },
    /// `"{completed}/{total}"` (`MofNCompleteColumn`, `progress.download`).
    MofN,
    /// `"{completed}/{total} {unit}"` in shared SI byte units, e.g. `0.5/1.0 kB`
    /// (`DownloadColumn`, `progress.download`).
    Download,
    /// As [`Download`](Self::Download) in binary units (`DownloadColumn(binary_units=True)`).
    BinaryDownload,
    /// Elapsed time `H:MM:SS` (`TimeElapsedColumn`, `progress.elapsed`).
    TimeElapsed,
    /// Estimated time remaining (`TimeRemainingColumn`, `progress.remaining`).
    TimeRemaining(TimeRemainingColumn),
    /// Data speed, e.g. `1.2 MB/s` (`TransferSpeedColumn`, `progress.data.speed`).
    TransferSpeed,
    /// Completed size in decimal units (`FileSizeColumn`, `progress.filesize`).
    FileSize,
    /// Total size in decimal units (`TotalFileSizeColumn`, `progress.filesize.total`).
    TotalFileSize,
    /// An animated spinner (`SpinnerColumn`).
    Spinner(SpinnerColumn),
}

impl ProgressColumn {
    /// `TimeRemainingColumn()` with upstream's defaults.
    pub fn time_remaining() -> Self {
        ProgressColumn::TimeRemaining(TimeRemainingColumn::new(false, false))
    }

    /// `SpinnerColumn()` with upstream's defaults (`dots`, finished text `" "`).
    pub fn spinner() -> Self {
        ProgressColumn::Spinner(SpinnerColumn::new("dots", " "))
    }

    fn is_bar(&self) -> bool {
        matches!(self, ProgressColumn::Bar)
    }

    /// The cell for `task` (never called on [`ProgressColumn::Bar`]).
    fn cell(&self, task: &Task) -> Text {
        let named = |plain: String, style: &str| Text::styled(plain, style);
        match self {
            ProgressColumn::Description => {
                let markup = format!("[progress.description]{}", task.description);
                Text::from_markup(&markup).unwrap_or_else(|_| Text::new(task.description.clone()))
            }
            ProgressColumn::Text(text, style) => Text::styled(text.clone(), style.clone()),
            ProgressColumn::Bar => unreachable!("bar column has no text cell"),
            ProgressColumn::Percentage => task.percentage_cell(),
            ProgressColumn::TaskProgress { show_speed } => {
                if task.total.is_none() && *show_speed {
                    render_speed(
                        task.finished_speed
                            .filter(|s| *s != 0.0)
                            .or_else(|| task.speed()),
                    )
                } else {
                    task.percentage_cell()
                }
            }
            ProgressColumn::MofN => named(task.mofn_text(), "progress.download"),
            ProgressColumn::Download => named(task.download_text(false), "progress.download"),
            ProgressColumn::BinaryDownload => named(task.download_text(true), "progress.download"),
            ProgressColumn::TimeElapsed => {
                let elapsed = if task.finished() {
                    task.finished_time
                } else {
                    task.elapsed()
                };
                let text = match elapsed {
                    None => "-:--:--".to_string(),
                    Some(elapsed) => timedelta(elapsed.max(0.0) as i64),
                };
                named(text, "progress.elapsed")
            }
            ProgressColumn::TimeRemaining(column) => column.render(task),
            ProgressColumn::TransferSpeed => {
                let speed = task
                    .finished_speed
                    .filter(|s| *s != 0.0)
                    .or_else(|| task.speed());
                let text = match speed {
                    None => "?".to_string(),
                    Some(speed) => format!("{}/s", filesize::decimal(speed as u64)),
                };
                named(text, "progress.data.speed")
            }
            ProgressColumn::FileSize => named(
                filesize::decimal(task.completed as u64),
                "progress.filesize",
            ),
            ProgressColumn::TotalFileSize => named(
                task.total
                    .map_or_else(String::new, |total| filesize::decimal(total as u64)),
                "progress.filesize.total",
            ),
            ProgressColumn::Spinner(column) => {
                if task.finished() {
                    Text::from_markup(&column.finished_text)
                        .unwrap_or_else(|_| Text::new(column.finished_text.clone()))
                } else {
                    let now = task.now();
                    let start = column.start.get().unwrap_or_else(|| {
                        column.start.set(Some(now));
                        now
                    });
                    let mut frame = column.spinner.render(now - start);
                    frame.set_base_style(column.style.clone());
                    frame
                }
            }
        }
    }
}

impl TimeRemainingColumn {
    fn render(&self, task: &Task) -> Text {
        // `ProgressColumn.__call__`: reuse a render younger than max_refresh,
        // but only while the task has completed nothing (`not task.completed`).
        let now = task.now();
        if task.completed == 0.0 {
            if let Some((timestamp, text)) = self.cache.borrow().get(&task.id) {
                if timestamp + 0.5 > now {
                    return text.clone();
                }
            }
        }
        let (task_time, style) = if self.elapsed_when_finished && task.finished() {
            (task.finished_time, "progress.elapsed")
        } else {
            (task.time_remaining(), "progress.remaining")
        };
        let text = if task.total.is_none() {
            Text::styled("", style)
        } else {
            match task_time {
                None => Text::styled(if self.compact { "--:--" } else { "-:--:--" }, style),
                Some(task_time) => {
                    let whole = task_time as i64;
                    let (minutes, seconds) = (whole.div_euclid(60), whole.rem_euclid(60));
                    let (hours, minutes) = (minutes.div_euclid(60), minutes.rem_euclid(60));
                    let formatted = if self.compact && hours == 0 {
                        format!("{minutes:02}:{seconds:02}")
                    } else {
                        format!("{hours}:{minutes:02}:{seconds:02}")
                    };
                    Text::styled(formatted, style)
                }
            }
        };
        self.cache.borrow_mut().insert(task.id, (now, text.clone()));
        text
    }
}

/// `TaskProgressColumn.render_speed`: iterations per second with a power-of-ten
/// suffix, e.g. `2.5×10³ it/s`.
fn render_speed(speed: Option<f64>) -> Text {
    let Some(speed) = speed else {
        return Text::styled("", "progress.percentage");
    };
    let (unit, suffix) =
        filesize::pick_unit_and_suffix(speed as u64, &["", "×10³", "×10⁶", "×10⁹", "×10¹²"], 1000);
    let data_speed = speed / unit as f64;
    Text::styled(
        format!("{data_speed:.1}{suffix} it/s"),
        "progress.percentage",
    )
}

/// Python's `str(timedelta(seconds=n))` for `n >= 0`: `H:MM:SS`, prefixed by
/// `N day(s), ` from a day upward.
fn timedelta(total_seconds: i64) -> String {
    let days = total_seconds / 86_400;
    let rest = total_seconds % 86_400;
    let clock = format!("{}:{:02}:{:02}", rest / 3600, rest % 3600 / 60, rest % 60);
    match days {
        0 => clock,
        1 => format!("1 day, {clock}"),
        days => format!("{days} days, {clock}"),
    }
}

/// Python's `f"{value:,.{precision}f}"`: fixed precision with `,` grouping.
fn grouped(value: f64, precision: usize) -> String {
    let formatted = format!("{value:.precision$}");
    let (sign, digits) = match formatted.strip_prefix('-') {
        Some(rest) => ("-", rest),
        None => ("", formatted.as_str()),
    };
    let (integer, fraction) = match digits.split_once('.') {
        Some((integer, fraction)) => (integer, Some(fraction)),
        None => (digits, None),
    };
    let mut grouped = String::new();
    for (index, digit) in integer.chars().enumerate() {
        if index > 0 && (integer.len() - index) % 3 == 0 {
            grouped.push(',');
        }
        grouped.push(digit);
    }
    match fraction {
        Some(fraction) => format!("{sign}{grouped}.{fraction}"),
        None => format!("{sign}{grouped}"),
    }
}

/// A single tracked task. Mirrors `rich.progress.Task`; read-only outside
/// [`Progress`].
pub struct Task {
    id: TaskId,
    description: String,
    total: Option<f64>,
    completed: f64,
    visible: bool,
    start_time: Option<f64>,
    stop_time: Option<f64>,
    finished_time: Option<f64>,
    finished_speed: Option<f64>,
    /// `(timestamp, completed)` speed samples (upstream `ProgressSample`).
    samples: VecDeque<(f64, f64)>,
    get_time: GetTime,
}

impl Task {
    fn now(&self) -> f64 {
        (self.get_time)()
    }

    /// This task's id.
    pub fn id(&self) -> TaskId {
        self.id
    }

    /// The description (console markup).
    pub fn description(&self) -> &str {
        &self.description
    }

    /// The total number of steps, or `None` when indeterminate.
    pub fn total(&self) -> Option<f64> {
        self.total
    }

    /// The number of steps completed.
    pub fn completed(&self) -> f64 {
        self.completed
    }

    /// Whether the task is shown.
    pub fn visible(&self) -> bool {
        self.visible
    }

    /// Whether the task has been started.
    pub fn started(&self) -> bool {
        self.start_time.is_some()
    }

    /// Steps left, or `None` when indeterminate.
    pub fn remaining(&self) -> Option<f64> {
        self.total.map(|total| total - self.completed)
    }

    /// Seconds since the task started (to its stop time, if stopped).
    pub fn elapsed(&self) -> Option<f64> {
        let start = self.start_time?;
        Some(self.stop_time.unwrap_or_else(|| self.now()) - start)
    }

    /// Whether the task has reached its total.
    pub fn finished(&self) -> bool {
        self.finished_time.is_some()
    }

    /// The elapsed time recorded when the task finished.
    pub fn finished_time(&self) -> Option<f64> {
        self.finished_time
    }

    /// The completion percentage, clamped to 0–100 (0 without a total).
    pub fn percentage(&self) -> f64 {
        match self.total {
            Some(total) if total != 0.0 => (self.completed / total * 100.0).clamp(0.0, 100.0),
            _ => 0.0,
        }
    }

    /// Steps per second over the sample window, or `None` without enough samples.
    pub fn speed(&self) -> Option<f64> {
        self.start_time?;
        let (first, _) = *self.samples.front()?;
        let (last, _) = *self.samples.back()?;
        let total_time = last - first;
        if total_time == 0.0 {
            return None;
        }
        let total_completed: f64 = self.samples.iter().skip(1).map(|(_, done)| done).sum();
        Some(total_completed / total_time)
    }

    /// Estimated seconds remaining (rounded up), 0 once finished.
    pub fn time_remaining(&self) -> Option<f64> {
        if self.finished() {
            return Some(0.0);
        }
        let speed = self.speed().filter(|speed| *speed != 0.0)?;
        let remaining = self.remaining()?;
        Some((remaining / speed).ceil())
    }

    /// `Task._reset`.
    fn clear_progress(&mut self) {
        self.samples.clear();
        self.finished_time = None;
        self.finished_speed = None;
    }

    /// The percentage cell: `[progress.percentage]{percentage:>3.0f}%`, empty
    /// without a total (`text_format_no_percentage`).
    fn percentage_cell(&self) -> Text {
        if self.total.is_none() {
            return Text::new("");
        }
        let mut text = Text::new(format!("{:>3.0}%", self.percentage()));
        let len = text.plain().len();
        text.stylize("progress.percentage", 0, len);
        text
    }

    /// The M-of-N cell text: `completed` right-justified to the width of `total`
    /// (`?` when indeterminate), then `/total`. Port of `MofNCompleteColumn.render`.
    fn mofn_text(&self) -> String {
        let completed = self.completed as i64;
        let total = self
            .total
            .map_or_else(|| "?".to_string(), |total| (total as i64).to_string());
        let total_width = total.chars().count();
        format!("{completed:>total_width$}/{total}")
    }

    /// The download cell text: `completed`/`total` in a shared byte unit, e.g.
    /// `0.5/1.0 kB`. Port of `DownloadColumn.render`.
    fn download_text(&self, binary: bool) -> String {
        const DECIMAL: &[&str] = &["bytes", "kB", "MB", "GB", "TB", "PB", "EB", "ZB", "YB"];
        const BINARY: &[&str] = &[
            "bytes", "KiB", "MiB", "GiB", "TiB", "PiB", "EiB", "ZiB", "YiB",
        ];
        let completed = self.completed as u64;
        let base_size = self.total.map_or(completed, |total| total as u64);
        let (unit, suffix) = if binary {
            filesize::pick_unit_and_suffix(base_size, BINARY, 1024)
        } else {
            filesize::pick_unit_and_suffix(base_size, DECIMAL, 1000)
        };
        let precision = if unit == 1 { 0 } else { 1 };
        let completed_str = grouped(completed as f64 / unit as f64, precision);
        let total_str = self.total.map_or_else(
            || "?".to_string(),
            |total| grouped((total as u64) as f64 / unit as f64, precision),
        );
        format!("{completed_str}/{total_str} {suffix}")
    }
}

/// Changes for [`Progress::update`]; unset fields are left alone. Upstream's
/// keyword arguments to `Progress.update`.
#[derive(Debug, Clone, Default)]
pub struct TaskUpdate {
    pub total: Option<f64>,
    pub completed: Option<f64>,
    pub advance: Option<f64>,
    pub description: Option<String>,
    pub visible: Option<bool>,
}

impl TaskUpdate {
    pub fn total(mut self, total: f64) -> Self {
        self.total = Some(total);
        self
    }

    pub fn completed(mut self, completed: f64) -> Self {
        self.completed = Some(completed);
        self
    }

    pub fn advance(mut self, advance: f64) -> Self {
        self.advance = Some(advance);
        self
    }

    pub fn description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    pub fn visible(mut self, visible: bool) -> Self {
        self.visible = Some(visible);
        self
    }
}

/// A progress display over one or more [`Task`]s. Mirrors `rich.progress.Progress`.
pub struct Progress {
    tasks: Vec<Task>,
    next_id: usize,
    columns: Vec<ProgressColumn>,
    get_time: GetTime,
    speed_estimate_period: f64,
}

impl Default for Progress {
    fn default() -> Self {
        Progress {
            tasks: Vec::new(),
            next_id: 0,
            columns: Progress::default_columns(),
            get_time: Arc::new(monotonic),
            speed_estimate_period: 30.0,
        }
    }
}

impl Progress {
    pub fn new() -> Self {
        Progress::default()
    }

    /// Upstream's `Progress.get_default_columns()`: description, bar,
    /// percentage and time remaining.
    pub fn default_columns() -> Vec<ProgressColumn> {
        vec![
            ProgressColumn::Description,
            ProgressColumn::Bar,
            ProgressColumn::Percentage,
            ProgressColumn::time_remaining(),
        ]
    }

    /// Replace the column list (default: [`default_columns`](Self::default_columns)).
    pub fn columns(mut self, columns: Vec<ProgressColumn>) -> Self {
        self.columns = columns;
        self
    }

    /// Read time from `clock` (seconds) instead of the monotonic clock.
    /// Upstream's `get_time`; makes time-based columns deterministic.
    pub fn clock(mut self, clock: impl Fn() -> f64 + Send + Sync + 'static) -> Self {
        self.get_time = Arc::new(clock);
        for task in &mut self.tasks {
            task.get_time = self.get_time.clone();
        }
        self
    }

    /// Seconds of history used for speed estimates (default 30).
    pub fn speed_estimate_period(mut self, seconds: f64) -> Self {
        self.speed_estimate_period = seconds;
        self
    }

    fn now(&self) -> f64 {
        (self.get_time)()
    }

    fn task_mut(&mut self, id: TaskId) -> Option<&mut Task> {
        self.tasks.iter_mut().find(|task| task.id == id)
    }

    /// Add a started task and return its id. Port of `Progress.add_task`
    /// (`start=True`); `total` of `None` is an indeterminate task.
    pub fn add_task(
        &mut self,
        description: impl Into<String>,
        total: impl Into<Option<f64>>,
        completed: f64,
    ) -> TaskId {
        let id = self.push_task(description.into(), total.into(), completed);
        self.start_task(id);
        id
    }

    /// Add a task that has not started (`add_task(start=False)`): it shows no
    /// elapsed time until [`start_task`](Self::start_task).
    pub fn add_unstarted_task(
        &mut self,
        description: impl Into<String>,
        total: impl Into<Option<f64>>,
        completed: f64,
    ) -> TaskId {
        self.push_task(description.into(), total.into(), completed)
    }

    fn push_task(&mut self, description: String, total: Option<f64>, completed: f64) -> TaskId {
        let id = TaskId(self.next_id);
        self.next_id += 1;
        self.tasks.push(Task {
            id,
            description,
            total,
            completed,
            visible: true,
            start_time: None,
            stop_time: None,
            finished_time: None,
            finished_speed: None,
            samples: VecDeque::new(),
            get_time: self.get_time.clone(),
        });
        id
    }

    /// The task with this id, if it has not been removed.
    pub fn task(&self, id: TaskId) -> Option<&Task> {
        self.tasks.iter().find(|task| task.id == id)
    }

    /// Every task, in the order added.
    pub fn tasks(&self) -> &[Task] {
        &self.tasks
    }

    /// Whether every task has finished. Port of `Progress.finished`.
    pub fn finished(&self) -> bool {
        self.tasks.iter().all(Task::finished)
    }

    /// Start a task's clock if it has not started. Port of `start_task`.
    pub fn start_task(&mut self, id: TaskId) {
        let now = self.now();
        if let Some(task) = self.task_mut(id) {
            task.start_time.get_or_insert(now);
        }
    }

    /// Stop a task's clock; its elapsed time freezes. Port of `stop_task`.
    pub fn stop_task(&mut self, id: TaskId) {
        let now = self.now();
        if let Some(task) = self.task_mut(id) {
            task.start_time.get_or_insert(now);
            task.stop_time = Some(now);
        }
    }

    /// Update a task. Port of `Progress.update`: a new total clears the speed
    /// samples; positive progress adds a sample; reaching the total records the
    /// finish time.
    pub fn update(&mut self, id: TaskId, update: TaskUpdate) {
        let now = self.now();
        let period = self.speed_estimate_period;
        let Some(task) = self.task_mut(id) else {
            return;
        };
        let completed_start = task.completed;
        if let Some(total) = update.total {
            if Some(total) != task.total {
                task.total = Some(total);
                task.clear_progress();
            }
        }
        if let Some(advance) = update.advance {
            task.completed += advance;
        }
        if let Some(completed) = update.completed {
            task.completed = completed;
        }
        if let Some(description) = update.description {
            task.description = description;
        }
        if let Some(visible) = update.visible {
            task.visible = visible;
        }
        let update_completed = task.completed - completed_start;
        let old_sample_time = now - period;
        while task
            .samples
            .front()
            .is_some_and(|(time, _)| *time < old_sample_time)
        {
            task.samples.pop_front();
        }
        if update_completed > 0.0 {
            task.samples.push_back((now, update_completed));
            if task.samples.len() > MAX_SAMPLES {
                task.samples.pop_front();
            }
        }
        if task.total.is_some_and(|total| task.completed >= total) && task.finished_time.is_none() {
            task.finished_time = task.elapsed();
        }
    }

    /// Advance a task by `amount` steps. Port of `Progress.advance`, which
    /// (unlike `update`) always records a sample and the finish speed.
    pub fn advance(&mut self, id: TaskId, amount: f64) {
        let now = self.now();
        let period = self.speed_estimate_period;
        let Some(task) = self.task_mut(id) else {
            return;
        };
        let completed_start = task.completed;
        task.completed += amount;
        let update_completed = task.completed - completed_start;
        let old_sample_time = now - period;
        while task
            .samples
            .front()
            .is_some_and(|(time, _)| *time < old_sample_time)
        {
            task.samples.pop_front();
        }
        while task.samples.len() > MAX_SAMPLES {
            task.samples.pop_front();
        }
        task.samples.push_back((now, update_completed));
        if task.samples.len() > MAX_SAMPLES {
            task.samples.pop_front();
        }
        if task.total.is_some_and(|total| task.completed >= total) && task.finished_time.is_none() {
            task.finished_time = task.elapsed();
            task.finished_speed = task.speed();
        }
    }

    /// Reset a task to `completed`, optionally restarting its clock and
    /// changing its total. Port of `Progress.reset`. Like upstream, a stop
    /// time set earlier is kept.
    pub fn reset(&mut self, id: TaskId, start: bool, total: Option<f64>, completed: f64) {
        let now = self.now();
        let Some(task) = self.task_mut(id) else {
            return;
        };
        task.clear_progress();
        task.start_time = start.then_some(now);
        if let Some(total) = total {
            task.total = Some(total);
        }
        task.completed = completed;
        task.finished_time = None;
    }

    /// Remove a task. Port of `Progress.remove_task`.
    pub fn remove_task(&mut self, id: TaskId) {
        self.tasks.retain(|task| task.id != id);
    }
}

impl Renderable for Progress {
    fn rich_render(&self, console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
        let width = options.max_width;
        let ncols = self.columns.len();
        let tasks: Vec<&Task> = self.tasks.iter().filter(|task| task.visible).collect();

        // Render every cell once: spinners and the remaining-time cache are
        // stateful, as upstream's columns are (each is called once per row).
        let cells: Vec<Vec<Option<Text>>> = tasks
            .iter()
            .map(|task| {
                self.columns
                    .iter()
                    .map(|column| (!column.is_bar()).then(|| column.cell(task)))
                    .collect()
            })
            .collect();

        // Fixed columns take their widest cell; bar columns flex.
        let mut col_widths = vec![0usize; ncols];
        for row in &cells {
            for (index, cell) in row.iter().enumerate() {
                if let Some(cell) = cell {
                    col_widths[index] = col_widths[index].max(cell_len(cell.plain()));
                }
            }
        }

        // The bar column(s) share whatever the fixed columns and the single-space
        // gaps leave, each capped at the default bar width. Port of the grid's
        // shrink-to-fit over `no_wrap` fixed columns + a flexing `BarColumn`.
        let gaps = ncols.saturating_sub(1);
        let fixed_sum: usize = col_widths.iter().sum();
        let bar_count = self.columns.iter().filter(|c| c.is_bar()).count();
        let bar_width = width
            .saturating_sub(fixed_sum + gaps)
            .checked_div(bar_count)
            .map_or(0, |per_bar| BAR_MAX_WIDTH.min(per_bar));
        for (index, column) in self.columns.iter().enumerate() {
            if column.is_bar() {
                col_widths[index] = bar_width;
            }
        }

        let theme = console.theme();
        let mut lines: Vec<Vec<Segment>> = Vec::with_capacity(tasks.len());
        for (task, row_cells) in tasks.iter().zip(cells) {
            let mut row: Vec<Segment> = Vec::new();
            for (index, (column, cell)) in self.columns.iter().zip(row_cells).enumerate() {
                if index > 0 {
                    // Inter-column gap: one unstyled space (the grid's collapsed
                    // padding, whose column style is null).
                    row.push(Segment::new(" ", None));
                }
                if column.is_bar() {
                    let bar = ProgressBar::new(
                        task.total.unwrap_or(0.0).max(0.0),
                        task.completed.max(0.0),
                    )
                    .width(bar_width);
                    row.extend(bar.rich_render(console, &options.update_width(bar_width)));
                } else if let Some(mut cell) = cell {
                    // Padding takes the cell's base style, not its spans — as a
                    // grid pads a `Text` cell upstream.
                    cell.truncate(col_widths[index], None, true);
                    row.extend(cell.render(theme, &Style::new()));
                }
            }
            lines.push(row);
        }

        let mut segments = Vec::new();
        let last = lines.len().saturating_sub(1);
        for (index, line) in lines.into_iter().enumerate() {
            segments.extend(line);
            if index != last {
                segments.push(Segment::line());
            }
        }
        segments
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::color::ColorSystem;

    fn render(progress: &Progress) -> String {
        Console::builder()
            .force_terminal(true)
            .color_system(Some(ColorSystem::Truecolor))
            .width(50)
            .no_color(false)
            .build()
            .render_to_string(progress)
    }

    #[test]
    fn three_tasks_match_upstream() {
        // Captured from real rich 15.0.0 (default columns, width 50).
        let mut progress = Progress::new().columns(vec![
            ProgressColumn::Description,
            ProgressColumn::Bar,
            ProgressColumn::Percentage,
        ]);
        progress.add_task("Downloading", 100.0, 50.0);
        progress.add_task("Processing", 100.0, 100.0);
        progress.add_task("Waiting", 100.0, 0.0);
        let expected = concat!(
            "Downloading \x1b[38;2;249;38;114m━━━━━━━━━━━━━━━━\x1b[0m",
            "\x1b[38;2;249;38;114m╸\x1b[0m\x1b[38;5;237m━━━━━━━━━━━━━━━━\x1b[0m \x1b[35m 50%\x1b[0m\n",
            "Processing  \x1b[38;2;114;156;31m",
            "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━\x1b[0m \x1b[35m100%\x1b[0m\n",
            "Waiting     \x1b[38;5;237m",
            "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━\x1b[0m \x1b[35m  0%\x1b[0m",
        );
        assert_eq!(render(&progress), expected);
    }

    #[test]
    fn download_text_matches_upstream() {
        // Captured from real rich 15.0.0 DownloadColumn.render (decimal units).
        let dl = |completed: f64, total: f64| {
            let mut progress = Progress::new();
            let id = progress.add_task("", total, completed);
            progress.task(id).unwrap().download_text(false)
        };
        assert_eq!(dl(500.0, 1000.0), "0.5/1.0 kB");
        assert_eq!(dl(500.0, 999.0), "500/999 bytes");
        assert_eq!(dl(1_500_000.0, 3_000_000.0), "1.5/3.0 MB");
        assert_eq!(dl(0.0, 1024.0), "0.0/1.0 kB");
        assert_eq!(dl(2_500_000_000.0, 10_000_000_000.0), "2.5/10.0 GB");
        assert_eq!(dl(250.0, 250.0), "250/250 bytes");
    }

    #[test]
    fn download_column_in_grid_matches_upstream() {
        // Captured from real rich 15.0.0: description + bar + download at width 50.
        let mut progress = Progress::new().columns(vec![
            ProgressColumn::Description,
            ProgressColumn::Bar,
            ProgressColumn::Download,
        ]);
        progress.add_task("File", 1000.0, 500.0);
        let expected = concat!(
            "File \x1b[38;2;249;38;114m━━━━━━━━━━━━━━━━━\x1b[0m\x1b[38;5;237m╺\x1b[0m",
            "\x1b[38;5;237m━━━━━━━━━━━━━━━━\x1b[0m \x1b[32m0.5/1.0 kB\x1b[0m",
        );
        assert_eq!(render(&progress), expected);
    }

    #[test]
    fn custom_columns_with_mofn_match_upstream() {
        // Captured from real rich 15.0.0: description + bar + M-of-N (differing
        // M-of-N widths → the narrower cell left-justifies with green padding).
        let mut progress = Progress::new().columns(vec![
            ProgressColumn::Description,
            ProgressColumn::Bar,
            ProgressColumn::MofN,
        ]);
        progress.add_task("A", 5.0, 3.0);
        progress.add_task("B", 100.0, 50.0);
        let console = Console::builder()
            .force_terminal(true)
            .color_system(Some(ColorSystem::Truecolor))
            .width(40)
            .no_color(false)
            .build();
        let expected = concat!(
            "A \x1b[38;2;249;38;114m━━━━━━━━━━━━━━━━━━\x1b[0m\x1b[38;5;237m╺\x1b[0m",
            "\x1b[38;5;237m━━━━━━━━━━━\x1b[0m \x1b[32m3/5    \x1b[0m\n",
            "B \x1b[38;2;249;38;114m━━━━━━━━━━━━━━━\x1b[0m\x1b[38;5;237m╺\x1b[0m",
            "\x1b[38;5;237m━━━━━━━━━━━━━━\x1b[0m \x1b[32m 50/100\x1b[0m",
        );
        assert_eq!(console.render_to_string(&progress), expected);
    }
}
