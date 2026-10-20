//! An approval queue (#471): review pending items one by one or in bulk,
//! and approve, reject or skip each, with a reason when one is wanted.
//!
//! The queue is a list of items, each with its decision mark (`·` pending,
//! `✓` approved, `✗` rejected, `↷` skipped), and a detail pane for the
//! focused item: its label, description, details and decision, then its
//! [`Preview`](crate::Preview). `a`, `r` and `s` decide the focused item and
//! move on to the next pending one; `A`, `R` and `S` first ask for a
//! reason. Space marks items, Ctrl+A marks every pending one, and a
//! decision then applies to all the marked items at once. `u` takes a
//! decision back. Enter submits the [`ApprovalReport`]: every item with its
//! decision and reason, pending ones included; Escape cancels. The queue
//! only records decisions: what they lead to is the caller's.
//!
//! With the mouse on, a click focuses a row, a click on its mark column
//! marks it, the wheel moves, and the buttons under the list are buttons.
//!
//! Decisions are drawn in the console theme's `approve.approved`,
//! `approve.rejected`, `approve.skipped` and `approve.pending` styles
//! (green, red, yellow and dim unless the theme sets them), so a theme can
//! restyle them without code.
//!
//! Without a terminal it asks item by item: `a`, `r` or `s` (or `yes`,
//! `no`, `skip`), with a reason after a colon (`r: not this week`); an
//! empty line skips, and `q` or the end of input leaves the rest pending.
//!
//! ```
//! use rich_interact::components::{ApprovalQueue, Verdict};
//! use rich_interact::{headless, Outcome};
//!
//! let queue = ApprovalQueue::new("Deploys", ["api", "web", "db"]);
//! // Approve api, reject web with a reason, skip db, submit.
//! let script = headless::Script::new().keys("a R").text("flaky").keys("enter s enter");
//! let (outcome, _) = headless::run(queue, script, 80, 12);
//! let Outcome::Done(report) = outcome.unwrap() else { panic!() };
//! assert_eq!(report.verdicts(), [Verdict::Approved, Verdict::Rejected, Verdict::Skipped]);
//! assert_eq!(report.decisions[1].reason.as_deref(), Some("flaky"));
//! ```

use std::cell::Cell;

use rich::{Segment, Style};

use crate::component::{Component, Context, Flow, View};
use crate::components::{fit, pad, pasted, plain, question, text, PreviewLayout, Theme};
use crate::event::{Button, Event, KeyCode, MouseKind};
use crate::item::Item;
use crate::keymap::{keys, Keymap};
use crate::kit::{ListState, TextBuffer};
use crate::policy::{LineIo, NotInteractive};

/// What was decided about one item.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Verdict {
    /// Not decided yet.
    #[default]
    Pending,
    Approved,
    Rejected,
    Skipped,
}

impl Verdict {
    /// `pending`, `approved`, `rejected` or `skipped`.
    pub fn as_str(self) -> &'static str {
        match self {
            Verdict::Pending => "pending",
            Verdict::Approved => "approved",
            Verdict::Rejected => "rejected",
            Verdict::Skipped => "skipped",
        }
    }

    /// The name [`as_str`](Self::as_str) gives, or the verb (`approve`,
    /// `reject`, `skip`).
    pub fn parse(name: &str) -> Option<Verdict> {
        Some(match name {
            "pending" => Verdict::Pending,
            "approved" | "approve" => Verdict::Approved,
            "rejected" | "reject" => Verdict::Rejected,
            "skipped" | "skip" => Verdict::Skipped,
            _ => return None,
        })
    }

    /// The mark a row shows: one cell that reads without colour.
    pub fn mark(self) -> &'static str {
        match self {
            Verdict::Pending => "·",
            Verdict::Approved => "✓",
            Verdict::Rejected => "✗",
            Verdict::Skipped => "↷",
        }
    }

    /// The console theme key its style is read from (`approve.approved`).
    pub fn theme_key(self) -> &'static str {
        match self {
            Verdict::Pending => "approve.pending",
            Verdict::Approved => "approve.approved",
            Verdict::Rejected => "approve.rejected",
            Verdict::Skipped => "approve.skipped",
        }
    }

    /// The style used when the theme has no [`theme_key`](Self::theme_key).
    fn default_style(self) -> &'static str {
        match self {
            Verdict::Pending => "dim",
            Verdict::Approved => "bold green",
            Verdict::Rejected => "bold red",
            Verdict::Skipped => "yellow",
        }
    }

    /// The verb, for a prompt: "approve".
    fn verb(self) -> &'static str {
        match self {
            Verdict::Pending => "undo",
            Verdict::Approved => "approve",
            Verdict::Rejected => "reject",
            Verdict::Skipped => "skip",
        }
    }
}

