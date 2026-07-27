//! Progress displays (static rendering).
//!
//! Port of `rich/progress.py`'s default display: a grid of tasks, each shown as
//! a **description**, a **bar** (which flexes to fill the width), and a
//! **percentage**. Upstream composes arbitrary `ProgressColumn`s in a
//! `Table.grid`; this first slice hard-codes the three default deterministic
//! columns (the time/rate columns are non-deterministic and deferred, along with
//! the `Live` refresh loop — see the Live/progress issue).

use crate::cells::{cell_len, set_cell_size};
use crate::console::{Console, ConsoleOptions};
use crate::progress_bar::ProgressBar;
use crate::protocol::Renderable;
use crate::segment::Segment;
use crate::style::Style;

/// The default `BarColumn` width (upstream `bar_width=40`); the bar shrinks below
/// this to fit, and never grows past it.
const BAR_MAX_WIDTH: usize = 40;

/// A single tracked task. Mirrors the fields of `rich.progress.Task` this slice
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
}

/// A progress display over one or more [`Task`]s. Mirrors `rich.progress.Progress`
/// with its default columns.
#[derive(Default)]
pub struct Progress {
    tasks: Vec<Task>,
}

impl Progress {
    pub fn new() -> Self {
        Progress::default()
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
        let description_width = self
            .tasks
            .iter()
            .map(|task| cell_len(&task.description))
            .max()
            .unwrap_or(0);
        let percentage_width = self
            .tasks
            .iter()
            .map(|task| cell_len(&task.percentage_text()))
            .max()
            .unwrap_or(0);

        // The bar is the one flexible column: it fills whatever the fixed columns
        // and the two single-space gaps leave, up to its max width. Port of the
        // grid's shrink-to-fit over a `no_wrap` description + percentage.
        let bar_width =
            BAR_MAX_WIDTH.min(width.saturating_sub(description_width + percentage_width + 2));

        let percentage_style = Style::parse("magenta").expect("valid built-in style");
        let mut lines: Vec<Vec<Segment>> = Vec::with_capacity(self.tasks.len());
        for task in &self.tasks {
            let mut row: Vec<Segment> = Vec::new();
            // Description (progress.description = none → plain), padded to width.
            row.push(Segment::new(
                set_cell_size(&task.description, description_width),
                None,
            ));
            row.push(Segment::new(" ", None)); // column gap
                                               // Bar (flexed to `bar_width`).
            let bar = ProgressBar::new(task.total, task.completed).width(bar_width);
            row.extend(bar.rich_render(console, &options.update_width(bar_width)));
            row.push(Segment::new(" ", None)); // column gap
                                               // Percentage (progress.percentage = magenta).
            row.push(Segment::new(
                task.percentage_text(),
                Some(percentage_style.clone()),
            ));
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
}
