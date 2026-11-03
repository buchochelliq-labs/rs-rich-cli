//! An interactive diff of two texts: hunks to move between, unchanged runs
//! folded, and a decision on each hunk that a caller can read back.
//!
//! The diff is rs-rich-ext's ([`rich_ext::diff`]): its line engine
//! ([`diff_lines`]), its word diff for the emphasis inside changed lines
//! ([`diff_words`]), its `@@` headers ([`hunk_header`]), its layouts
//! ([`Layout`]) and its theme styles (`diff.added`, `diff.removed`,
//! `diff.hunk`, … with [`rich_ext::diff::STYLES`]' defaults). The node
//! draws row by row, so only the rows in view are rendered and
//! highlighted, and a ten-thousand-line diff scrolls as a short one does.
//!
//! A hunk here is one run of changed lines: what a reviewer accepts or
//! rejects as a whole. Unchanged lines show `context` lines around each
//! hunk; the rest of a run is folded into one row, which Enter (or a
//! click) opens.

use std::cell::RefCell;
use std::collections::BTreeSet;
use std::ops::Range;
use std::rc::Rc;

use rich::protocol::ConsoleCodeHighlighting;
use rich::{Console, Segment, Style, Text};
use rich_ext::diff::{Layout, Op, diff_lines, diff_words, hunk_header};
use rich_interact::{Button, Key, KeyCode, MouseKind};

use crate::app::Ctx;
use crate::node::{Axis, Node};
use crate::reactive::{Signal, signal};
use crate::widget::{Canvas, DrawCx, EventCx, MeasureCx, MouseExt, Used, Widget, WidgetEvent};

/// How many rows a turn of the wheel scrolls.
const WHEEL_STEP: isize = 3;

/// What was decided about one hunk of a [`diff_view`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Decision {
    /// Not decided yet: [`accepted_text`] keeps the old lines.
    #[default]
    Pending,
    /// Take the new lines.
    Accepted,
    /// Keep the old lines.
    Rejected,
}

/// What a diff view calls when the last pending hunk is decided.
type Done = Rc<dyn Fn(&[Decision], &mut Ctx)>;

/// The options of a [`diff_view_with`]: its layout, highlighting, context,
/// and where its decisions go.
#[derive(Clone)]
#[must_use = "options do nothing until they are passed to what takes them"]
pub struct DiffOptions {
    layout: Layout,
    side_by_side_at: Option<u16>,
    language: Option<String>,
    context: usize,
    line_numbers: bool,
    decisions: Option<Signal<Vec<Decision>>>,
    cursor: Option<Signal<Option<usize>>>,
    on_done: Option<Done>,
}

impl Default for DiffOptions {
    fn default() -> DiffOptions {
        DiffOptions::new()
    }
}

impl std::fmt::Debug for DiffOptions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DiffOptions")
            .field("layout", &self.layout)
            .field("side_by_side_at", &self.side_by_side_at)
            .field("language", &self.language)
            .field("context", &self.context)
            .field("line_numbers", &self.line_numbers)
            .field("decisions", &self.decisions.is_some())
            .field("on_done", &self.on_done.is_some())
            .finish_non_exhaustive()
    }
}

impl DiffOptions {
    /// Unified, unhighlighted, three lines of context, line numbers, and
    /// no decisions.
    pub fn new() -> DiffOptions {
        DiffOptions {
            layout: Layout::Unified,
            side_by_side_at: None,
            language: None,
            context: 3,
            line_numbers: true,
            decisions: None,
            cursor: None,
            on_done: None,
        }
    }

    /// Unified (removed lines above the added ones) or side by side.
    pub fn layout(mut self, layout: Layout) -> DiffOptions {
        self.layout = layout;
        self
    }

    /// Side by side while the view is at least `columns` wide, and the
    /// [layout](Self::layout) below that.
    pub fn side_by_side_at(mut self, columns: u16) -> DiffOptions {
        self.side_by_side_at = Some(columns);
        self
    }

    /// Highlight both sides as `language` (`"rust"`, `"py"`), through the
    /// console's code highlighter (syntect, or lumis when registered).
    pub fn language(mut self, language: impl Into<String>) -> DiffOptions {
        self.language = Some(language.into());
        self
    }

    /// Unchanged lines shown around each hunk (default 3); the rest of a
    /// run of them is folded.
    pub fn context(mut self, lines: usize) -> DiffOptions {
        self.context = lines;
        self
    }

    /// Show the old and new line numbers (default on).
    pub fn line_numbers(mut self, on: bool) -> DiffOptions {
        self.line_numbers = on;
        self
    }

    /// Decide on each hunk into `decisions`, one per hunk: `a` and `r`
    /// accept or reject the hunk under the cursor, `A` and `R` all of
    /// them, and a status row counts what is decided. The view keeps the
    /// signal as long as the hunks, and sets every hunk back to
    /// [`Pending`](Decision::Pending) when either text changes.
    pub fn decide(mut self, decisions: Signal<Vec<Decision>>) -> DiffOptions {
        self.decisions = Some(decisions);
        self
    }

    /// Keep the hunk cursor in `cursor`: `None` before the first `]`, or
    /// the hunk it is on. Set it to move it.
    pub fn cursor(mut self, cursor: Signal<Option<usize>>) -> DiffOptions {
        self.cursor = Some(cursor);
        self
    }

    /// Call `done` with the decisions when the last pending hunk is
    /// decided (and again whenever a decision changes after that).
    pub fn on_done(mut self, done: impl Fn(&[Decision], &mut Ctx) + 'static) -> DiffOptions {
        self.on_done = Some(Rc::new(done));
        self
    }
}