impl std::fmt::Display for Verdict {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// One item and what was decided about it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Decision<T> {
    /// The item's index in the queue.
    pub index: usize,
    pub value: T,
    pub label: String,
    pub verdict: Verdict,
    pub reason: Option<String>,
}

/// What a review ended with: one [`Decision`] for every item, in the
/// queue's order, pending ones included.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ApprovalReport<T> {
    pub decisions: Vec<Decision<T>>,
}

impl<T> ApprovalReport<T> {
    /// Each item's verdict, in order.
    pub fn verdicts(&self) -> Vec<Verdict> {
        self.decisions.iter().map(|d| d.verdict).collect()
    }

    /// The decisions with `verdict`.
    pub fn with(&self, verdict: Verdict) -> impl Iterator<Item = &Decision<T>> {
        self.decisions.iter().filter(move |d| d.verdict == verdict)
    }

    /// The approved items' values.
    pub fn approved(&self) -> Vec<&T> {
        self.with(Verdict::Approved).map(|d| &d.value).collect()
    }

    /// The rejected items' values.
    pub fn rejected(&self) -> Vec<&T> {
        self.with(Verdict::Rejected).map(|d| &d.value).collect()
    }

    /// How many items have `verdict`.
    pub fn count(&self, verdict: Verdict) -> usize {
        self.with(verdict).count()
    }

    /// `2 approved · 1 rejected · 0 skipped · 3 pending`.
    pub fn summary(&self) -> String {
        summary(&self.verdicts())
    }
}

fn summary(verdicts: &[Verdict]) -> String {
    let count = |verdict| verdicts.iter().filter(|v| **v == verdict).count();
    format!(
        "{} approved · {} rejected · {} skipped · {} pending",
        count(Verdict::Approved),
        count(Verdict::Rejected),
        count(Verdict::Skipped),
        count(Verdict::Pending)
    )
}

/// The keys of an [`ApprovalQueue`], in context `approve`.
pub fn approve_keymap() -> Keymap {
    Keymap::new("approve")
        .bind("approve", keys("a"), "approve")
        .bind("reject", keys("r"), "reject")
        .bind("skip", keys("s"), "skip")
        .bind("approve-reason", keys("A"), "approve, with a reason")
        .bind("reject-reason", keys("R"), "reject, with a reason")
        .bind("skip-reason", keys("S"), "skip, with a reason")
        .bind("undo", keys("u"), "back to pending")
        .bind("mark", keys("space"), "mark for a bulk decision")
        .bind("mark-all", keys("ctrl+a"), "mark every pending item")
        .bind("next-pending", keys("n tab"), "next pending item")
        .bind("up", keys("up k ctrl+p"), "move up")
        .bind("down", keys("down j ctrl+n"), "move down")
        .bind("page-up", keys("pageup"), "page up")
        .bind("page-down", keys("pagedown"), "page down")
        .bind("first", keys("home g"), "first")
        .bind("last", keys("end G"), "last")
        .bind("preview", keys("p"), "show or hide the details")
        .bind("submit", keys("enter"), "submit the decisions")
        .bind("cancel", keys("escape"), "cancel")
}

/// The buttons under the list: label and action, in order.
const BUTTONS: [(&str, &str); 5] = [
    ("[a] approve", "approve"),
    ("[r] reject", "reject"),
    ("[s] skip", "skip"),
    ("[u] undo", "undo"),
    ("[enter] submit", "submit"),
];

/// A reason being typed: for which decision and which items.
struct ReasonEditor {
    verdict: Verdict,
    targets: Vec<usize>,
    buffer: TextBuffer,
    error: Option<String>,
}

