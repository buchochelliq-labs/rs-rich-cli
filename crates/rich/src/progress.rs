//! Progress displays (static rendering).
//!
//! Port of `rich/progress.py`'s display: a grid of tasks, one row each, whose
//! cells come from a list of [`ProgressColumn`]s. Upstream builds a `Table.grid`
//! (`padding=(0, 1)`); we render the equivalent inline — fixed columns take their
//! widest cell, the bar column flexes to fill (capped at 40), and columns are
//! separated by a single unstyled space.
//!
//! The **deterministic** columns are ported: description, static text, the bar,
//! percentage, and M-of-N. The time/rate/spinner columns depend on wall-clock
//! elapsed (and the `Live` refresh loop) and remain deferred — see the
//! Live/progress issue and docs/DIVERGENCES.md.

use crate::cells::{cell_len, set_cell_size};
use crate::console::{Console, ConsoleOptions};
use crate::filesize;
use crate::progress_bar::ProgressBar;
use crate::protocol::Renderable;
use crate::segment::Segment;
use crate::style::Style;

/// The default `BarColumn` width (upstream `bar_width=40`); the bar shrinks below
/// this to fit, and never grows past it.
const BAR_MAX_WIDTH: usize = 40;

/// A column in a [`Progress`] display. Mirrors the deterministic subset of
/// upstream's `ProgressColumn`s.
pub enum ProgressColumn {
    /// The task description (`progress.description` — no style).
    Description,
    /// A static text cell with an explicit style (a simplified `TextColumn`).
    Text(String, Style),
    /// The flexing progress bar (`BarColumn`).
    Bar,
    /// The completion percentage `"{pct:>3}%"` (`progress.percentage` — magenta).
    Percentage,
    /// `"{completed}/{total}"` (`MofNCompleteColumn`, `progress.download` — green).
    MofN,
    /// `"{completed}/{total} {unit}"` in shared SI byte units, e.g. `0.5/1.0 kB`
    /// (`DownloadColumn`, `progress.download` — green).
    Download,
}

impl ProgressColumn {
    fn is_bar(&self) -> bool {
        matches!(self, ProgressColumn::Bar)
    }

    /// The `(text, style)` cell for `task` (never called on [`ProgressColumn::Bar`]).
    fn cell(&self, task: &Task) -> (String, Option<Style>) {
        let style = |spec: &str| Style::parse(spec).expect("valid built-in style");
        match self {
            ProgressColumn::Description => (task.description.clone(), None),
            ProgressColumn::Text(text, text_style) => (text.clone(), Some(text_style.clone())),
            ProgressColumn::Percentage => (task.percentage_text(), Some(style("magenta"))),
            ProgressColumn::MofN => (task.mofn_text(), Some(style("green"))),
            ProgressColumn::Download => (task.download_text(), Some(style("green"))),
            ProgressColumn::Bar => unreachable!("bar column has no text cell"),
        }
    }
}

/// A single tracked task. Mirrors the fields of `rich.progress.Task` this port
/// renders.
pub struct Task {
    description: String,
    total: f64,
    completed: f64,
}

impl Task {
    /// The clamped completion percentage (`Task.percentage`).
    fn percentage(&self) -> f64 {
        if self.total > 0.0 {
            (self.completed / self.total * 100.0).clamp(0.0, 100.0)
        } else {
            0.0
        }
    }

    /// The percentage cell text (`{task.percentage:>3.0f}%`).
    fn percentage_text(&self) -> String {
        format!("{:>3}%", self.percentage().round() as i64)
    }

    /// The M-of-N cell text: `completed` right-justified to the width of `total`,
    /// then `/total`. Port of `MofNCompleteColumn.render`.
    fn mofn_text(&self) -> String {
        let completed = self.completed as i64;
        let total = self.total as i64;
        let total_width = total.to_string().len();
        format!("{completed:>total_width$}/{total}")
    }