/// One hunk: a run of changed lines, as line ranges of each side.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Change {
    old: Range<usize>,
    new: Range<usize>,
}

/// The diff of two texts, as the view and [`accepted_text`] read it.
struct Model {
    old_src: String,
    new_src: String,
    /// Lines without their terminators, for showing.
    old: Vec<String>,
    new: Vec<String>,
    changes: Vec<Change>,
}

/// Lines that keep their terminators: a missing final newline is a change.
fn lines(text: &str) -> Vec<&str> {
    text.split_inclusive('\n').collect()
}

fn strip_eol(line: &str) -> &str {
    let line = line.strip_suffix('\n').unwrap_or(line);
    line.strip_suffix('\r').unwrap_or(line)
}

/// The runs of changed lines in `ops`, each a hunk.
fn changes(ops: &[Op]) -> Vec<Change> {
    let mut out: Vec<Change> = Vec::new();
    let mut open: Option<Change> = None;
    for op in ops {
        if op.is_equal() {
            out.extend(open.take());
            continue;
        }
        let (old, new) = (op.old(), op.new_range());
        open = Some(match open.take() {
            Some(change) => Change {
                old: change.old.start..old.end,
                new: change.new.start..new.end,
            },
            None => Change { old, new },
        });
    }
    out.extend(open);
    out
}

impl Model {
    fn new(old: &str, new: &str) -> Model {
        let (a, b) = (lines(old), lines(new));
        let changes = changes(&diff_lines(&a, &b));
        Model {
            old_src: old.to_string(),
            new_src: new.to_string(),
            old: a.iter().map(|l| strip_eol(l).to_string()).collect(),
            new: b.iter().map(|l| strip_eol(l).to_string()).collect(),
            changes,
        }
    }

    /// The equal lines before hunk `gap` (after the last, for the last
    /// gap): their old range and where they start on the new side.
    fn gap(&self, gap: usize) -> (Range<usize>, usize) {
        let (start, new_start) = match gap.checked_sub(1).and_then(|i| self.changes.get(i)) {
            Some(change) => (change.old.end, change.new.end),
            None => (0, 0),
        };
        let end = self
            .changes
            .get(gap)
            .map_or(self.old.len(), |change| change.old.start);
        (start..end.max(start), new_start)
    }

    /// The lines of gap `gap` shown after the hunk before it, folded, and
    /// shown before the hunk after it, at `context`.
    fn gap_parts(&self, gap: usize, context: usize) -> (usize, usize, usize) {
        let len = self.gap(gap).0.len();
        let after = if gap == 0 { 0 } else { context.min(len) };
        let before = if gap == self.changes.len() {
            0
        } else {
            context.min(len - after)
        };
        (after, len - after - before, before)
    }
}

/// The text with the [accepted](Decision::Accepted) hunks of the diff of
/// `old` against `new` applied, and every other hunk left as it was: what
/// an app writes back after a review. `decisions` has one entry per hunk,
/// in order (as a [`diff_view`] keeps them); a missing one is pending.
///
/// ```
/// use intuituive::widgets::{Decision, accepted_text};
///
/// let (old, new) = ("a\nb\nc\nd\n", "A\nb\nc\nD\n");
/// let picked = [Decision::Accepted, Decision::Rejected];
/// assert_eq!(accepted_text(old, new, &picked), "A\nb\nc\nd\n");
/// assert_eq!(accepted_text(old, new, &[]), old);
/// ```
pub fn accepted_text(old: &str, new: &str, decisions: &[Decision]) -> String {
    let (a, b) = (lines(old), lines(new));
    let changes = changes(&diff_lines(&a, &b));
    let mut out = String::with_capacity(old.len().max(new.len()));
    let mut at = 0;
    for (hunk, change) in changes.iter().enumerate() {
        out.extend(a[at..change.old.start].iter().copied());
        match decisions.get(hunk) {
            Some(Decision::Accepted) => out.extend(b[change.new.clone()].iter().copied()),
            _ => out.extend(a[change.old.clone()].iter().copied()),
        }
        at = change.old.end;
    }
    out.extend(a[at..].iter().copied());
    out
}

/// What a line is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Context,
    Delete,
    Insert,
}

impl Kind {
    fn marker(self) -> &'static str {
        match self {
            Kind::Context => " ",
            Kind::Delete => "-",
            Kind::Insert => "+",
        }
    }
}

/// Which side a line is on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Side {
    Old,
    New,
}

/// One row of the view.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Row {
    /// Hunk `0`'s `@@` header.
    Header(usize),
    /// A line, unified: its kind, old and new line, and its hunk.
    Line {
        kind: Kind,
        old: Option<usize>,
        new: Option<usize>,
        hunk: Option<usize>,
    },
    /// A row side by side: the old line on the left, the new on the right.
    Pair {
        left: Option<(Kind, usize)>,
        right: Option<(Kind, usize)>,
        hunk: Option<usize>,
    },
    /// Unchanged lines folded away: gap `gap`, `lines` of them.
    Fold { gap: usize, lines: usize },
}

impl Row {
    fn hunk(self) -> Option<usize> {
        match self {
            Row::Header(hunk) => Some(hunk),
            Row::Line { hunk, .. } | Row::Pair { hunk, .. } => hunk,
            Row::Fold { .. } => None,
        }
    }
}

