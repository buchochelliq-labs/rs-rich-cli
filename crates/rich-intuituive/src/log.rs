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

use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;

use rich::Segment;

use crate::node::{Kind, Node};
use crate::reactive::{signal, Signal};

pub(crate) struct LogData {
    pub lines: VecDeque<String>,
    pub capacity: usize,
    /// Lines ever pushed, so a view knows how many arrived since it drew.
    pub total: u64,
}

/// A bounded log of console-markup lines. `Copy`-cheap to clone: clones
/// share the same lines.
#[derive(Clone)]
pub struct Log {
    pub(crate) data: Rc<RefCell<LogData>>,
    pub(crate) version: Signal<u64>,
}

impl Log {
    /// A log keeping the last `capacity` lines. Call it while the app is
    /// being built (it makes a signal).
    pub fn new(capacity: usize) -> Log {
        Log {
            data: Rc::new(RefCell::new(LogData {
                lines: VecDeque::new(),
                capacity: capacity.max(1),
                total: 0,
            })),
            version: signal(0),
        }
    }

    /// Add a line (console markup) at the bottom.
    pub fn push(&self, line: impl Into<String>) {
        {
            let mut data = self.data.borrow_mut();
            if data.lines.len() == data.capacity {
                data.lines.pop_front();
            }
            data.lines.push_back(line.into());
            data.total += 1;
        }
        self.version.update(|v| *v += 1);
    }

    /// Remove every line.
    pub fn clear(&self) {
        self.data.borrow_mut().lines.clear();
        // A total that jumps past the view's makes it draw everything.
        self.data.borrow_mut().total += u32::MAX as u64;
        self.version.update(|v| *v += 1);
    }

    pub fn len(&self) -> usize {
        self.data.borrow().lines.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// A node showing the latest lines, one per row.
    pub fn view(&self) -> Node {
        Node::new_kind(Kind::Log(LogView {
            log: self.clone(),
            drawn_total: None,
        }))
    }
}

pub(crate) struct LogView {
    pub log: Log,
    /// The log's total when the view last drew.
    pub drawn_total: Option<u64>,
}

/// One log line rendered to one row at `width` (cropped, never wrapped).
pub(crate) fn render_line(console: &rich::Console, markup: &str, width: u16) -> Vec<Segment> {
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