/// A queue of pending items to approve, reject or skip. Returns an
/// [`ApprovalReport`].
#[must_use = "a component does nothing until it is run or mounted"]
pub struct ApprovalQueue<T> {
    prompt: String,
    items: Vec<Item<T>>,
    verdicts: Vec<Verdict>,
    reasons: Vec<Option<String>>,
    /// The cursor, the scroll, and the marks for a bulk decision.
    list: ListState,
    height: usize,
    preview: PreviewLayout,
    preview_height: usize,
    theme: Theme,
    mouse: bool,
    keymap: Keymap,
    /// Decisions that cannot be made without a reason.
    required: Vec<Verdict>,
    /// After a decision, move to the next pending item.
    advance: bool,
    /// Submit by itself once nothing is pending.
    auto_submit: bool,
    reason: Option<ReasonEditor>,
    /// A line in place of the buttons until the next key.
    status: Option<String>,
    /// Once finished: the answer shown in place of the queue.
    answer: Option<Option<String>>,
    /// Rows on screen, from the last context.
    space: Cell<usize>,
}

impl<T> ApprovalQueue<T> {
    pub fn new<I, V>(prompt: impl Into<String>, items: I) -> ApprovalQueue<T>
    where
        I: IntoIterator<Item = V>,
        V: Into<Item<T>>,
    {
        let items: Vec<Item<T>> = items.into_iter().map(Into::into).collect();
        let mut list = ListState::new();
        list.set_len(items.len());
        list.clear_selection(items.len());
        ApprovalQueue {
            prompt: prompt.into(),
            verdicts: vec![Verdict::Pending; items.len()],
            reasons: vec![None; items.len()],
            items,
            list,
            height: 10,
            preview: PreviewLayout::Auto,
            preview_height: 8,
            theme: Theme::default(),
            mouse: false,
            keymap: approve_keymap(),
            required: Vec::new(),
            advance: true,
            auto_submit: false,
            reason: None,
            status: None,
            answer: None,
            space: Cell::new(usize::MAX),
        }
    }

    /// Show at most `rows` items at once (default 10).
    pub fn height(mut self, rows: usize) -> Self {
        self.height = rows.max(1);
        self
    }

    /// Where the detail pane goes (default: beside the list from 72
    /// columns, below it when narrower); `Hidden` hides it.
    pub fn preview(mut self, layout: PreviewLayout) -> Self {
        self.preview = layout;
        self
    }

    /// Rows the detail pane takes when it is below the list (default 8).
    pub fn preview_height(mut self, rows: usize) -> Self {
        self.preview_height = rows.max(1);
        self
    }

    pub fn theme(mut self, theme: Theme) -> Self {
        self.theme = theme;
        self
    }

    /// Report the mouse (#476): rows, the mark column and the buttons
    /// click, and the wheel moves.
    pub fn with_mouse(mut self, on: bool) -> Self {
        self.mouse = on;
        self
    }

    /// Make `keys` do `action` (see [`approve_keymap`]) on this queue
    /// only; no keys unbinds it.
    pub fn rebind(
        mut self,
        action: &str,
        keys: impl IntoIterator<Item = crate::event::Key>,
    ) -> Self {
        self.keymap.rebind(action, keys);
        self
    }

    /// Ask for a reason whenever an item gets `verdict` (a rejection, say),
    /// and refuse an empty one.
    pub fn require_reason(mut self, verdict: Verdict) -> Self {
        if verdict != Verdict::Pending && !self.required.contains(&verdict) {
            self.required.push(verdict);
        }
        self
    }

    /// Whether a decision moves the cursor on to the next pending item
    /// (default on).
    pub fn advance(mut self, on: bool) -> Self {
        self.advance = on;
        self
    }

    /// Submit by itself as soon as nothing is pending (default off: Enter
    /// submits).
    pub fn auto_submit(mut self, on: bool) -> Self {
        self.auto_submit = on;
        self
    }

    /// Start item `index` decided: a review picked up where it was left.
    pub fn decided(mut self, index: usize, verdict: Verdict, reason: Option<String>) -> Self {
        if index < self.items.len() {
            self.verdicts[index] = verdict;
            self.reasons[index] = reason;
        }
        self
    }

    pub fn items(&self) -> &[Item<T>] {
        &self.items
    }

    /// Each item's verdict so far.
    pub fn verdicts(&self) -> &[Verdict] {
        &self.verdicts
    }