fn build_rows(model: &Model, context: usize, open: &BTreeSet<usize>, sbs: bool) -> Vec<Row> {
    let mut rows = Vec::new();
    let n = model.changes.len();
    if n == 0 {
        return rows;
    }
    let context_rows = |rows: &mut Vec<Row>, gap: usize, lines: Range<usize>, hunk| {
        let (old, new_start) = model.gap(gap);
        for k in lines {
            let j = new_start + (k - old.start);
            rows.push(if sbs {
                Row::Pair {
                    left: Some((Kind::Context, k)),
                    right: Some((Kind::Context, j)),
                    hunk,
                }
            } else {
                Row::Line {
                    kind: Kind::Context,
                    old: Some(k),
                    new: Some(j),
                    hunk,
                }
            });
        }
    };
    for gap in 0..=n {
        let (range, _) = model.gap(gap);
        let (after, hidden, before) = model.gap_parts(gap, context);
        let s = range.start;
        context_rows(&mut rows, gap, s..s + after, gap.checked_sub(1));
        if hidden > 0 {
            if open.contains(&gap) {
                context_rows(&mut rows, gap, s + after..s + after + hidden, None);
            } else {
                rows.push(Row::Fold { gap, lines: hidden });
            }
        }
        let Some(change) = model.changes.get(gap) else {
            break;
        };
        rows.push(Row::Header(gap));
        context_rows(&mut rows, gap, range.end - before..range.end, Some(gap));
        let hunk = Some(gap);
        if sbs {
            let (dl, il) = (change.old.len(), change.new.len());
            for j in 0..dl.max(il) {
                rows.push(Row::Pair {
                    left: (j < dl).then(|| (Kind::Delete, change.old.start + j)),
                    right: (j < il).then(|| (Kind::Insert, change.new.start + j)),
                    hunk,
                });
            }
        } else {
            for k in change.old.clone() {
                rows.push(Row::Line {
                    kind: Kind::Delete,
                    old: Some(k),
                    new: None,
                    hunk,
                });
            }
            for k in change.new.clone() {
                rows.push(Row::Line {
                    kind: Kind::Insert,
                    old: None,
                    new: Some(k),
                    hunk,
                });
            }
        }
    }
    rows
}

/// What the view and its bindings share: the texts, the state in signals,
/// and the diff, computed again only when a text changes.
struct Core {
    old: Signal<String>,
    new: Signal<String>,
    options: DiffOptions,
    decisions: Signal<Vec<Decision>>,
    cursor: Signal<Option<usize>>,
    /// The gaps opened by hand.
    open: Signal<BTreeSet<usize>>,
    model: RefCell<Option<Rc<Model>>>,
}

impl Core {
    /// The diff, read untracked. When a text changes after a diff was
    /// shown, the decisions start again, all pending: a decision belongs to
    /// the hunk it was made on, not to its position.
    fn model(&self) -> Rc<Model> {
        let (model, replaced) = {
            let mut cached = self.model.borrow_mut();
            let fresh = cached.as_ref().is_some_and(|model| {
                self.old.with_untracked(|old| *old == model.old_src)
                    && self.new.with_untracked(|new| *new == model.new_src)
            });
            let replaced = !fresh && cached.is_some();
            if !fresh {
                let model = self
                    .old
                    .with_untracked(|old| self.new.with_untracked(|new| Model::new(old, new)));
                *cached = Some(Rc::new(model));
            }
            (cached.clone().expect("just set"), replaced)
        };
        if replaced {
            self.decisions
                .set(vec![Decision::Pending; model.changes.len()]);
        }
        model
    }

    /// The decisions, as many as the hunks.
    fn decisions_now(&self, hunks: usize) -> Vec<Decision> {
        let mut decisions = self.decisions.with_untracked(Vec::clone);
        decisions.resize(hunks, Decision::Pending);
        decisions
    }

    fn step(&self, forward: bool) {
        let n = self.model().changes.len();
        if n == 0 {
            return;
        }
        let at = self.cursor.with_untracked(|c| *c);
        let to = match (at, forward) {
            (None, true) => 0,
            (None, false) => n - 1,
            (Some(c), true) => (c + 1).min(n - 1),
            (Some(c), false) => c.saturating_sub(1),
        };
        self.cursor.set(Some(to.min(n - 1)));
    }

    fn decide(&self, cx: &mut Ctx, all: bool, decision: Decision) {
        let n = self.model().changes.len();
        let mut decisions = self.decisions_now(n);
        let before = decisions.clone();
        if all {
            decisions.fill(decision);
        } else if let Some(c) = self.cursor.with_untracked(|c| *c).filter(|&c| c < n) {
            decisions[c] = decision;
        } else {
            return;
        }
        if decisions == before && self.decisions.with_untracked(Vec::len) == n {
            return;
        }
        self.decisions.set(decisions.clone());
        if n > 0
            && !decisions.contains(&Decision::Pending)
            && let Some(done) = &self.options.on_done
        {
            done(&decisions, cx);
        }
    }

    /// Open the folds around the cursor's hunk, or close them when they
    /// are open.
    fn toggle_folds(&self) {
        let model = self.model();
        let context = self.options.context;
        let gaps: Vec<usize> = match self.cursor.with_untracked(|c| *c) {
            Some(c) => vec![c, c + 1],
            None => (0..=model.changes.len()).collect(),
        };
        let foldable: Vec<usize> = gaps
            .into_iter()
            .filter(|&g| g <= model.changes.len() && model.gap_parts(g, context).1 > 0)
            .collect();
        if foldable.is_empty() {
            return;
        }
        self.open.update(|open| {
            if foldable.iter().any(|g| !open.contains(g)) {
                open.extend(foldable.iter().copied());
            } else {
                for g in &foldable {
                    open.remove(g);
                }
            }
        });
    }

