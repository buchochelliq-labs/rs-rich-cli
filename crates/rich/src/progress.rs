//! Progress displays.
//!
//! Port of `rich/progress.py`'s display and task model: a grid of tasks, one
//! row each, whose cells come from a list of [`ProgressColumn`]s, laid out as
//! upstream's `make_tasks_table` does: a [`Table::grid`] with `padding=(0, 1)`,
//! each column's table-column options, and `expand`.
//!
//! Time is read from an injectable clock ([`Progress::clock`], upstream's
//! `get_time`), so elapsed time, speed, ETA and spinner frames are
//! deterministic under test. The default clock is monotonic.
//!
//! [`Progress::start`] runs the display live on the [`Live`](crate::live::Live)
//! refresh thread ([`LiveProgress`]); [`LiveProgress::track`] and [`track`]
//! port `track()`. `TextColumn` format strings use [`pyformat`].
//!
//! `transient` and `disable` apply to the live display; [`LiveProgress::wrap_read`]
//! and [`LiveProgress::open`] port `wrap_file` and `open`.

use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap, VecDeque};
use std::sync::Arc;

use crate::console::{Console, ConsoleOptions, Justify};
use crate::filesize;
use crate::progress_bar::ProgressBar;
use crate::protocol::Renderable;
use crate::pyformat::{self, FormatValue};
use crate::segment::Segment;
use crate::spinner::Spinner;
use crate::style::{Style, StyleType};
use crate::table::{Cell, ColumnOptions, Table};
use crate::text::Text;

/// Upstream keeps at most this many speed samples per task (`deque(maxlen=1000)`).
const MAX_SAMPLES: usize = 1000;

/// A source of the current time in seconds. Upstream's `GetTimeCallable`.
pub use crate::console::GetTime;

use crate::console::monotonic;

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
}

