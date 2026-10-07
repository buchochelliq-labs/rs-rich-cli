//! A streaming log: lines arrive at the bottom, older ones scroll up.
//!
//! Re-rendering a whole log for every new line is what made the spike's
//! log scenario slower than ratatui. A [`Log`] keeps its lines in a bounded
//! buffer; its node renders each line once, and on an append it moves the
//! rows already on screen up inside its own rectangle and renders only the
//! new lines into the rows that opened at the bottom.
//!
//! ```
//! use intuituive::prelude::*;
//!
//! let app = App::new(|| {
//!     let log = Log::new(1_000);
//!     log.push("[green]started[/]");
//!     log.view().panel("Log").on_key("q", |cx| cx.quit())
//! });
//! ```

use std::collections::VecDeque;

use rich::Segment;

use crate::node::{Axis, Node};
use crate::reactive::{signal, Signal};
use crate::widget::{Canvas, DrawCx, MeasureCx, Widget};

pub(crate) struct LogData {
    pub lines: VecDeque<String>,
    pub capacity: usize,
    /// Lines ever pushed, so a view knows how many arrived since it drew.
    pub total: u64,
}

/// A bounded log of console-markup lines. A `Copy` handle, like a signal:
/// move it into as many closures as you like.
#[derive(Clone, Copy)]
pub struct Log {
    pub(crate) data: Signal<LogData>,
}

impl Log {
    /// A log keeping the last `capacity` lines. Call it while the app is
    /// being built (it makes a signal).
    pub fn new(capacity: usize) -> Log {
        Log {
            data: signal(LogData {
                lines: VecDeque::new(),
                capacity: capacity.max(1),
                total: 0,
            }),
        }
    }

    /// Add a line (console markup) at the bottom.
    pub fn push(&self, line: impl Into<String>) {
        let line = line.into();
        self.data.update(|data| {
            if data.lines.len() == data.capacity {
                data.lines.pop_front();
            }
            data.lines.push_back(line);
            data.total += 1;
        });
    }

    /// Remove every line.
    pub fn clear(&self) {
        self.data.update(|data| {
            data.lines.clear();
            // A total that jumps past the view's makes it draw everything.
            data.total += u32::MAX as u64;
        });
    }

    pub fn len(&self) -> usize {
        self.data.with_untracked(|data| data.lines.len())
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// A node showing the latest lines, one per row.
    pub fn view(&self) -> Node {
        Node::from_widget(
            Box::new(LogView {
                log: *self,
                drawn_total: None,
                shown: 0,
            }),
            "log",
        )
    }
}

pub(crate) struct LogView {
    pub log: Log,
    /// The log's total when the view last drew.
    pub drawn_total: Option<u64>,
    /// How many lines it showed then (at most its height).
    pub shown: usize,
}

impl Widget for LogView {
    fn name(&self) -> &'static str {
        "log"
    }

    fn measure(&mut self, _cx: &MeasureCx, axis: Axis, width: u16, _height: u16) -> u16 {
        match axis {
            Axis::Vertical => self
                .log
                .data
                .with(|data| data.lines.len())
                .min(u16::MAX as usize) as u16,
            Axis::Horizontal => width,
        }
    }

    /// Only what arrived since the last frame draws, moving what is on
    /// screen up, unless the log moved or was drawn over.
    fn draw(&mut self, cx: &mut DrawCx, canvas: &mut Canvas) {
        let total = self.log.data.with(|data| data.total);
        let (width, height) = (canvas.width(), canvas.height() as usize);
        let full = cx.repaint();
        let arrived = self
            .drawn_total
            .map(|drawn| total.saturating_sub(drawn) as usize);
        let console = cx.console();
        let shown = self.shown;
        self.log.data.with_untracked(|data| {
            let render = |from: usize| -> Vec<Vec<Segment>> {
                data.lines
                    .range(from..)
                    .map(|line| render_line(console, line, width))
                    .collect()
            };
            let len = data.lines.len();
            match arrived {
                Some(0) if !full => {}
                Some(n) if !full && n < height => {
                    if len == shown + n && shown + n <= height {
                        // Room left and nothing dropped: the new lines go
                        // under the old.
                        canvas.lines_at(0, shown as u16, width, n as u16, &render(shown));
                    } else if shown == height && len >= height {
                        // Full: scroll what is on screen up, and render only
                        // the new lines into the rows that opened.
                        canvas.scroll_up(n as u16);
                        let top = (height - n) as u16;
                        canvas.lines_at(0, top, width, n as u16, &render(len - n));
                    } else {
                        // Lines were dropped from a log shorter than the
                        // view: draw what it keeps.
                        canvas.lines(&render(len.saturating_sub(height)));
                    }
                }
                _ => canvas.lines(&render(len.saturating_sub(height))),
            }
        });
        self.drawn_total = Some(total);
        self.shown = self
            .log
            .data
            .with_untracked(|data| data.lines.len())
            .min(height);
    }

    fn retained(&self) -> bool {
        true
    }
}

/// One log line rendered to one row at `width` (cropped, never wrapped).
fn render_line(console: &rich::Console, markup: &str, width: u16) -> Vec<Segment> {
    let text = rich::Text::from_markup(markup)
        .unwrap_or_else(|_| rich::Text::new(markup))
        .no_wrap(true)
        .overflow(rich::Overflow::Crop);
    let options = console.options().update_width(width.max(1) as usize);
    console
        .render_lines(&text, &options, false)
        .into_iter()
        .next()
        .unwrap_or_default()
}