    fn toggle_gap(&self, gap: usize) {
        self.open.update(|open| {
            if !open.remove(&gap) {
                open.insert(gap);
            }
        });
    }
}

/// A side's lines in view, highlighted: which lines, and each one's spans.
struct Highlight {
    model: *const Model,
    lines: Range<usize>,
    spans: Vec<Vec<(Range<usize>, Style)>>,
}

struct DiffWidget {
    core: Rc<Core>,
    rows: Vec<Row>,
    /// What the rows were built from: the diff, the open gaps, side by side.
    rows_key: Option<(*const Model, BTreeSet<usize>, bool)>,
    top: usize,
    /// The rows of the view last drawn (the status row left out).
    height: usize,
    /// The cursor at the last draw: a new one is scrolled into view.
    drawn_cursor: Option<Option<usize>>,
    highlight: [Option<Highlight>; 2],
}

/// The styles a view draws in.
struct Looks {
    context: Style,
    removed: Style,
    added: Style,
    removed_em: Style,
    added_em: Style,
    hunk: Style,
    number: Style,
    cursor: Style,
    good: Style,
    bad: Style,
    muted: Style,
}

fn diff_style(cx: &DrawCx, key: &str) -> Style {
    let fallback = rich_ext::diff::STYLES
        .iter()
        .find(|(name, _)| *name == key)
        .map_or("none", |(_, spec)| spec);
    cx.style(key, fallback)
}

impl Looks {
    fn new(cx: &DrawCx) -> Looks {
        Looks {
            context: diff_style(cx, "diff.context"),
            removed: diff_style(cx, "diff.removed"),
            added: diff_style(cx, "diff.added"),
            removed_em: diff_style(cx, "diff.removed.emphasis"),
            added_em: diff_style(cx, "diff.added.emphasis"),
            hunk: diff_style(cx, "diff.hunk"),
            number: diff_style(cx, "diff.line_number"),
            cursor: cx.style("diff.cursor", "bold bright_cyan"),
            good: cx.style("good", "green"),
            bad: cx.style("bad", "bold red"),
            muted: cx.style("muted", "dim"),
        }
    }

    fn kind(&self, kind: Kind) -> &Style {
        match kind {
            Kind::Context => &self.context,
            Kind::Delete => &self.removed,
            Kind::Insert => &self.added,
        }
    }
}

/// One line of `text` at `width` cells, cropped, padded when `pad`.
fn one_line(console: &Console, text: Text, width: usize, pad: bool) -> Vec<Segment> {
    let text = text.no_wrap(true).overflow(rich::Overflow::Crop);
    let options = console.options().update_width(width.max(1));
    console
        .render_lines(&text, &options, pad)
        .into_iter()
        .next()
        .unwrap_or_default()
}

/// The byte ranges of `old` and `new` to emphasise, when they share enough
/// to make it worth it (as rs-rich-ext's views decide).
fn emphasis(old: &str, new: &str) -> (Vec<Range<usize>>, Vec<Range<usize>>) {
    let ops = diff_words(old, new);
    let same: usize = ops
        .iter()
        .filter(|op| op.is_equal())
        .map(|op| op.old().len())
        .sum();
    let total = old.len() + new.len();
    if total == 0 || same * 2 * 10 < total * 4 {
        return (Vec::new(), Vec::new());
    }
    let (mut a, mut b) = (Vec::new(), Vec::new());
    for op in ops.iter().filter(|op| !op.is_equal()) {
        let (o, n) = (op.old(), op.new_range());
        if !o.is_empty() {
            a.push(o);
        }
        if !n.is_empty() {
            b.push(n);
        }
    }
    (a, b)
}

impl DiffWidget {
    fn model_tracked(&self) -> Rc<Model> {
        self.core.old.with(|_| ());
        self.core.new.with(|_| ());
        self.core.model()
    }

    fn side_by_side(&self, width: u16) -> bool {
        let options = &self.core.options;
        match options.side_by_side_at {
            Some(at) if width >= at => true,
            _ => options.layout == Layout::SideBySide,
        }
    }

    fn status_shown(&self) -> bool {
        self.core.options.decisions.is_some()
    }

    fn max_top(&self) -> usize {
        self.rows.len().saturating_sub(self.height.max(1))
    }

    fn scroll_by(&mut self, by: isize) -> Used {
        let to = (self.top as isize + by).clamp(0, self.max_top() as isize) as usize;
        if to == self.top {
            return Used::No;
        }
        self.top = to;
        Used::Yes
    }

    /// Highlight the lines of `side` in `lines`, if the view has a
    /// language and they are not highlighted already.
    fn highlight(&mut self, console: &Console, model: &Rc<Model>, side: Side, lines: Range<usize>) {
        let Some(language) = self.core.options.language.as_deref() else {
            return;
        };
        let slot = &mut self.highlight[side as usize];
        let key = Rc::as_ptr(model);
        if slot
            .as_ref()
            .is_some_and(|h| h.model == key && h.lines == lines)
        {
            return;
        }
        let source = match side {
            Side::Old => &model.old,
            Side::New => &model.new,
        };
        let code = source[lines.clone()].join("\n");
        let (engine, theme) = match console.code_highlighting() {
            Some(chosen) => (chosen.highlighter.clone(), chosen.theme.clone()),
            #[cfg(feature = "syntax")]
            None => (rich::SyntectHighlighter::shared(), None),
            #[cfg(not(feature = "syntax"))]
            None => return,
        };
        let theme = theme.unwrap_or_else(|| engine.default_theme().to_string());
        let Ok(out) = engine.highlight(&code, Some(language), &theme) else {
            return;
        };
        *slot = Some(Highlight {
            model: key,
            lines,
            spans: out
                .lines
                .into_iter()
                .map(|line| {
                    line.spans
                        .into_iter()
                        .map(|span| (span.range, without_background(&span.style)))
                        .collect()
                })
                .collect(),
        });
    }