    /// Item `index`'s reason, if it has one.
    pub fn reason(&self, index: usize) -> Option<&str> {
        self.reasons.get(index)?.as_deref()
    }

    /// The focused item's index (none when the queue is empty).
    pub fn focused(&self) -> Option<usize> {
        (!self.items.is_empty()).then(|| self.list.cursor())
    }

    /// The items marked for a bulk decision.
    pub fn marked(&self) -> Vec<usize> {
        self.list.selected()
    }

    /// Whether a reason is being typed.
    pub fn is_asking_reason(&self) -> bool {
        self.reason.is_some()
    }

    /// The keys, with any rebinding.
    pub fn approve_keymap(&self) -> &Keymap {
        &self.keymap
    }

    /// `2 approved · 1 rejected · 0 skipped · 3 pending`, so far.
    pub fn summary(&self) -> String {
        summary(&self.verdicts)
    }

    /// Rows for the list on a terminal `height` rows tall, beside the
    /// question and the buttons.
    fn rows(&self) -> usize {
        self.height
            .min(self.items.len().max(1))
            .min(self.space.get().saturating_sub(2).max(1))
    }

    fn step(&mut self, delta: isize) {
        self.list.step(delta, self.rows());
    }

    /// The next pending item after `from`, wrapping round; `None` when
    /// nothing is pending.
    fn next_pending(&self, from: usize) -> Option<usize> {
        let len = self.items.len();
        (1..=len)
            .map(|offset| (from + offset) % len)
            .find(|&index| self.verdicts[index] == Verdict::Pending)
    }

    /// What a decision applies to: the marked items, or the focused one.
    fn targets(&self) -> Vec<usize> {
        let marked = self.list.selected();
        if marked.is_empty() {
            self.focused().into_iter().collect()
        } else {
            marked
        }
    }

    /// Decide `targets` with `verdict` and `reason`, then move on.
    fn apply(&mut self, verdict: Verdict, targets: &[usize], reason: Option<String>) {
        for &index in targets {
            self.verdicts[index] = verdict;
            self.reasons[index] = reason.clone();
        }
        self.list.clear_selection(self.items.len());
        if targets.len() > 1 {
            self.status = Some(format!("{} {} items", verdict.as_str(), targets.len()));
        }
        if self.advance && verdict != Verdict::Pending {
            if let Some(next) = self.next_pending(self.list.cursor()) {
                self.list.select_row(next, self.rows());
            }
        }
    }

    /// Start deciding with `verdict`: at once, or after a reason.
    fn decide(&mut self, verdict: Verdict, ask: bool) -> Flow<Vec<usize>> {
        let targets = self.targets();
        if targets.is_empty() {
            return Flow::Continue;
        }
        if ask || self.required.contains(&verdict) {
            // A single item's own reason, to edit.
            let current = match targets[..] {
                [index] => self.reasons[index].clone().unwrap_or_default(),
                _ => String::new(),
            };
            self.reason = Some(ReasonEditor {
                verdict,
                targets,
                buffer: TextBuffer::with_text(current),
                error: None,
            });
            return Flow::Continue;
        }
        self.apply(verdict, &targets, None);
        self.maybe_submit()
    }

    fn maybe_submit(&mut self) -> Flow<Vec<usize>> {
        if self.auto_submit && !self.verdicts.contains(&Verdict::Pending) {
            return self.submit();
        }
        Flow::Continue
    }

    fn submit(&mut self) -> Flow<Vec<usize>> {
        self.reason = None;
        self.answer = Some(Some(self.summary()));
        Flow::Done((0..self.items.len()).collect())
    }