impl SpinnerColumn {
    /// A spinner by name (e.g. `"dots"`), styled `progress.spinner`, showing
    /// `finished_text` (console markup) once a task finishes.
    pub fn new(name: &str, finished_text: impl Into<String>) -> Self {
        SpinnerColumn {
            spinner: Spinner::new(name),
            style: StyleType::Name("progress.spinner".to_string()),
            finished_text: finished_text.into(),
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

/// A text cell built from a format string. Port of `TextColumn`: the format
/// is expanded against the task as `text_format.format(task=task)`, so it can
/// use any task attribute (`{task.completed}`, `{task.percentage:>3.0f}`) and
/// per-task fields (`{task.fields[name]}`).
pub struct TextColumn {
    text_format: String,
    style: StyleType,
    justify: Justify,
    markup: bool,
}

impl TextColumn {
    /// `TextColumn(text_format)` with upstream's defaults: no style, left
    /// justified, console markup on.
    pub fn new(text_format: impl Into<String>) -> Self {
        TextColumn {
            text_format: text_format.into(),
            style: StyleType::default(),
            justify: Justify::Left,
            markup: true,
        }
    }

    /// The style of the whole cell (upstream `style`).
    pub fn style(mut self, style: impl Into<StyleType>) -> Self {
        self.style = style.into();
        self
    }

    /// Justify the text within the column (upstream `justify`).
    pub fn justify(mut self, justify: Justify) -> Self {
        self.justify = justify;
        self
    }

    /// Parse the expanded text as console markup (upstream `markup`, default on).
    pub fn markup(mut self, markup: bool) -> Self {
        self.markup = markup;
        self
    }

    fn render(&self, task: &Task) -> Text {
        let expanded = pyformat::format(&self.text_format, |name| task.format_field(name));
        let mut text = if self.markup {
            Text::from_markup(&expanded).unwrap_or_else(|_| Text::new(expanded.clone()))
        } else {
            Text::new(expanded)
        };
        text.set_base_style(self.style.clone());
        text.set_justify(self.justify);
        text
    }
}

/// A column in a [`Progress`] display. Mirrors upstream's `ProgressColumn`s.
pub enum ProgressColumn {
    /// The task description as console markup
    /// (`TextColumn("[progress.description]{task.description}")`).
    Description,
    /// A static text cell with an explicit style (a simplified `TextColumn`).
    Text(String, Style),
    /// A text cell formatted from the task (`TextColumn`).
    TextFormat(TextColumn),
    /// The same renderable in every row (`RenderableColumn`). It may span
    /// several lines; the row grows to fit it.
    Renderable(Arc<dyn Renderable + Send + Sync>),
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
    /// A bar with its width and styles set (`BarColumn(bar_width=…, style=…)`).
    /// A `bar_width` of `None` lets the bar fill its column.
    BarWith(BarColumn),
    /// A column with explicit table-column options (upstream's `table_column=`
    /// argument). See [`ProgressColumn::with_table_column`].
    WithTableColumn(Box<ProgressColumn>, ColumnOptions),
}

/// A progress bar column's width and styles. Port of `BarColumn`'s arguments.
#[derive(Clone, Debug)]
pub struct BarColumn {
    bar_width: Option<usize>,
    style: StyleType,
    complete_style: StyleType,
    finished_style: StyleType,
    pulse_style: StyleType,
}

impl Default for BarColumn {
    fn default() -> Self {
        BarColumn {
            bar_width: Some(40),
            style: "bar.back".into(),
            complete_style: "bar.complete".into(),
            finished_style: "bar.finished".into(),
            pulse_style: "bar.pulse".into(),
        }
    }
}

impl BarColumn {
    /// Upstream's defaults: 40 cells wide, `bar.*` styles.
    pub fn new() -> Self {
        BarColumn::default()
    }

    /// The bar width, or `None` to fill the column (upstream `bar_width`).
    pub fn bar_width(mut self, width: Option<usize>) -> Self {
        self.bar_width = width;
        self
    }

    /// The background style (upstream `style`).
    pub fn style(mut self, style: impl Into<StyleType>) -> Self {
        self.style = style.into();
        self
    }

    /// The completed-part style (upstream `complete_style`).
    pub fn complete_style(mut self, style: impl Into<StyleType>) -> Self {
        self.complete_style = style.into();
        self
    }

    /// The finished style (upstream `finished_style`).
    pub fn finished_style(mut self, style: impl Into<StyleType>) -> Self {
        self.finished_style = style.into();
        self
    }

    /// The pulse style (upstream `pulse_style`).
    pub fn pulse_style(mut self, style: impl Into<StyleType>) -> Self {
        self.pulse_style = style.into();
        self
    }

    /// Port of `BarColumn.render`.
    fn render(&self, task: &Task) -> ProgressBar {
        let bar = match task.total {
            Some(total) => ProgressBar::new(total.max(0.0), task.completed.max(0.0)),
            None => ProgressBar::indeterminate(),
        };
        let bar = match self.bar_width {
            Some(width) => bar.width(width.max(1)),
            None => bar,
        };
        bar.pulse(!task.started())
            .animation_time(task.now())
            .style(self.style.clone())
            .complete_style(self.complete_style.clone())
            .finished_style(self.finished_style.clone())
            .pulse_style(self.pulse_style.clone())
    }
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

    /// This column with explicit table-column options, as upstream's
    /// `table_column=Column(...)` argument sets them: width, ratio, justify,
    /// wrapping and style of the grid column.
    pub fn with_table_column(self, options: ColumnOptions) -> Self {
        let inner = match self {
            ProgressColumn::WithTableColumn(inner, _) => *inner,
            column => column,
        };
        ProgressColumn::WithTableColumn(Box::new(inner), options)
    }

    /// Port of `get_table_column()`: text columns default to
    /// `Column(no_wrap=True)`, the rest to `Column()`.
    fn table_column(&self) -> ColumnOptions {
        match self {
            ProgressColumn::WithTableColumn(_, options) => options.clone(),
            ProgressColumn::Description
            | ProgressColumn::Text(..)
            | ProgressColumn::TextFormat(_)
            | ProgressColumn::Percentage
            | ProgressColumn::TaskProgress { .. } => ColumnOptions {
                no_wrap: true,
                ..ColumnOptions::default()
            },
            _ => ColumnOptions::default(),
        }
    }

    /// The grid cell for `task`: the column's `__call__(task)`.
    fn table_cell(&self, task: &Task) -> Cell {
        match self {
            ProgressColumn::WithTableColumn(inner, _) => inner.table_cell(task),
            ProgressColumn::Bar => Cell::Renderable(Arc::new(BarColumn::default().render(task))),
            ProgressColumn::BarWith(column) => Cell::Renderable(Arc::new(column.render(task))),
            ProgressColumn::Renderable(renderable) => Cell::Renderable(renderable.clone()),
            column => Cell::Text(column.cell(task)),
        }
    }

    /// The cell for `task` (never called on [`ProgressColumn::Bar`]).
    fn cell(&self, task: &Task) -> Text {
        let named = |plain: String, style: &str| Text::styled(plain, style);
        match self {
            // `TextColumn`s hand their `justify` (default left) to the text, so
            // it overrides the table column's.
            ProgressColumn::Description => {
                let markup = format!("[progress.description]{}", task.description);
                Text::from_markup(&markup)
                    .unwrap_or_else(|_| Text::new(task.description.clone()))
                    .justify(Justify::Left)
            }
            ProgressColumn::Text(text, style) => {
                Text::styled(text.clone(), style.clone()).justify(Justify::Left)
            }
            ProgressColumn::TextFormat(column) => column.render(task),
            ProgressColumn::Bar
            | ProgressColumn::BarWith(_)
            | ProgressColumn::Renderable(_)
            | ProgressColumn::WithTableColumn(..) => {
                unreachable!("bar, renderable and wrapped columns have no text cell")
            }
            ProgressColumn::Percentage => task.percentage_cell().justify(Justify::Left),
            ProgressColumn::TaskProgress { show_speed } => {
                if task.total.is_none() && *show_speed {
                    render_speed(
                        task.finished_speed
                            .filter(|s| *s != 0.0)
                            .or_else(|| task.speed()),
                    )
                } else {
                    task.percentage_cell().justify(Justify::Left)
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
                    Some(speed) => format!("{}/s", filesize::decimal_signed(speed as i64)),
                };
                named(text, "progress.data.speed")
            }
            // `filesize.decimal(int(task.completed))`: `int()` truncates
            // toward zero and keeps the sign.
            ProgressColumn::FileSize => named(
                filesize::decimal_signed(task.completed as i64),
                "progress.filesize",
            ),
            ProgressColumn::TotalFileSize => named(
                task.total
                    .map_or_else(String::new, |total| filesize::decimal_signed(total as i64)),
                "progress.filesize.total",
            ),
            ProgressColumn::Spinner(column) => {
                if task.finished() {
                    Text::from_markup(&column.finished_text)
                        .unwrap_or_else(|_| Text::new(column.finished_text.clone()))
                } else {
                    // Upstream's `self.spinner.render(task.get_time())`: the
                    // spinner itself starts its animation at the first render.
                    let mut frame = column.spinner.render(task.now());
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
    let (unit, suffix) = filesize::pick_unit_and_suffix_signed(
        speed as i64,
        &["", "×10³", "×10⁶", "×10⁹", "×10¹²"],
        1000,
    );
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
    /// Arbitrary per-task values for format strings (upstream `fields`).
    fields: BTreeMap<String, FormatValue>,
    get_time: GetTime,
}

impl Task {
    /// This task's custom fields (upstream `Task.fields`).
    pub fn fields(&self) -> &BTreeMap<String, FormatValue> {
        &self.fields
    }

    /// Resolve a `str.format` field name against this task, as
    /// `text_format.format(task=task)` does: `task.<attribute>` or
    /// `task.fields[<name>]`.
    fn format_field(&self, name: &str) -> Option<FormatValue> {
        let attribute = name.strip_prefix("task.")?;
        if let Some(key) = attribute
            .strip_prefix("fields[")
            .and_then(|rest| rest.strip_suffix(']'))
        {
            return self.fields.get(key).cloned();
        }
        Some(match attribute {
            "id" => FormatValue::Int(self.id.0 as i64),
            "description" => FormatValue::Str(self.description.clone()),
            // Python keeps the ints a caller passes (`total=200` prints `200`);
            // this API takes floats, so a whole number formats as an int.
            "total" => self.total.map_or(FormatValue::None, whole_number),
            "completed" => whole_number(self.completed),
            "visible" => FormatValue::Bool(self.visible),
            "started" => FormatValue::Bool(self.started()),
            "finished" => FormatValue::Bool(self.finished()),
            "percentage" => FormatValue::Float(self.percentage()),
            "remaining" => self.remaining().into(),
            "elapsed" => self.elapsed().into(),
            "speed" => self.speed().into(),
            "time_remaining" => self.time_remaining().into(),
            "start_time" => self.start_time.into(),
            "stop_time" => self.stop_time.into(),
            "finished_time" => self.finished_time.into(),
            "finished_speed" => self.finished_speed.into(),
            _ => return None,
        })
    }
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
        // `int(task.completed)` / `int(task.total)`: truncated, sign kept.
        let completed = self.completed as i64;
        let base_size = self.total.map_or(completed, |total| total as i64);
        let (unit, suffix) = if binary {
            filesize::pick_unit_and_suffix_signed(base_size, BINARY, 1024)
        } else {
            filesize::pick_unit_and_suffix_signed(base_size, DECIMAL, 1000)
        };
        let precision = if unit == 1 { 0 } else { 1 };
        let completed_str = grouped(completed as f64 / unit as f64, precision);
        let total_str = self.total.map_or_else(
            || "?".to_string(),
            |total| grouped((total as i64) as f64 / unit as f64, precision),
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
    /// Custom fields to set (upstream `**fields`).
    pub fields: Vec<(String, FormatValue)>,
    /// Redraw a live display right after the update (upstream `refresh=True`).
    pub refresh: bool,
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

    /// Redraw a [`LiveProgress`] right after this update (upstream
    /// `refresh=True`).
    pub fn refresh(mut self, refresh: bool) -> Self {
        self.refresh = refresh;
        self
    }

    /// Set a custom field (upstream `update(task_id, **fields)`).
    pub fn field(mut self, name: impl Into<String>, value: impl Into<FormatValue>) -> Self {
        self.fields.push((name.into(), value.into()));
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
    expand: bool,
    transient: bool,
    disable: bool,
}

impl Default for Progress {
    fn default() -> Self {
        Progress {
            tasks: Vec::new(),
            next_id: 0,
            columns: Progress::default_columns(),
            get_time: Arc::new(monotonic),
            speed_estimate_period: 30.0,
            expand: false,
            transient: false,
            disable: false,
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

    /// Stretch the task grid to the full width (upstream `expand`).
    pub fn expand(mut self, expand: bool) -> Self {
        self.expand = expand;
        self
    }

    /// Erase the display when it stops (upstream `transient`).
    pub fn transient(mut self, transient: bool) -> Self {
        self.transient = transient;
        self
    }

    /// Show nothing: [`start`](Self::start) draws no display, while tasks
    /// still update (upstream `disable`).
    pub fn disable(mut self, disable: bool) -> Self {
        self.disable = disable;
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

    /// Add a task with custom fields for format strings. Port of
    /// `add_task(description, total=…, completed=…, start=…, **fields)`.
    pub fn add_task_with<K: Into<String>, V: Into<FormatValue>>(
        &mut self,
        description: impl Into<String>,
        total: impl Into<Option<f64>>,
        completed: f64,
        start: bool,
        fields: impl IntoIterator<Item = (K, V)>,
    ) -> TaskId {
        let id = self.push_task(description.into(), total.into(), completed);
        if let Some(task) = self.task_mut(id) {
            task.fields = fields
                .into_iter()
                .map(|(name, value)| (name.into(), value.into()))
                .collect();
        }
        if start {
            self.start_task(id);
        }
        id
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
            fields: BTreeMap::new(),
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
        task.fields.extend(update.fields);
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

impl Progress {
    /// The grid the display renders. Port of `Progress.make_tasks_table`: a
    /// `Table.grid` with one column per [`ProgressColumn`] (its table column
    /// options), `padding=(0, 1)` and the progress's `expand`, and one row per
    /// visible task.
    pub fn make_tasks_table(&self) -> Table {
        let mut table = Table::grid().padding(0, 1, 0, 1).expand(self.expand);
        for column in &self.columns {
            table.add_column_with(Text::new(""), column.table_column());
        }
        for task in self.tasks.iter().filter(|task| task.visible) {
            // Each column is called once per row, in order: spinners and the
            // remaining-time cache are stateful, as upstream's columns are.
            let cells = self
                .columns
                .iter()
                .map(|column| column.table_cell(task))
                .collect();
            table.add_row_cells(cells);
        }
        table
    }
}

impl Renderable for Progress {
    fn rich_render(&self, console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
        self.make_tasks_table().rich_render(console, options)
    }
}

/// A [`Progress`] shared with the auto-refresh thread of a live display.
struct ProgressView(Arc<std::sync::Mutex<Progress>>);

impl Renderable for ProgressView {
    fn rich_render(&self, console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
        match self.0.lock() {
            Ok(progress) => progress.rich_render(console, options),
            Err(poisoned) => poisoned.into_inner().rich_render(console, options),
        }
    }
}

impl Progress {
    /// Start an auto-refreshing live display of this progress, redrawn
    /// `refresh_per_second` times a second on a background thread. Port of
    /// `Progress.start` with `auto_refresh=True`; stop it with
    /// [`LiveProgress::stop`] (upstream's `with progress:` block).
    pub fn start<W: std::io::Write + Send + 'static>(
        self,
        console: Console,
        writer: W,
        refresh_per_second: f64,
    ) -> LiveProgress<W> {
        // `disable` draws nothing: upstream skips `live.start()` and `stop()`.
        if self.disable {
            return LiveProgress {
                progress: Arc::new(std::sync::Mutex::new(self)),
                live: None,
                writer: Some(writer),
                interactive: true,
                holder: std::sync::Mutex::new(None),
            };
        }
        let transient = self.transient;
        let interactive = console.is_terminal();
        let shared = Arc::new(std::sync::Mutex::new(self));
        let live = crate::live::Live::spawn_with(
            Box::new(ProgressView(shared.clone())),
            console,
            writer,
            refresh_per_second,
            transient,
        );
        LiveProgress {
            progress: shared,
            live: Some(live),
            writer: None,
            interactive,
            holder: std::sync::Mutex::new(None),
        }
    }
}

/// A running [`Progress`] display. Task changes made through it are picked up
/// by the next refresh, as with upstream's `refresh=False` updates.
pub struct LiveProgress<W: std::io::Write + Send + 'static> {
    progress: Arc<std::sync::Mutex<Progress>>,
    live: Option<crate::live::AutoLive<W>>,
    /// The sink of a disabled display, which never reaches a live thread.
    writer: Option<W>,
    /// Whether the console is a terminal; `stop` ends a file with a newline.
    interactive: bool,
    /// The thread inside [`with`](LiveProgress::with), if any. Upstream's lock
    /// is an `RLock`; ours is not, so re-entry is detected rather than left to
    /// deadlock on the mutex or on the refresh thread.
    holder: std::sync::Mutex<Option<std::thread::ThreadId>>,
}

/// Clears [`LiveProgress`]'s `holder` when a `with` block ends, unwinding
/// included.
struct HolderGuard<'a>(&'a std::sync::Mutex<Option<std::thread::ThreadId>>);

impl Drop for HolderGuard<'_> {
    fn drop(&mut self) {
        *self.0.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = None;
    }
}

impl<W: std::io::Write + Send + 'static> LiveProgress<W> {
    /// Run `f` with the progress locked, for any change not wrapped below.
    ///
    /// `f` may call [`refresh`](Self::refresh) (the frame is drawn as soon as
    /// the lock is released), but not `with` or a method built on it: upstream
    /// re-enters its `RLock`, which a `&mut Progress` cannot express, so a
    /// nested call panics instead of deadlocking.
    pub fn with<R>(&self, f: impl FnOnce(&mut Progress) -> R) -> R {
        let current = std::thread::current().id();
        assert!(
            !self.held_by(current),
            "LiveProgress::with re-entered from inside a `with` closure; \
             use the `&mut Progress` it was given instead"
        );
        let mut progress = match self.progress.lock() {
            Ok(progress) => progress,
            Err(poisoned) => poisoned.into_inner(),
        };
        *self
            .holder
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(current);
        let _holder = HolderGuard(&self.holder);
        f(&mut progress)
    }

    /// Whether `thread` is inside [`with`](Self::with) right now.
    fn held_by(&self, thread: std::thread::ThreadId) -> bool {
        *self
            .holder
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            == Some(thread)
    }

    /// [`Progress::add_task`], then redraw (upstream's `add_task` refreshes).
    pub fn add_task(
        &self,
        description: impl Into<String>,
        total: impl Into<Option<f64>>,
        completed: f64,
    ) -> TaskId {
        let id = self.with(|progress| progress.add_task(description, total, completed));
        self.refresh();
        id
    }

    /// [`Progress::reset`], then redraw (upstream's `reset` refreshes).
    pub fn reset(&self, id: TaskId, start: bool, total: Option<f64>, completed: f64) {
        self.with(|progress| progress.reset(id, start, total, completed));
        self.refresh();
    }

    /// [`Progress::advance`].
    pub fn advance(&self, id: TaskId, amount: f64) {
        self.with(|progress| progress.advance(id, amount));
    }

    /// [`Progress::update`]; redraws when the update asks to
    /// ([`TaskUpdate::refresh`]).
    pub fn update(&self, id: TaskId, update: TaskUpdate) {
        let refresh = update.refresh;
        self.with(|progress| progress.update(id, update));
        if refresh {
            self.refresh();
        }
    }

    /// Redraw now rather than at the next tick, returning once the frame is
    /// written. Port of `Progress.refresh`.
    ///
    /// Called from inside [`with`](Self::with), the redraw is queued instead:
    /// the refresh thread needs the lock this thread holds to render, so
    /// waiting for it would deadlock. The frame is drawn when `with` returns.
    pub fn refresh(&self) {
        if let Some(live) = &self.live {
            if self.held_by(std::thread::current().id()) {
                live.refresh();
            } else {
                live.refresh_wait();
            }
        }
    }

    /// Iterate `iter`, advancing a new task by one after each item is
    /// processed. Port of `Progress.track`: the total defaults to the
    /// iterator's exact length, else the task is indeterminate.
    pub fn track<I: IntoIterator>(
        &self,
        iter: I,
        total: Option<f64>,
        description: impl Into<String>,
    ) -> Track<'_, I::IntoIter, W> {
        let iter = iter.into_iter();
        let total = total.or_else(|| match iter.size_hint() {
            (lower, Some(upper)) if lower == upper && lower > 0 => Some(lower as f64),
            _ => None,
        });
        let task = self.add_task(description, total, 0.0);
        Track {
            iter,
            progress: self,
            task,
            pending: false,
        }
    }

    /// Track reading from `reader`: each read advances the task by the bytes
    /// read. Port of `Progress.wrap_file`: `total` is the byte count, or else
    /// the total of `task`; a new task named `description` is added when
    /// `task` is `None`, otherwise `task`'s total is set.
    pub fn wrap_read<R: std::io::Read>(
        &self,
        reader: R,
        total: Option<u64>,
        task: Option<TaskId>,
        description: impl Into<String>,
    ) -> std::io::Result<ProgressReader<'_, R, W>> {
        let total = total.map(|total| total as f64).or_else(|| {
            task.and_then(|task| self.with(|progress| progress.task(task).and_then(Task::total)))
        });
        let Some(total) = total else {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "unable to get the total number of bytes, please specify 'total'",
            ));
        };
        let task = self.task_for(task, total, description);
        Ok(ProgressReader {
            reader,
            progress: self,
            task,
        })
    }

    /// Open `path` for reading and track it. Port of `Progress.open` in
    /// binary mode: `total` defaults to the file's size.
    pub fn open(
        &self,
        path: impl AsRef<std::path::Path>,
        total: Option<u64>,
        task: Option<TaskId>,
        description: impl Into<String>,
    ) -> std::io::Result<ProgressReader<'_, std::fs::File, W>> {
        let file = std::fs::File::open(path)?;
        let total = match total {
            Some(total) => total,
            None => file.metadata()?.len(),
        };
        let task = self.task_for(task, total as f64, description);
        Ok(ProgressReader {
            reader: file,
            progress: self,
            task,
        })
    }

    /// A new task with `total`, or `task` with its total set to it.
    fn task_for(&self, task: Option<TaskId>, total: f64, description: impl Into<String>) -> TaskId {
        match task {
            Some(task) => {
                self.update(task, TaskUpdate::default().total(total));
                task
            }
            None => self.add_task(description, total, 0.0),
        }
    }

    /// Commit the final frame, stop the refresh thread, and return the
    /// progress and the output sink. Port of `Progress.stop`.
    pub fn stop(mut self) -> (Progress, W) {
        let writer = match self.live.take() {
            Some(live) => {
                let mut writer = live.stop();
                // `Progress.stop`: `console.print()` when not interactive.
                if !self.interactive {
                    let _ = writer.write_all(b"\n");
                }
                writer
            }
            None => self
                .writer
                .take()
                .expect("a disabled display keeps its writer"),
        };
        let progress = match Arc::try_unwrap(std::mem::replace(
            &mut self.progress,
            Arc::new(std::sync::Mutex::new(Progress::new())),
        )) {
            Ok(mutex) => mutex
                .into_inner()
                .unwrap_or_else(|poisoned| poisoned.into_inner()),
            // The refresh thread has exited, so this is the only owner left.
            Err(_) => unreachable!("progress still shared after the live display stopped"),
        };
        (progress, writer)
    }
}

/// A reader that advances a task by the bytes read through it. Returned by
/// [`LiveProgress::wrap_read`] and [`LiveProgress::open`] (upstream `_Reader`).
pub struct ProgressReader<'a, R, W: std::io::Write + Send + 'static> {
    reader: R,
    progress: &'a LiveProgress<W>,
    task: TaskId,
}

impl<R, W: std::io::Write + Send + 'static> ProgressReader<'_, R, W> {
    /// The task this reader advances.
    pub fn task(&self) -> TaskId {
        self.task
    }

    /// The wrapped reader.
    pub fn into_inner(self) -> R {
        self.reader
    }
}

impl<R: std::io::Read, W: std::io::Write + Send + 'static> std::io::Read
    for ProgressReader<'_, R, W>
{
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let count = self.reader.read(buf)?;
        self.progress.advance(self.task, count as f64);
        Ok(count)
    }
}

impl<R: std::io::BufRead, W: std::io::Write + Send + 'static> std::io::BufRead
    for ProgressReader<'_, R, W>
{
    fn fill_buf(&mut self) -> std::io::Result<&[u8]> {
        self.reader.fill_buf()
    }

    fn consume(&mut self, amount: usize) {
        self.reader.consume(amount);
        self.progress.advance(self.task, amount as f64);
    }
}

/// The iterator [`LiveProgress::track`] returns.
pub struct Track<'a, I: Iterator, W: std::io::Write + Send + 'static> {
    iter: I,
    progress: &'a LiveProgress<W>,
    task: TaskId,
    /// Whether an item has been handed out but not yet counted: upstream
    /// advances after the loop body, when the next item is requested.
    pending: bool,
}

impl<I: Iterator, W: std::io::Write + Send + 'static> Track<'_, I, W> {
    /// The task this iterator advances.
    pub fn task(&self) -> TaskId {
        self.task
    }
}

impl<I: Iterator, W: std::io::Write + Send + 'static> Iterator for Track<'_, I, W> {
    type Item = I::Item;

    fn next(&mut self) -> Option<I::Item> {
        if std::mem::take(&mut self.pending) {
            self.progress.advance(self.task, 1.0);
        }
        let item = self.iter.next();
        if item.is_some() {
            self.pending = true;
        } else {
            self.progress.refresh();
        }
        item
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.iter.size_hint()
    }
}

/// Track progress over `iter` with a live display on stdout. Port of the
/// module-level `rich.progress.track`: the description, bar, progress and
/// time-remaining columns, refreshed ten times a second, stopped when the
/// iterator is exhausted or dropped.
pub fn track<I: IntoIterator>(iter: I, description: &str) -> TrackStdout<I::IntoIter> {
    let mut columns = Vec::new();
    if !description.is_empty() {
        columns.push(ProgressColumn::Description);
    }
    columns.extend([
        ProgressColumn::Bar,
        ProgressColumn::TaskProgress { show_speed: true },
        ProgressColumn::TimeRemaining(TimeRemainingColumn::new(false, true)),
    ]);
    let iter = iter.into_iter();
    let total = match iter.size_hint() {
        (lower, Some(upper)) if lower == upper && lower > 0 => Some(lower as f64),
        _ => None,
    };
    let live = Progress::new()
        .columns(columns)
        .start(Console::new(), std::io::stdout(), 10.0);
    let task = live.add_task(description, total, 0.0);
    TrackStdout {
        iter,
        live: Some(live),
        task,
        pending: false,
    }
}

/// The iterator [`track`] returns; it owns its live display.
pub struct TrackStdout<I: Iterator> {
    iter: I,
    live: Option<LiveProgress<std::io::Stdout>>,
    task: TaskId,
    pending: bool,
}

impl<I: Iterator> Iterator for TrackStdout<I> {
    type Item = I::Item;

    fn next(&mut self) -> Option<I::Item> {
        let live = self.live.as_ref()?;
        if std::mem::take(&mut self.pending) {
            live.advance(self.task, 1.0);
        }
        match self.iter.next() {
            Some(item) => {
                self.pending = true;
                Some(item)
            }
            None => {
                if let Some(live) = self.live.take() {
                    live.stop();
                }
                None
            }
        }
    }
}

impl<I: Iterator> Drop for TrackStdout<I> {
    fn drop(&mut self) {
        if let Some(live) = self.live.take() {
            live.stop();
        }
    }
}

/// A whole, exactly representable count as an int, else the float.
fn whole_number(value: f64) -> FormatValue {
    if value.fract() == 0.0 && value.abs() < 9_007_199_254_740_992.0 {
        FormatValue::Int(value as i64)
    } else {
        FormatValue::Float(value)
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

    fn live(columns: Vec<ProgressColumn>) -> LiveProgress<Vec<u8>> {
        let console = Console::builder()
            .force_terminal(true)
            .color_system(Some(ColorSystem::Truecolor))
            .width(40)
            .build();
        Progress::new()
            .columns(columns)
            .clock(|| 0.0)
            .start(console, Vec::new(), 1e-9)
    }

    /// Run `f` on its own thread and fail (rather than hang the suite) if it
    /// has not finished within a few seconds.
    fn within_deadline<R: Send + 'static>(f: impl FnOnce() -> R + Send + 'static) -> R {
        let (done, wait) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = done.send(std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)));
        });
        match wait.recv_timeout(std::time::Duration::from_secs(10)) {
            Ok(Ok(value)) => value,
            Ok(Err(payload)) => std::panic::resume_unwind(payload),
            Err(_) => panic!("deadlocked: did not finish within 10s"),
        }
    }

    #[test]
    fn refresh_inside_with_does_not_deadlock() {
        // Upstream's `Progress` lock is an `RLock` and `refresh()` renders in
        // the caller's thread, so refreshing while holding the lock is fine.
        let output = within_deadline(|| {
            let live = live(vec![ProgressColumn::Description, ProgressColumn::MofN]);
            live.with(|progress| {
                let task = progress.add_task("inside", Some(2.0), 1.0);
                live.refresh();
                task
            });
            live.refresh();
            String::from_utf8(live.stop().1).unwrap()
        });
        assert!(
            output.ends_with("inside \x1b[32m1/2\x1b[0m\n\x1b[?25h"),
            "{output:?}"
        );
    }

    #[test]
    fn nested_with_panics_instead_of_deadlocking() {
        let result = within_deadline(|| {
            let live = live(vec![ProgressColumn::Description]);
            let nested = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                live.with(|_| live.add_task("nested", None, 0.0))
            }));
            // The display is still usable after the refused re-entry.
            let task = live.add_task("after", None, 0.0);
            live.stop();
            (nested.is_err(), task)
        });
        assert!(result.0, "a nested `with` must be refused, not deadlock");
    }

    #[test]
    fn track_counts_each_item_after_its_loop_body() {
        let live = live(vec![ProgressColumn::Description, ProgressColumn::MofN]);
        let mut seen = Vec::new();
        let tracked = live.track(vec!['a', 'b', 'c'], None, "letters");
        let task = tracked.task();
        for item in tracked {
            // Upstream advances after the loop body, so the item being
            // processed is not yet counted.
            let completed = live.with(|progress| progress.task(task).unwrap().completed());
            seen.push((item, completed));
        }
        assert_eq!(seen, vec![('a', 0.0), ('b', 1.0), ('c', 2.0)]);
        let (progress, bytes) = live.stop();
        let task = progress.task(task).unwrap();
        assert_eq!((task.total(), task.completed()), (Some(3.0), 3.0));
        let output = String::from_utf8(bytes).unwrap();
        assert!(
            output.ends_with("letters \x1b[32m3/3\x1b[0m\n\x1b[?25h"),
            "{output:?}"
        );
    }

    #[test]
    fn track_leaves_an_iterator_of_unknown_length_indeterminate() {
        let live = live(vec![ProgressColumn::MofN]);
        let task = {
            let mut tracked = live.track((0..10).filter(|n| n % 3 == 0), None, "");
            let task = tracked.task();
            assert_eq!(tracked.by_ref().count(), 4);
            task
        };
        let with_total = {
            let mut tracked = live.track(0..2, Some(5.0), "");
            tracked.by_ref().for_each(drop);
            tracked.task()
        };
        let (progress, _) = live.stop();
        assert_eq!(progress.task(task).unwrap().total(), None);
        assert_eq!(progress.task(task).unwrap().completed(), 4.0);
        assert_eq!(progress.task(with_total).unwrap().total(), Some(5.0));
    }

    fn quiet_console() -> Console {
        Console::builder().force_terminal(false).width(40).build()
    }

    #[test]
    fn wrap_read_advances_by_the_bytes_read() {
        use std::io::Read;
        let live = Progress::new()
            .disable(true)
            .start(quiet_console(), Vec::new(), 1.0);
        let mut reader = live
            .wrap_read(&b"hello world"[..], Some(11), None, "Reading...")
            .expect("total given");
        let task = reader.task();
        let mut buf = [0u8; 4];
        reader.read_exact(&mut buf).unwrap();
        assert_eq!(live.with(|p| p.task(task).unwrap().completed()), 4.0);
        let mut rest = Vec::new();
        reader.read_to_end(&mut rest).unwrap();
        assert!(live.with(|p| p.task(task).unwrap().finished()));
        let (_, out) = live.stop();
        assert!(out.is_empty(), "a disabled display writes nothing");
    }

    #[test]
    fn wrap_read_needs_a_total() {
        let live = Progress::new()
            .disable(true)
            .start(quiet_console(), Vec::new(), 1.0);
        let err = live
            .wrap_read(&b""[..], None, None, "x")
            .err()
            .expect("no total");
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput);
        let task = live.add_task("sized", 5.0, 0.0);
        assert!(live.wrap_read(&b""[..], None, Some(task), "x").is_ok());
    }

    #[test]
    fn open_takes_the_file_size_as_total() {
        use std::io::Read;
        let path = std::env::temp_dir().join(format!("rs-rich-open-{}", std::process::id()));
        std::fs::write(&path, b"0123456789").unwrap();
        let live = Progress::new()
            .disable(true)
            .start(quiet_console(), Vec::new(), 1.0);
        let mut reader = live.open(&path, None, None, "Reading...").unwrap();
        let task = reader.task();
        assert_eq!(live.with(|p| p.task(task).unwrap().total()), Some(10.0));
        std::io::copy(&mut reader, &mut std::io::sink()).unwrap();
        assert_eq!(live.with(|p| p.task(task).unwrap().completed()), 10.0);
        let _ = reader.read(&mut [0u8; 1]);
        drop(reader);
        std::fs::remove_file(path).unwrap();
    }
}