    /// The text of line `index` of `side`, in the styles of `kind`, its
    /// highlighting and `emphasis`.
    fn content(
        &self,
        model: &Model,
        looks: &Looks,
        side: Side,
        index: usize,
        kind: Kind,
        emphasis: &[Range<usize>],
    ) -> Text {
        let line = match side {
            Side::Old => &model.old[index],
            Side::New => &model.new[index],
        };
        let mut text = Text::new(line.clone());
        let spans = self.highlight[side as usize].as_ref().and_then(|h| {
            h.lines
                .contains(&index)
                .then(|| h.spans.get(index - h.lines.start))
                .flatten()
        });
        match spans {
            Some(spans) => {
                for (range, style) in spans {
                    text.stylize(style.clone(), range.start, range.end.min(line.len()));
                }
            }
            None => text.stylize(looks.kind(kind).clone(), 0, line.len()),
        }
        let em = match kind {
            Kind::Delete => &looks.removed_em,
            _ => &looks.added_em,
        };
        for range in emphasis {
            text.stylize(em.clone(), range.start, range.end.min(line.len()));
        }
        text
    }

    /// The emphasis of a changed line: against its partner on the other
    /// side, the line at the same place in the other half of its hunk.
    fn emphasis_of(&self, model: &Model, side: Side, index: usize) -> Vec<Range<usize>> {
        let change = model.changes.iter().find(|c| match side {
            Side::Old => c.old.contains(&index),
            Side::New => c.new.contains(&index),
        });
        let Some(change) = change else {
            return Vec::new();
        };
        let (own, other) = match side {
            Side::Old => (&change.old, &change.new),
            Side::New => (&change.new, &change.old),
        };
        let j = index - own.start;
        if j >= other.len() {
            return Vec::new();
        }
        let partner = other.start + j;
        match side {
            Side::Old => emphasis(&model.old[index], &model.new[partner]).0,
            Side::New => emphasis(&model.old[partner], &model.new[index]).1,
        }
    }

    fn number(&self, number: Option<usize>, digits: usize) -> String {
        match number {
            Some(n) => format!("{:>digits$}", n + 1),
            None => " ".repeat(digits),
        }
    }

    /// A row's line on one side: its number, marker and text, at `width`.
    #[allow(clippy::too_many_arguments)]
    fn half(
        &self,
        console: &Console,
        model: &Model,
        looks: &Looks,
        side: Side,
        line: Option<(Kind, usize)>,
        digits: usize,
        width: usize,
    ) -> Vec<Segment> {
        let mut text = Text::new("");
        if let Some((kind, index)) = line {
            if self.core.options.line_numbers {
                text.append(
                    &self.number(Some(index), digits),
                    Some(looks.number.clone().into()),
                );
                text.append(" ", None);
            }
            text.append(kind.marker(), Some(looks.kind(kind).clone().into()));
            text.append(" ", None);
            let em = if kind == Kind::Context {
                Vec::new()
            } else {
                self.emphasis_of(model, side, index)
            };
            text = text.append_text(&self.content(model, looks, side, index, kind, &em));
        }
        one_line(console, text, width, true)
    }

    #[allow(clippy::too_many_arguments)]
    fn render_row(
        &self,
        console: &Console,
        model: &Model,
        looks: &Looks,
        row: Row,
        cursor: Option<usize>,
        decisions: &[Decision],
        width: usize,
        digits: usize,
    ) -> Vec<Segment> {
        let marked = cursor.is_some() && row.hunk() == cursor;
        let mut out = vec![if marked {
            Segment::new("▌", Some(looks.cursor.clone()))
        } else {
            Segment::new(" ", None)
        }];
        let rest = width.saturating_sub(1);
        let gutter = if self.core.options.line_numbers {
            digits * 2 + 2
        } else {
            0
        };
        match row {
            Row::Header(hunk) => {
                let change = &model.changes[hunk];
                let mut text = Text::new("");
                let style = if marked { &looks.cursor } else { &looks.hunk };
                text.append(
                    &hunk_header(change.old.clone(), change.new.clone()),
                    Some(style.clone().into()),
                );
                if self.core.options.decisions.is_some() {
                    match decisions.get(hunk).copied().unwrap_or_default() {
                        Decision::Accepted => {
                            text.append("  ✓ accepted", Some(looks.good.clone().into()))
                        }
                        Decision::Rejected => {
                            text.append("  ✗ rejected", Some(looks.bad.clone().into()))
                        }
                        Decision::Pending => {}
                    }
                }
                out.extend(one_line(console, text, rest, false));
            }
            Row::Fold { lines, .. } => {
                let mut text = Text::new(" ".repeat(gutter));
                let what = if lines == 1 { "line" } else { "lines" };
                text.append(
                    &format!("⋯ {lines} unchanged {what}"),
                    Some(looks.number.clone().into()),
                );
                out.extend(one_line(console, text, rest, false));
            }
            Row::Line { kind, old, new, .. } => {
                let mut text = Text::new("");
                if self.core.options.line_numbers {
                    let numbers =
                        format!("{} {} ", self.number(old, digits), self.number(new, digits));
                    text.append(&numbers, Some(looks.number.clone().into()));
                }
                text.append(kind.marker(), Some(looks.kind(kind).clone().into()));
                text.append(" ", None);
                let (side, index) = match (kind, old, new) {
                    (Kind::Insert, _, Some(j)) | (Kind::Context, None, Some(j)) => (Side::New, j),
                    (_, Some(k), _) => (Side::Old, k),
                    _ => (Side::New, 0),
                };
                let em = if kind == Kind::Context {
                    Vec::new()
                } else {
                    self.emphasis_of(model, side, index)
                };
                text = text.append_text(&self.content(model, looks, side, index, kind, &em));
                out.extend(one_line(console, text, rest, false));
            }
            Row::Pair { left, right, .. } => {
                let inner = rest.saturating_sub(3);
                let lw = inner / 2;
                let rw = inner - lw;
                out.extend(self.half(console, model, looks, Side::Old, left, digits, lw));
                out.push(Segment::new(" │ ", Some(looks.number.clone())));
                out.extend(self.half(console, model, looks, Side::New, right, digits, rw));
            }
        }
        out
    }