    fn reason_event(&mut self, event: &Event) -> Flow<Vec<usize>> {
        let Some(editor) = self.reason.as_mut() else {
            return Flow::Continue;
        };
        if let Event::Paste(text) = event {
            editor.buffer.insert(&pasted(text, " "));
            editor.error = None;
            return Flow::Continue;
        }
        let Some(key) = event.key() else {
            return Flow::Ignored;
        };
        let typing = !key.modifiers.ctrl && !key.modifiers.alt;
        match key.code {
            KeyCode::Enter => {
                let reason = editor.buffer.text().trim().to_string();
                if reason.is_empty() && self.required.contains(&editor.verdict) {
                    editor.error = Some(format!("a reason is needed to {}", editor.verdict.verb()));
                    return Flow::Continue;
                }
                let Some(editor) = self.reason.take() else {
                    return Flow::Continue;
                };
                let reason = (!reason.is_empty()).then_some(reason);
                self.apply(editor.verdict, &editor.targets, reason);
                return self.maybe_submit();
            }
            KeyCode::Escape => self.reason = None,
            KeyCode::Backspace => {
                editor.buffer.backspace();
            }
            KeyCode::Delete => {
                editor.buffer.delete();
            }
            KeyCode::Left => editor.buffer.left(),
            KeyCode::Right => editor.buffer.right(),
            KeyCode::Home => editor.buffer.home(),
            KeyCode::End => editor.buffer.end(),
            KeyCode::Char('u') if key.modifiers.ctrl => editor.buffer.delete_to_start(),
            KeyCode::Char('w') if key.modifiers.ctrl => editor.buffer.delete_word(),
            KeyCode::Char(c) if typing => {
                editor.buffer.insert(c.encode_utf8(&mut [0; 4]));
                editor.error = None;
            }
            _ => {}
        }
        Flow::Continue
    }

    fn act(&mut self, action: &str) -> Flow<Vec<usize>> {
        let page = self.rows() as isize;
        match action {
            "approve" => return self.decide(Verdict::Approved, false),
            "reject" => return self.decide(Verdict::Rejected, false),
            "skip" => return self.decide(Verdict::Skipped, false),
            "approve-reason" => return self.decide(Verdict::Approved, true),
            "reject-reason" => return self.decide(Verdict::Rejected, true),
            "skip-reason" => return self.decide(Verdict::Skipped, true),
            "undo" => {
                let targets = self.targets();
                self.apply(Verdict::Pending, &targets, None);
            }
            "mark" => {
                if let Some(index) = self.focused() {
                    self.list.toggle(index);
                    self.step(1);
                }
            }
            "mark-all" => {
                let pending: Vec<usize> = (0..self.items.len())
                    .filter(|&index| self.verdicts[index] == Verdict::Pending)
                    .collect();
                let all = !pending.is_empty()
                    && pending.iter().all(|&index| self.list.is_selected(index));
                self.list.clear_selection(self.items.len());
                if !all {
                    for index in pending {
                        self.list.set_selected(index, true);
                    }
                }
            }
            "next-pending" => {
                if let Some(next) = self.next_pending(self.list.cursor()) {
                    self.list.select_row(next, self.rows());
                }
            }
            "up" => self.step(-1),
            "down" => self.step(1),
            "page-up" => self.step(-page),
            "page-down" => self.step(page),
            "first" => self.step(isize::MIN / 2),
            "last" => self.step(isize::MAX / 2),
            "preview" => {
                self.preview = match self.preview {
                    PreviewLayout::Hidden => PreviewLayout::Auto,
                    _ => PreviewLayout::Hidden,
                };
            }
            "submit" => return self.submit(),
            "cancel" => {
                self.answer = Some(None);
                return Flow::Cancel;
            }
            _ => return Flow::Ignored,
        }
        Flow::Continue
    }

    /// The detail pane's layout at `width`, and the list's width beside it.
    fn layout(&self, width: usize) -> (PreviewLayout, usize) {
        if self.items.is_empty() {
            return (PreviewLayout::Hidden, width);
        }
        let layout = match self.preview {
            PreviewLayout::Auto if width >= 72 => PreviewLayout::Right,
            PreviewLayout::Auto => PreviewLayout::Below,
            layout => layout,
        };
        let left = (width * 45 / 100).max(20).min(width);
        (
            layout,
            if layout == PreviewLayout::Right {
                left
            } else {
                width
            },
        )
    }

    /// The buttons' cells on their row: `(from, to, action)`.
    fn button_spans(&self) -> Vec<(usize, usize, &'static str)> {
        let mut at = 2;
        BUTTONS
            .iter()
            .map(|(label, action)| {
                let span = (at, at + label.chars().count(), *action);
                at = span.1 + 2;
                span
            })
            .collect()
    }

