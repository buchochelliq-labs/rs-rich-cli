//! The standard end-of-command summary: how it went, how long it took, what
//! needs attention and what to do next.
//!
//! ```text
//! ✖ error Deploy  4m 12s
//!   1 failed, 1 warning, 5 succeeded, 1 skipped
//!   ✖ error upload / images  1m 02s  403 Forbidden
//!   ⚠ warning build  12.4s  2 deprecations
//!   ↷ skipped verify
//! Next steps:
//!   → check the bucket policy
//!   → rerun with --resume
//! ```
//!
//! A [`CompletionSummary`] is built by hand or from a finished
//! [`TaskTree`] (`CompletionSummary::from(&tree)`), whose leaf tasks become
//! its items and counts. By default only items that need attention —
//! anything but a success — are listed; the counts cover the rest. With
//! [`SymbolSet::Ascii`] or [`SymbolSet::Words`] every marker, bullet and
//! unit is ASCII, so the summary carries its meaning in a log file or
//! without colour.
//!
//! ```
//! use std::time::Duration;
//! use rich::Console;
//! use rich_ext::a11y::SymbolSet;
//! use rich_ext::workflow::{CompletionSummary, State};
//!
//! let summary = CompletionSummary::new("Release")
//!     .item(State::Succeeded, "publish")
//!     .item(State::Warning, "changelog")
//!     .duration(Duration::from_secs(95))
//!     .next_step("announce the release")
//!     .symbols(SymbolSet::Ascii);
//! let console = Console::builder().width(40).build();
//! assert_eq!(
//!     console.render_to_string(&summary),
//!     "[WARN] Release  1m 35s\n  1 warning, 1 succeeded\n  [WARN] changelog\nNext steps:\n  - announce the release",
//! );
//! ```

use std::collections::BTreeMap;
use std::time::Duration;

use rich::{Console, ConsoleOptions, Renderable, Segment, Text};

use super::command::CommandRecord;
use super::tasks::TaskTree;
use super::{render_texts, span, Look, State};
use crate::a11y::{AccessibilityPolicy, SymbolSet};

/// One line of a [`CompletionSummary`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SummaryItem {
    /// How it ended.
    pub state: State,
    /// What it was.
    pub label: String,
    /// How long it took.
    pub duration: Option<Duration>,
    /// A short explanation: the failure, the warning, the skip reason.
    pub detail: Option<String>,
}

impl SummaryItem {
    /// An item with no duration or detail.
    pub fn new(state: State, label: impl Into<String>) -> Self {
        SummaryItem {
            state,
            label: label.into(),
            duration: None,
            detail: None,
        }
    }

    /// Set the duration.
    pub fn duration(mut self, duration: Duration) -> Self {
        self.duration = Some(duration);
        self
    }

    /// Set the detail.
    pub fn detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = Some(detail.into());
        self
    }
}

impl From<&CommandRecord> for SummaryItem {
    /// The command line, its state and duration, and its exit detail.
    fn from(record: &CommandRecord) -> Self {
        let mut item = SummaryItem::new(record.state(), record.command_line());
        item.duration = Some(record.duration);
        let detail = record.status.detail();
        if !detail.is_empty() {
            item.detail = Some(detail);
        }
        item
    }
}

/// An end-of-command summary. See the [module docs](self).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompletionSummary {
    title: String,
    status: Option<State>,
    counts: BTreeMap<State, usize>,
    duration: Option<Duration>,
    items: Vec<SummaryItem>,
    next_steps: Vec<String>,
    show_all_items: bool,
    look: Look,
}

impl CompletionSummary {
    /// An empty summary titled `title`.
    pub fn new(title: impl Into<String>) -> Self {
        CompletionSummary {
            title: title.into(),
            status: None,
            counts: BTreeMap::new(),
            duration: None,
            items: Vec::new(),
            next_steps: Vec::new(),
            show_all_items: false,
            look: Look {
                animate: false,
                ..Look::default()
            },
        }
    }

    /// Set the overall state instead of deriving it from the counts.
    pub fn status(mut self, state: State) -> Self {
        self.status = Some(state);
        self
    }

    /// Set how many things ended in `state`, instead of counting items.
    pub fn count(mut self, state: State, count: usize) -> Self {
        self.counts.insert(state, count);
        self
    }

    /// Set the total duration.
    pub fn duration(mut self, duration: Duration) -> Self {
        self.duration = Some(duration);
        self
    }

    /// Add an item with no duration or detail.
    pub fn item(self, state: State, label: impl Into<String>) -> Self {
        self.push(SummaryItem::new(state, label))
    }

    /// Add an item.
    pub fn push(mut self, item: SummaryItem) -> Self {
        self.items.push(item);
        self
    }

    /// Add a next step, shown as a bulleted list at the end.
    pub fn next_step(mut self, step: impl Into<String>) -> Self {
        self.next_steps.push(step.into());
        self
    }