    fn status(&self, looks: &Looks, cursor: Option<usize>, decisions: &[Decision]) -> Text {
        let n = decisions.len();
        let count = |d| decisions.iter().filter(|&&x| x == d).count();
        let (accepted, rejected) = (count(Decision::Accepted), count(Decision::Rejected));
        let mut text = Text::new("");
        let at = match cursor {
            Some(c) => format!("hunk {} of {n}", c + 1),
            None => format!("{n} hunks"),
        };
        text.append(&at, None);
        text.append(
            &format!(" · {} of {n} decided", accepted + rejected),
            Some(looks.muted.clone().into()),
        );
        if accepted > 0 {
            text.append(
                &format!(" · {accepted} accepted"),
                Some(looks.good.clone().into()),
            );
        }
        if rejected > 0 {
            text.append(
                &format!(" · {rejected} rejected"),
                Some(looks.bad.clone().into()),
            );
        }
        text
    }
}

/// `style` without its background, so the view's own shows through.
fn without_background(style: &Style) -> Style {
    let definition = style.definition();
    let kept = match definition.split_once(" on ") {
        Some((before, _)) => before.to_string(),
        None if definition.starts_with("on ") => String::new(),
        None => definition,
    };
    Style::parse(&kept).unwrap_or_default()
}

impl Widget for DiffWidget {
    fn name(&self) -> &'static str {
        "diff_view"
    }

    fn role(&self) -> crate::a11y::Role {
        crate::a11y::Role::Region
    }

    fn describe(&self) -> Option<String> {
        let model = self.core.model();
        let n = model.changes.len();
        let decisions = self.core.decisions_now(n);
        let decided = decisions
            .iter()
            .filter(|d| **d != Decision::Pending)
            .count();
        Some(format!("diff, {n} hunks, {decided} decided"))
    }

    /// The hunk under the cursor: its header and what was decided.
    fn access_text(&self) -> Option<String> {
        let model = self.core.model();
        let c = self.core.cursor.with_untracked(|c| *c)?;
        let change = model.changes.get(c)?;
        let decision = self.core.decisions_now(model.changes.len())[c];
        Some(format!(
            "hunk {} of {}: {} {:?}",
            c + 1,
            model.changes.len(),
            hunk_header(change.old.clone(), change.new.clone()),
            decision
        ))
    }

    fn measure(&mut self, _cx: &MeasureCx, axis: Axis, width: u16, _height: u16) -> u16 {
        match axis {
            Axis::Vertical => {
                let model = self.model_tracked();
                let open = self.core.open.with(BTreeSet::clone);
                let rows = build_rows(&model, self.core.options.context, &open, false).len();
                let rows = rows.max(1) + usize::from(self.status_shown());
                rows.min(u16::MAX as usize) as u16
            }
            Axis::Horizontal => width,
        }
    }

    fn draw(&mut self, cx: &mut DrawCx, canvas: &mut Canvas) {
        let (width, height) = (canvas.width(), canvas.height() as usize);
        let status = self.status_shown() && height > 1;
        let body = height - usize::from(status);
        self.height = body;
        let model = self.model_tracked();
        let open = self.core.open.with(BTreeSet::clone);
        let cursor = self.core.cursor.get();
        let mut decisions = self.core.decisions.get();
        decisions.resize(model.changes.len(), Decision::Pending);
        let sbs = self.side_by_side(width);
        let key = (Rc::as_ptr(&model), open, sbs);
        if self.rows_key.as_ref() != Some(&key) {
            self.rows = build_rows(&model, self.core.options.context, &key.1, sbs);
            self.rows_key = Some(key);
        }
        // A cursor moved by a key (not a click) brings its hunk into view.
        if self.drawn_cursor.is_some_and(|drawn| drawn != cursor)
            && let Some(c) = cursor
            && let Some(at) = self.rows.iter().position(|r| *r == Row::Header(c))
        {
            let end = self.rows[at + 1..]
                .iter()
                .position(|r| r.hunk() != Some(c))
                .map_or(self.rows.len(), |i| at + 1 + i);
            if at < self.top || end > self.top + body {
                self.top = at.saturating_sub(1);
            }
        }
        self.drawn_cursor = Some(cursor);
        self.top = self.top.min(self.max_top());
        let console = cx.console();
        let looks = Looks::new(cx);
        let mut lines: Vec<Vec<Segment>> = Vec::with_capacity(height);
        if self.rows.is_empty() {
            let text = Text::new("no differences");
            lines.push(one_line(
                console,
                Text::styled(text.plain().to_string(), looks.number.clone()),
                width as usize,
                false,
            ));
        } else {
            let view = self.top..(self.top + body).min(self.rows.len());
            // The lines of each side in view, highlighted at once.
            for side in [Side::Old, Side::New] {
                let mut range: Option<Range<usize>> = None;
                for row in &self.rows[view.clone()] {
                    let index = match (*row, side) {
                        (Row::Line { old: Some(k), .. }, Side::Old) => Some(k),
                        (Row::Line { new: Some(j), .. }, Side::New) => Some(j),
                        (
                            Row::Pair {
                                left: Some((_, k)), ..
                            },
                            Side::Old,
                        ) => Some(k),
                        (
                            Row::Pair {
                                right: Some((_, j)),
                                ..
                            },
                            Side::New,
                        ) => Some(j),
                        _ => None,
                    };
                    if let Some(i) = index {
                        range = Some(match range {
                            Some(r) => r.start.min(i)..r.end.max(i + 1),
                            None => i..i + 1,
                        });
                    }
                }
                if let Some(range) = range {
                    self.highlight(console, &model, side, range);
                }
            }
            let digits = model
                .old
                .len()
                .max(model.new.len())
                .max(1)
                .to_string()
                .len();
            for row in &self.rows[view] {
                lines.push(self.render_row(
                    console,
                    &model,
                    &looks,
                    *row,
                    cursor,
                    &decisions,
                    width as usize,
                    digits,
                ));
            }
        }
        lines.truncate(body);
        while lines.len() < body {
            lines.push(Vec::new());
        }
        if status {
            let text = self.status(&looks, cursor, &decisions);
            lines.push(one_line(console, text, width as usize, false));
        }
        canvas.lines(&lines);
    }

    fn event(&mut self, cx: &mut EventCx, event: &WidgetEvent) -> Used {
        let used = match event {
            WidgetEvent::Key(key) => self.key(key),
            WidgetEvent::Mouse(mouse) => match mouse.kind {
                MouseKind::Down(Button::Left) => {
                    let at = self.top + mouse.row as usize;
                    match (mouse.row as usize) < self.height {
                        true => match self.rows.get(at).copied() {
                            Some(Row::Fold { gap, .. }) => {
                                self.core.toggle_gap(gap);
                                Used::Yes
                            }
                            Some(row) => match row.hunk() {
                                Some(h) => {
                                    // Clicked, the hunk is in view already.
                                    self.drawn_cursor = Some(Some(h));
                                    self.core.cursor.set(Some(h));
                                    Used::Yes
                                }
                                None => Used::No,
                            },
                            None => Used::No,
                        },
                        false => Used::No,
                    }
                }
                _ => match mouse.wheel() {
                    0 => Used::No,
                    way => {
                        self.scroll_by(way as isize * WHEEL_STEP);
                        Used::Yes
                    }
                },
            },
            _ => Used::No,
        };
        if used == Used::Yes {
            cx.redraw();
        }
        used
    }

    fn focusable(&self) -> bool {
        true
    }
}