    fn mouse_event(
        &mut self,
        kind: MouseKind,
        column: usize,
        row: usize,
        width: usize,
    ) -> Flow<Vec<usize>> {
        let rows = self.rows();
        let shown = self.list.visible(rows).len();
        let (layout, left) = self.layout(width);
        match kind {
            MouseKind::ScrollUp => self.step(-1),
            MouseKind::ScrollDown => self.step(1),
            MouseKind::Down(Button::Left) => {
                if row >= 1 && row < 1 + shown {
                    if layout == PreviewLayout::Right && column >= left {
                        return Flow::Continue;
                    }
                    let position = self.list.offset() + row - 1;
                    self.list.select_row(position, rows);
                    // The mark column, after the pointer.
                    if (2..4).contains(&column) {
                        self.list.toggle(position);
                    }
                } else if row == 1 + rows && self.reason.is_none() {
                    let hit = self
                        .button_spans()
                        .into_iter()
                        .find(|(from, to, _)| (*from..*to).contains(&column));
                    if let Some((_, _, action)) = hit {
                        return self.act(action);
                    }
                }
            }
            _ => return Flow::Ignored,
        }
        Flow::Continue
    }

    fn event(&mut self, event: &Event, context: &Context<'_>) -> Flow<Vec<usize>> {
        self.space.set(context.height);
        if matches!(event, Event::Key(_) | Event::Mouse(_) | Event::Paste(_)) {
            self.status = None;
        }
        if let Some(mouse) = event.mouse() {
            return self.mouse_event(
                mouse.kind,
                mouse.column as usize,
                mouse.row as usize,
                context.width,
            );
        }
        if self.reason.is_some() {
            return self.reason_event(event);
        }
        let Some(key) = event.key() else {
            return Flow::Ignored;
        };
        match self.keymap.action(key).map(str::to_string) {
            Some(action) => self.act(&action),
            None => Flow::Ignored,
        }
    }

    fn style(&self, context: &Context<'_>, verdict: Verdict) -> Style {
        context
            .console
            .theme()
            .get(verdict.theme_key())
            .cloned()
            .unwrap_or_else(|| Style::parse(verdict.default_style()).expect("a built-in style"))
    }

    fn row(&self, context: &Context<'_>, position: usize, width: usize) -> Vec<Segment> {
        let theme = &self.theme;
        let item = &self.items[position];
        let verdict = self.verdicts[position];
        let focused = position == self.list.cursor();
        let mut line = if focused {
            vec![text(format!("{} ", theme.pointer), &theme.pointer_style)]
        } else {
            vec![plain("  ")]
        };
        line.push(if self.list.is_selected(position) {
            text(format!("{} ", theme.checked), &theme.checked_style)
        } else {
            plain("  ")
        });
        let style = self.style(context, verdict);
        line.push(text(format!("{} ", verdict.mark()), &style));
        line.push(if focused {
            text(item.label.clone(), &theme.focused)
        } else {
            plain(item.label.clone())
        });
        match (&self.reasons[position], &item.description) {
            (Some(reason), _) => line.push(text(format!("  — {reason}"), &style)),
            (None, Some(description)) => line.push(text(format!("  {description}"), &theme.hint)),
            _ => {}
        }
        fit(line, width)
    }

    /// The focused item's details: label, description, metadata, decision,
    /// then its preview.
    fn detail(&self, context: &Context<'_>, width: usize) -> Vec<Vec<Segment>> {
        let Some(index) = self.focused() else {
            return Vec::new();
        };
        let theme = &self.theme;
        let item = &self.items[index];
        let verdict = self.verdicts[index];
        let mut lines = vec![fit(vec![text(item.label.clone(), &theme.focused)], width)];
        if let Some(description) = &item.description {
            lines.extend(context.lines_at(&rich::Text::new(description.clone()), width));
        }
        for (key, value) in &item.metadata {
            lines.push(fit(
                vec![text(format!("{key}: "), &theme.hint), plain(value.clone())],
                width,
            ));
        }
        let mut decided = vec![text(
            format!("{} {}", verdict.mark(), verdict.as_str()),
            &self.style(context, verdict),
        )];
        if let Some(reason) = &self.reasons[index] {
            decided.push(plain(format!(" — {reason}")));
        }
        lines.push(fit(decided, width));
        if let Some(preview) = &item.preview {
            lines.push(Vec::new());
            lines.extend(context.lines_at(&*preview.renderable(), width));
        }
        lines
    }