    /// The download cell text: `completed`/`total` in a shared SI byte unit, e.g.
    /// `0.5/1.0 kB`. Port of `DownloadColumn.render` (decimal units). The ratio is
    /// always `< base`, so upstream's `,` thousands grouping never triggers.
    fn download_text(&self) -> String {
        const SUFFIXES: &[&str] = &["bytes", "kB", "MB", "GB", "TB", "PB", "EB", "ZB", "YB"];
        let completed = self.completed as u64;
        let total = self.total as u64;
        let (unit, suffix) = filesize::pick_unit_and_suffix(total, SUFFIXES, 1000);
        let precision = if unit == 1 { 0 } else { 1 };
        let completed_ratio = completed as f64 / unit as f64;
        let total_ratio = total as f64 / unit as f64;
        format!("{completed_ratio:.precision$}/{total_ratio:.precision$} {suffix}")
    }
}

/// A progress display over one or more [`Task`]s. Mirrors `rich.progress.Progress`.
pub struct Progress {
    tasks: Vec<Task>,
    columns: Vec<ProgressColumn>,
}

impl Default for Progress {
    fn default() -> Self {
        Progress {
            tasks: Vec::new(),
            // Upstream's default columns: description, bar, percentage.
            columns: vec![
                ProgressColumn::Description,
                ProgressColumn::Bar,
                ProgressColumn::Percentage,
            ],
        }
    }
}

impl Progress {
    pub fn new() -> Self {
        Progress::default()
    }

    /// Replace the column list (default: description, bar, percentage).
    pub fn columns(mut self, columns: Vec<ProgressColumn>) -> Self {
        self.columns = columns;
        self
    }

    /// Add a task with the given description, total, and current completion.
    pub fn add_task(
        &mut self,
        description: impl Into<String>,
        total: f64,
        completed: f64,
    ) -> &mut Self {
        self.tasks.push(Task {
            description: description.into(),
            total,
            completed,
        });
        self
    }
}

impl Renderable for Progress {
    fn rich_render(&self, console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
        let width = options.max_width;
        let ncols = self.columns.len();

        // Fixed columns take their widest cell; bar columns flex.
        let mut col_widths = vec![0usize; ncols];
        for (index, column) in self.columns.iter().enumerate() {
            if column.is_bar() {
                continue;
            }
            col_widths[index] = self
                .tasks
                .iter()
                .map(|task| cell_len(&column.cell(task).0))
                .max()
                .unwrap_or(0);
        }

        // The bar column(s) share whatever the fixed columns and the single-space
        // gaps leave, each capped at the default bar width. Port of the grid's
        // shrink-to-fit over `no_wrap` fixed columns + a flexing `BarColumn`.
        let gaps = ncols.saturating_sub(1);
        let fixed_sum: usize = col_widths.iter().sum();
        let bar_count = self.columns.iter().filter(|c| c.is_bar()).count();
        // Free width split across the bar column(s), each capped at the default.
        let bar_width = width
            .saturating_sub(fixed_sum + gaps)
            .checked_div(bar_count)
            .map_or(0, |per_bar| BAR_MAX_WIDTH.min(per_bar));
        for (index, column) in self.columns.iter().enumerate() {
            if column.is_bar() {
                col_widths[index] = bar_width;
            }
        }

        let mut lines: Vec<Vec<Segment>> = Vec::with_capacity(self.tasks.len());
        for task in &self.tasks {
            let mut row: Vec<Segment> = Vec::new();
            for (index, column) in self.columns.iter().enumerate() {
                if index > 0 {
                    // Inter-column gap: one unstyled space (the grid's collapsed
                    // padding, whose column style is null).
                    row.push(Segment::new(" ", None));
                }
                if column.is_bar() {
                    let bar = ProgressBar::new(task.total, task.completed).width(bar_width);
                    row.extend(bar.rich_render(console, &options.update_width(bar_width)));
                } else {
                    let (text, style) = column.cell(task);
                    row.push(Segment::new(set_cell_size(&text, col_widths[index]), style));
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
        let mut progress = Progress::new();
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
            Task {
                description: String::new(),
                total,
                completed,
            }
            .download_text()
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