impl DiffWidget {
    fn key(&mut self, key: &Key) -> Used {
        if key.modifiers.ctrl || key.modifiers.alt {
            return Used::No;
        }
        let page = self.height.saturating_sub(1).max(1) as isize;
        let letter = |c: char| key.code == KeyCode::Char(c);
        match key.code {
            KeyCode::Up => self.scroll_by(-1),
            KeyCode::Down => self.scroll_by(1),
            _ if letter('k') => self.scroll_by(-1),
            _ if letter('j') => self.scroll_by(1),
            KeyCode::PageUp => self.scroll_by(-page),
            KeyCode::PageDown => self.scroll_by(page),
            KeyCode::Home => self.scroll_by(-(self.top as isize)),
            _ if letter('g') => self.scroll_by(-(self.top as isize)),
            KeyCode::End => self.scroll_by(self.max_top() as isize),
            _ if letter('G') => self.scroll_by(self.max_top() as isize),
            _ => Used::No,
        }
    }
}

/// An interactive diff of `old` against `new` (read again when either
/// changes): see [`diff_view_with`]. Unified, with three lines of context
/// and no decisions.
///
/// ```
/// use intuituive::prelude::*;
/// use intuituive::widgets::diff_view;
///
/// let app = App::new(|| diff_view(signal("a\nb\n".to_string()), signal("a\nc\n".to_string())));
/// let mut driver = app.driver(30, 4);
/// driver.settle();
/// let screen = driver.screen().plain();
/// assert_eq!(screen[0].trim_end(), " @@ -2 +2 @@");
/// assert_eq!(screen[1].trim_end(), " 1 1   a");
/// assert_eq!(screen[2].trim_end(), " 2   - b");
/// assert_eq!(screen[3].trim_end(), "   2 + c");
/// ```
pub fn diff_view(old: Signal<String>, new: Signal<String>) -> Node {
    diff_view_with(old, new, DiffOptions::new())
}