    /// The row under the list: the reason being typed, a status, or the
    /// buttons and counts.
    fn footer(&self, width: usize) -> (Vec<Segment>, Option<usize>) {
        let theme = &self.theme;
        if let Some(editor) = &self.reason {
            let count = editor.targets.len();
            let what = if count == 1 {
                String::new()
            } else {
                format!(" {count} items")
            };
            let mut line = vec![text(
                format!("  Reason to {}{what} › ", editor.verdict.verb()),
                &theme.prompt,
            )];
            let used = crate::components::width(&line);
            if let Some(error) = &editor.error {
                line.push(text(error.clone(), &theme.error));
                return (fit(line, width), None);
            }
            let (shown, caret) = editor.buffer.window(width.saturating_sub(used + 1), None);
            line.push(plain(shown));
            return (
                fit(line, width),
                Some((used + caret).min(width.saturating_sub(1))),
            );
        }
        if let Some(status) = &self.status {
            return (
                fit(vec![text(format!("  {status}"), &theme.hint)], width),
                None,
            );
        }
        let mut line = vec![plain("  ")];
        for (at, (label, _)) in BUTTONS.iter().enumerate() {
            if at > 0 {
                line.push(plain("  "));
            }
            line.push(text(*label, &theme.hint));
        }
        let marked = self.list.selected_count();
        if marked > 0 {
            line.push(text(format!("  · {marked} marked"), &theme.checked_style));
        }
        (fit(line, width), None)
    }

    fn render_view(&self, context: &Context<'_>) -> View {
        self.space.set(context.height);
        let width = context.width;
        let theme = &self.theme;
        let mut header = question(theme, &self.prompt);
        if let Some(answer) = &self.answer {
            header.push(match answer {
                Some(answer) => text(answer.clone(), &theme.answer),
                None => text("cancelled", &theme.hint),
            });
            return View::new(vec![fit(header, width)]);
        }
        let done = self
            .verdicts
            .iter()
            .filter(|v| **v != Verdict::Pending)
            .count();
        header.push(text(
            format!("{done}/{} decided · {}", self.items.len(), self.summary()),
            &theme.hint,
        ));
        let mut lines = vec![fit(header, width)];
        let rows = self.rows();
        let (layout, left) = self.layout(width);
        let mut list: Vec<Vec<Segment>> = self
            .list
            .visible(rows)
            .map(|position| self.row(context, position, left))
            .collect();
        if self.items.is_empty() {
            list.push(vec![text("  nothing to review", &theme.hint)]);
        }
        list.resize_with(rows, Vec::new);
        let (footer, caret) = self.footer(width);
        match layout {
            PreviewLayout::Right => {
                let right = width.saturating_sub(left + 3).max(1);
                let detail = self.detail(context, right);
                for (row, line) in list.into_iter().enumerate() {
                    let mut line = pad(line, left);
                    line.push(text(" │ ", &theme.border));
                    if let Some(detail) = detail.get(row) {
                        line.extend(fit(detail.clone(), right));
                    }
                    lines.push(line);
                }
                lines.push(footer);
            }
            PreviewLayout::Below => {
                lines.extend(list);
                lines.push(footer);
                lines.push(vec![text("─".repeat(width), &theme.border)]);
                lines.extend(
                    self.detail(context, width)
                        .into_iter()
                        .take(self.preview_height),
                );
            }
            _ => {
                lines.extend(list);
                lines.push(footer);
            }
        }
        let view = View::new(lines);
        match caret {
            Some(column) => view.with_cursor(1 + rows, column),
            None => view,
        }
    }

    /// Ask without a terminal, item by item.
    fn ask(&mut self, io: &mut dyn LineIo) -> Result<Option<Vec<usize>>, NotInteractive> {
        io.write(&format!("{}\n", self.prompt));
        let total = self.items.len();
        for index in 0..total {
            if self.verdicts[index] != Verdict::Pending {
                continue;
            }
            let item = &self.items[index];
            let mut about = format!("[{}/{total}] {}", index + 1, item.label);
            if let Some(description) = &item.description {
                about.push_str(&format!(" — {description}"));
            }
            io.write(&format!("{about}\n"));
            for (key, value) in &item.metadata {
                io.write(&format!("  {key}: {value}\n"));
            }
            loop {
                io.write("Approve, reject or skip? [a/r/s, a reason after a colon, q stops]: ");
                let Some(line) = io.read_line() else {
                    return Ok(Some((0..total).collect()));
                };
                match parse_answer(&line) {
                    Ok(None) => return Ok(Some((0..total).collect())),
                    Ok(Some((verdict, reason))) => {
                        if reason.is_none() && self.required.contains(&verdict) {
                            io.write(&format!("A reason is needed to {}.\n", verdict.verb()));
                            continue;
                        }
                        self.verdicts[index] = verdict;
                        self.reasons[index] = reason;
                        break;
                    }
                    Err(message) => io.write(&format!("{message}\n")),
                }
            }
        }
        Ok(Some((0..total).collect()))
    }
}