    /// List every item, not only those that need attention.
    pub fn show_all_items(mut self, show: bool) -> Self {
        self.show_all_items = show;
        self
    }

    /// Mark status with `set`; `Ascii` and `Words` also use ASCII bullets
    /// and units.
    pub fn symbols(mut self, set: SymbolSet) -> Self {
        self.look.symbols = set;
        self
    }

    /// Follow `policy`'s status symbols.
    pub fn policy(mut self, policy: &AccessibilityPolicy) -> Self {
        self.look.symbols = policy.status_symbols;
        self
    }

    /// The counts by state: those set with [`count`](Self::count), or else
    /// the items counted.
    pub fn counts(&self) -> BTreeMap<State, usize> {
        if !self.counts.is_empty() {
            return self.counts.clone();
        }
        let mut counts = BTreeMap::new();
        for item in &self.items {
            *counts.entry(item.state).or_insert(0) += 1;
        }
        counts
    }

    /// The overall state: the one set with [`status`](Self::status), or the
    /// worst counted state (skipped only when everything was skipped,
    /// succeeded when there is nothing).
    pub fn overall(&self) -> State {
        if let Some(state) = self.status {
            return state;
        }
        let counts = self.counts();
        let present: Vec<State> = counts
            .iter()
            .filter(|(_, n)| **n > 0)
            .map(|(s, _)| *s)
            .collect();
        match present.as_slice() {
            [] => State::Succeeded,
            [State::Skipped] => State::Skipped,
            _ => present
                .into_iter()
                .filter(|s| *s != State::Skipped)
                .min()
                .unwrap_or(State::Succeeded),
        }
    }

    fn lines(&self, console: &Console) -> Vec<Text> {
        let look = self.look;
        let overall = self.overall();
        let mut lines = Vec::new();

        let mut title = Text::new("");
        title.append(
            &look.marker(overall, Duration::ZERO),
            span(console, overall.style_key()),
        );
        title.append(" ", None);
        title.append(&self.title, span(console, "workflow.summary.title"));
        if let Some(duration) = self.duration {
            title.append("  ", None);
            title.append(&look.duration(duration), span(console, "workflow.duration"));
        }
        lines.push(title);

        let counts = self.counts();
        let mut line = Text::new("  ");
        let mut first = true;
        for state in State::ALL {
            let Some(&count) = counts.get(&state).filter(|n| **n > 0) else {
                continue;
            };
            if !first {
                line.append(", ", span(console, "workflow.summary.counts"));
            }
            first = false;
            line.append(&state.count_label(count), span(console, state.style_key()));
        }
        if !first {
            lines.push(line);
        }

        for item in &self.items {
            if !self.show_all_items && item.state == State::Succeeded {
                continue;
            }
            let mut text = Text::new("  ");
            text.append(
                &look.marker(item.state, Duration::ZERO),
                span(console, item.state.style_key()),
            );
            text.append(" ", None);
            text.append(&item.label, span(console, "workflow.task.label"));
            if let Some(duration) = item.duration {
                text.append("  ", None);
                text.append(&look.duration(duration), span(console, "workflow.duration"));
            }
            if let Some(detail) = &item.detail {
                text.append("  ", None);
                text.append(detail, span(console, "workflow.task.note"));
            }
            lines.push(text);
        }

        if !self.next_steps.is_empty() {
            lines.push(Text::styled(
                "Next steps:",
                super::style(console, "workflow.summary.next"),
            ));
            let bullet = if look.ascii() { "-" } else { "→" };
            for step in &self.next_steps {
                lines.push(Text::new(format!("  {bullet} {step}")));
            }
        }
        lines
    }
}

impl Renderable for CompletionSummary {
    fn rich_render(&self, console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
        render_texts(console, options, &self.lines(console))
    }
}

impl From<&TaskTree> for CompletionSummary {
    /// Title from the tree's title (or `Tasks`), state from
    /// [`TaskTree::overall`], duration from [`TaskTree::total_elapsed`], and
    /// one item per leaf task labelled with its path (`upload / images`).
    fn from(tree: &TaskTree) -> Self {
        let mut summary =
            CompletionSummary::new(tree.get_title().unwrap_or("Tasks")).status(tree.overall());
        summary.duration = tree.total_elapsed();
        for leaf in tree.leaves() {
            let mut path = vec![tree.label(leaf)];
            let mut parent = tree.parent(leaf);
            while let Some(id) = parent {
                path.push(tree.label(id));
                parent = tree.parent(id);
            }
            path.reverse();
            let mut item = SummaryItem::new(tree.state(leaf), path.join(" / "));
            item.duration = tree.elapsed(leaf);
            item.detail = tree.get_note(leaf).map(str::to_string);
            summary.items.push(item);
        }
        summary
    }
}