/// An interactive diff of `old` against `new`, rs-rich-ext's line diff
/// drawn a row at a time: hunks with their `@@` headers, unchanged runs
/// folded to `context` lines around each hunk (Enter opens the folds
/// around the hunk under the cursor, a click opens one), word-level
/// emphasis inside changed lines, and highlighting with
/// [`DiffOptions::language`]. Unified, or side by side
/// ([`DiffOptions::layout`], or by width with
/// [`DiffOptions::side_by_side_at`]).
///
/// Its keys (listed in the help, under `diff`): `]` or `n` and `[` or `p`
/// move the hunk cursor (it starts before the first hunk), and with
/// [`DiffOptions::decide`] `a` and `r` accept or reject the hunk under
/// it, `A` and `R` every hunk. Their ids, for a
/// [keymap](crate::App::keymap): `diff.next`, `diff.previous`,
/// `diff.fold`, `diff.accept`, `diff.reject`, `diff.accept_all` and
/// `diff.reject_all`. Up, Down, `j`, `k`, PageUp, PageDown,
/// Home and End scroll, and so does the wheel; a click puts the cursor on
/// a hunk. Only the rows in view are rendered. [`accepted_text`] applies
/// the decisions.
///
/// ```
/// use intuituive::prelude::*;
/// use intuituive::widgets::{DiffOptions, Decision, accepted_text, diff_view_with};
///
/// let app = App::new(|| {
///     let (old, new) = (signal("a\nb\n".to_string()), signal("A\nb\nC\n".to_string()));
///     let decisions = signal(Vec::<Decision>::new());
///     let options = DiffOptions::new().decide(decisions).on_done(move |decisions, cx| {
///         let text = accepted_text(&old.get(), &new.get(), decisions);
///         assert_eq!(text, "A\nb\n");
///         cx.quit();
///     });
///     diff_view_with(old, new, options)
/// });
/// let mut driver = app.driver(50, 8);
/// driver.press("] a");
/// let screen = driver.screen().plain();
/// assert_eq!(screen[7].trim_end(), "hunk 1 of 2 · 1 of 2 decided · 1 accepted");
/// driver.press("] r");
/// assert!(driver.is_done());
/// ```
pub fn diff_view_with(old: Signal<String>, new: Signal<String>, options: DiffOptions) -> Node {
    let decide = options.decisions.is_some();
    let core = Rc::new(Core {
        old,
        new,
        decisions: options.decisions.unwrap_or_else(|| signal(Vec::new())),
        cursor: options.cursor.unwrap_or_else(|| signal(None)),
        open: signal(BTreeSet::new()),
        options,
        model: RefCell::new(None),
    });
    let widget = DiffWidget {
        core: core.clone(),
        rows: Vec::new(),
        rows_key: None,
        top: 0,
        height: 0,
        drawn_cursor: None,
        highlight: [None, None],
    };
    let mut node = Node::from_widget(Box::new(widget), "diff_view").focusable();
    let c = core.clone();
    node = node
        .bind_in("diff", "] n", "Next hunk", move |_| c.step(true))
        .binding_id("diff.next");
    let c = core.clone();
    node = node
        .bind_in("diff", "[ p", "Previous hunk", move |_| c.step(false))
        .binding_id("diff.previous");
    let c = core.clone();
    node = node
        .bind_in(
            "diff",
            "enter",
            "Open or fold the unchanged lines",
            move |_| c.toggle_folds(),
        )
        .binding_id("diff.fold");
    if decide {
        let c = core.clone();
        node = node
            .bind_in("diff", "a", "Accept the hunk", move |cx| {
                c.decide(cx, false, Decision::Accepted)
            })
            .binding_id("diff.accept");
        let c = core.clone();
        node = node
            .bind_in("diff", "r", "Reject the hunk", move |cx| {
                c.decide(cx, false, Decision::Rejected)
            })
            .binding_id("diff.reject");
        let c = core.clone();
        node = node
            .bind_in("diff", "A", "Accept every hunk", move |cx| {
                c.decide(cx, true, Decision::Accepted)
            })
            .binding_id("diff.accept_all");
        let c = core;
        node = node
            .bind_in("diff", "R", "Reject every hunk", move |cx| {
                c.decide(cx, true, Decision::Rejected)
            })
            .binding_id("diff.reject_all");
    }
    node
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn changes_merge_adjacent_runs() {
        let model = Model::new("a\nb\nc\n", "a\nB\nC\nc\nd\n");
        assert_eq!(
            model.changes,
            [
                Change {
                    old: 1..2,
                    new: 1..3
                },
                Change {
                    old: 3..3,
                    new: 4..5
                }
            ]
        );
    }

    #[test]
    fn accepted_text_at_both_ends() {
        let (old, new) = ("x\na\nb\ny\n", "a\nb\nz");
        // Hunks: delete `x`; `y\n` -> `z` (no final newline).
        assert_eq!(
            accepted_text(old, new, &[Decision::Accepted, Decision::Accepted]),
            new
        );
        assert_eq!(
            accepted_text(old, new, &[Decision::Accepted, Decision::Rejected]),
            "a\nb\ny\n"
        );
        assert_eq!(
            accepted_text(old, new, &[Decision::Rejected, Decision::Accepted]),
            "x\na\nb\nz"
        );
        assert_eq!(accepted_text("", "new\n", &[Decision::Accepted]), "new\n");
        assert_eq!(accepted_text("old\n", "", &[Decision::Accepted]), "");
    }

    #[test]
    fn folds_leave_context() {
        let old: String = (1..=20).map(|i| format!("{i}\n")).collect();
        let new = old.replace("10\n", "ten\n");
        let model = Model::new(&old, &new);
        let rows = build_rows(&model, 2, &BTreeSet::new(), false);
        assert_eq!(rows[0], Row::Fold { gap: 0, lines: 7 });
        assert_eq!(rows[1], Row::Header(0));
        assert_eq!(rows.last(), Some(&Row::Fold { gap: 1, lines: 8 }));
        let open = build_rows(&model, 2, &BTreeSet::from([0, 1]), false);
        assert_eq!(open.len(), 1 + 19 + 2);
    }
}