/// A line-prompt answer: `a`, `r`, `s` (or `approve`, `yes`, `no`…), with
/// an optional reason after a colon. `Ok(None)` stops the review (`q`); an
/// empty line skips.
pub(crate) fn parse_answer(line: &str) -> Result<Option<(Verdict, Option<String>)>, String> {
    let (word, reason) = match line.split_once(':') {
        Some((word, reason)) => (word, Some(reason.trim().to_string())),
        None => (line, None),
    };
    let reason = reason.filter(|reason| !reason.is_empty());
    let verdict = match word.trim().to_lowercase().as_str() {
        "a" | "approve" | "approved" | "y" | "yes" => Verdict::Approved,
        "r" | "reject" | "rejected" | "n" | "no" => Verdict::Rejected,
        "s" | "skip" | "skipped" | "" => Verdict::Skipped,
        "q" | "quit" => return Ok(None),
        other => return Err(format!("{other:?} is not a, r, s or q")),
    };
    Ok(Some((verdict, reason)))
}

impl<T: Clone> ApprovalQueue<T> {
    /// The report as things stand: every item with its decision so far.
    pub fn report(&self) -> ApprovalReport<T> {
        ApprovalReport {
            decisions: self
                .items
                .iter()
                .enumerate()
                .map(|(index, item)| Decision {
                    index,
                    value: item.value.clone(),
                    label: item.label.clone(),
                    verdict: self.verdicts[index],
                    reason: self.reasons[index].clone(),
                })
                .collect(),
        }
    }
}

impl<T: Clone> Component for ApprovalQueue<T> {
    type Output = ApprovalReport<T>;

    fn handle(&mut self, event: &Event, context: &Context<'_>) -> Flow<ApprovalReport<T>> {
        match self.event(event, context) {
            Flow::Done(_) => Flow::Done(self.report()),
            Flow::Cancel => Flow::Cancel,
            Flow::Ignored => Flow::Ignored,
            _ => Flow::Continue,
        }
    }

    fn render(&self, context: &Context<'_>) -> View {
        self.render_view(context)
    }

    fn mouse(&self) -> bool {
        self.mouse
    }

    fn keymap(&self) -> Keymap {
        if self.reason.is_some() {
            return Keymap::new("approve-reason")
                .bind("confirm", keys("enter"), "decide with this reason")
                .bind("back", keys("escape"), "back, deciding nothing");
        }
        self.keymap.clone()
    }

    /// The decisions as they stand: without a terminal and asked for
    /// defaults, nothing is decided.
    fn default_value(&self) -> Option<ApprovalReport<T>> {
        Some(self.report())
    }

    fn prompt(&mut self, io: &mut dyn LineIo) -> Result<Option<ApprovalReport<T>>, NotInteractive> {
        Ok(self.ask(io)?.map(|_| self.report()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn answers_parse_with_reasons() {
        assert_eq!(parse_answer("a"), Ok(Some((Verdict::Approved, None))));
        assert_eq!(
            parse_answer("r: not now "),
            Ok(Some((Verdict::Rejected, Some("not now".into()))))
        );
        assert_eq!(parse_answer(""), Ok(Some((Verdict::Skipped, None))));
        assert_eq!(parse_answer("YES:"), Ok(Some((Verdict::Approved, None))));
        assert_eq!(parse_answer("q"), Ok(None));
        assert!(parse_answer("maybe").is_err());
        assert_eq!(Verdict::parse("reject"), Some(Verdict::Rejected));
        assert_eq!(Verdict::Skipped.to_string(), "skipped");
    }
}
