//! A tree of nested tasks for build, deploy and install workflows.
//!
//! A [`TaskTree`] holds tasks added under optional parents. Each leaf moves
//! through `pending → running → succeeded | warning | failed | skipped |
//! cancelled`; a parent's state is aggregated from its children, so only
//! leaves need transitions. Times come from an injected [`Clock`] (a
//! [`ManualClock`](super::ManualClock) makes tests exact), and every task
//! owns a [`CancelToken`] that is a child of its parent's: cancelling a task
//! stops its whole subtree and marks every unfinished task in it cancelled.
//!
//! The tree renders with guides, a status marker per task, a spinner for
//! running work, optional progress and durations:
//!
//! ```text
//! Deploy
//! ├── ✔ ok build  12.4s
//! ├── ⠧ running upload  21/50 (42%)  3.0s
//! │   ├── ✔ ok assets  1.1s
//! │   └── ⠧ running images  21/50 (42%)  1.9s
//! └── … pending verify
//! ```
//!
//! [`TaskTreeView::collapse_finished`] folds successful finished subtrees to
//! one line. Every view is a plain renderable, so a live display renders it
//! into a [`LiveCoordinator`](crate::live::LiveCoordinator) region on each
//! change or tick.
//!
//! ```
//! use std::time::Duration;
//! use rich_ext::workflow::{ManualClock, State, TaskTree};
//!
//! let clock = ManualClock::new();
//! let mut tree = TaskTree::with_clock(clock.clone());
//! let deploy = tree.add(None, "deploy");
//! let upload = tree.add(Some(deploy), "upload");
//! let verify = tree.add(Some(deploy), "verify");
//! tree.start(upload);
//! clock.advance(Duration::from_secs(2));
//! assert_eq!(tree.state(deploy), State::Running);
//!
//! tree.cancel(deploy);
//! assert!(tree.token(verify).is_cancelled());
//! assert_eq!(tree.state(verify), State::Cancelled);
//! assert_eq!(tree.elapsed(deploy), Some(Duration::from_secs(2)));
//! ```

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use rich::{Console, ConsoleOptions, Overflow, Renderable, Segment, Text};

use super::{render_texts, span, Clock, Look, State, SystemClock};
use crate::a11y::{AccessibilityPolicy, SymbolSet};
use crate::cancel::CancelToken;

/// A task's handle within its [`TaskTree`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TaskId(usize);

#[derive(Clone, Debug)]
struct Task {
    label: String,
    parent: Option<TaskId>,
    children: Vec<TaskId>,
    state: State,
    started: Option<Duration>,
    finished: Option<Duration>,
    progress: Option<(u64, Option<u64>)>,
    note: Option<String>,
    token: CancelToken,
}

/// Nested tasks with timing, aggregate status and cancellation. See the
/// [module docs](self).
///
/// Methods taking a [`TaskId`] panic when the id came from another tree.
#[derive(Clone)]
pub struct TaskTree {
    title: Option<String>,
    tasks: Vec<Task>,
    roots: Vec<TaskId>,
    clock: Arc<dyn Clock>,
    token: CancelToken,
}

impl std::fmt::Debug for TaskTree {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TaskTree")
            .field("title", &self.title)
            .field("tasks", &self.tasks)
            .field("roots", &self.roots)
            .field("now", &self.clock.now())
            .finish_non_exhaustive()
    }
}

impl Default for TaskTree {
    fn default() -> Self {
        Self::new()
    }
}

impl TaskTree {
    /// An empty tree timed by the wall clock.
    pub fn new() -> Self {
        Self::with_clock(SystemClock::new())
    }

    /// An empty tree timed by `clock`.
    pub fn with_clock(clock: impl Clock + 'static) -> Self {
        TaskTree {
            title: None,
            tasks: Vec::new(),
            roots: Vec::new(),
            clock: Arc::new(clock),
            token: CancelToken::new(),
        }
    }

    /// A heading shown above the tasks and used by
    /// [`CompletionSummary`](super::CompletionSummary).
    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    /// The heading, if any.
    pub fn get_title(&self) -> Option<&str> {
        self.title.as_deref()
    }

    /// Add a pending task under `parent` (or at the top level).
    pub fn add(&mut self, parent: Option<TaskId>, label: impl Into<String>) -> TaskId {
        let id = TaskId(self.tasks.len());
        let token = match parent {
            Some(parent) => self.task(parent).token.child(),
            None => self.token.child(),
        };
        self.tasks.push(Task {
            label: label.into(),
            parent,
            children: Vec::new(),
            state: State::Pending,
            started: None,
            finished: None,
            progress: None,
            note: None,
            token,
        });
        match parent {
            Some(parent) => self.tasks[parent.0].children.push(id),
            None => self.roots.push(id),
        }
        id
    }

    fn task(&self, id: TaskId) -> &Task {
        &self.tasks[id.0]
    }

    fn now(&self) -> Duration {
        self.clock.now()
    }

    /// Mark `id` running from now.
    pub fn start(&mut self, id: TaskId) -> &mut Self {
        let now = self.now();
        let task = &mut self.tasks[id.0];
        task.state = State::Running;
        task.started = Some(now);
        task.finished = None;
        self
    }

    fn finish(&mut self, id: TaskId, state: State, note: Option<String>) -> &mut Self {
        let now = self.now();
        let task = &mut self.tasks[id.0];
        task.state = state;
        // A task skipped before it started has no duration.
        if state != State::Skipped || task.started.is_some() {
            task.started.get_or_insert(now);
            task.finished = Some(now);
        }
        if note.is_some() {
            task.note = note;
        }
        self
    }

    /// Mark `id` succeeded.
    pub fn succeed(&mut self, id: TaskId) -> &mut Self {
        self.finish(id, State::Succeeded, None)
    }

    /// Mark `id` finished with a warning, shown after the task.
    pub fn warn(&mut self, id: TaskId, message: impl Into<String>) -> &mut Self {
        self.finish(id, State::Warning, Some(message.into()))
    }

    /// Mark `id` failed, with the reason shown after the task.
    pub fn fail(&mut self, id: TaskId, message: impl Into<String>) -> &mut Self {
        self.finish(id, State::Failed, Some(message.into()))
    }

    /// Mark `id` skipped, with an optional reason.
    pub fn skip(&mut self, id: TaskId, reason: Option<&str>) -> &mut Self {
        self.finish(id, State::Skipped, reason.map(str::to_string))
    }

    /// Set a note shown after the task without changing its state.
    pub fn note(&mut self, id: TaskId, note: impl Into<String>) -> &mut Self {
        self.tasks[id.0].note = Some(note.into());
        self
    }

    /// Report `completed` of `total` units (or of an unknown total).
    pub fn progress(&mut self, id: TaskId, completed: u64, total: Option<u64>) -> &mut Self {
        self.tasks[id.0].progress = Some((completed, total));
        self
    }

    /// Cancel `id`: its token (and so every descendant's) is cancelled, and
    /// each unfinished task in the subtree is marked cancelled now.
    pub fn cancel(&mut self, id: TaskId) -> &mut Self {
        self.tasks[id.0].token.cancel();
        self.sync_cancelled();
        self
    }

    /// Cancel every task.
    pub fn cancel_all(&mut self) -> &mut Self {
        self.token.cancel();
        self.sync_cancelled();
        self
    }

    /// Mark unfinished tasks whose token was cancelled from elsewhere (a
    /// worker thread, a Ctrl-C handler) as cancelled; returns how many.
    pub fn sync_cancelled(&mut self) -> usize {
        let now = self.now();
        // Decide on the aggregate states first: a parent whose children all
        // finished is finished even if it was never started itself.
        let cancelled: Vec<usize> = (0..self.tasks.len())
            .filter(|&i| !self.state(TaskId(i)).is_finished() && self.tasks[i].token.is_cancelled())
            .collect();
        for &i in &cancelled {
            let task = &mut self.tasks[i];
            task.state = State::Cancelled;
            if task.started.is_some() {
                task.finished = Some(now);
            }
        }
        cancelled.len()
    }

    /// The token cancelled with `id`; hand it to the work doing the task.
    pub fn token(&self, id: TaskId) -> CancelToken {
        self.task(id).token.clone()
    }

    /// The token for the whole tree.
    pub fn root_token(&self) -> CancelToken {
        self.token.clone()
    }

    /// The label of `id`.
    pub fn label(&self, id: TaskId) -> &str {
        &self.task(id).label
    }

    /// The note (warning, failure reason, skip reason) of `id`.
    pub fn get_note(&self, id: TaskId) -> Option<&str> {
        self.task(id).note.as_deref()
    }

    /// The parent of `id`.
    pub fn parent(&self, id: TaskId) -> Option<TaskId> {
        self.task(id).parent
    }

    /// The children of `id`, in the order added.
    pub fn children(&self, id: TaskId) -> &[TaskId] {
        &self.task(id).children
    }

    /// The top-level tasks, in the order added.
    pub fn roots(&self) -> &[TaskId] {
        &self.roots
    }

    /// Every task, depth first in display order.
    pub fn iter(&self) -> impl Iterator<Item = TaskId> + '_ {
        let mut stack: Vec<TaskId> = self.roots.iter().rev().copied().collect();
        std::iter::from_fn(move || {
            let id = stack.pop()?;
            stack.extend(self.task(id).children.iter().rev());
            Some(id)
        })
    }

    /// The tasks with no children, depth first.
    pub fn leaves(&self) -> impl Iterator<Item = TaskId> + '_ {
        self.iter().filter(|id| self.task(*id).children.is_empty())
    }

    /// The reported progress of `id`: `(completed, total)`.
    pub fn get_progress(&self, id: TaskId) -> Option<(u64, Option<u64>)> {
        self.task(id).progress
    }

    /// The state of `id`, aggregated from its children when it has any:
    ///
    /// * an explicit failure or cancellation of the parent itself wins;
    /// * any running child, or a mix of finished and pending children, is
    ///   running;
    /// * all pending is pending (or running, if the parent was started);
    /// * all finished is the worst outcome — failed, cancelled, warning,
    ///   succeeded — and skipped only when every child was skipped.
    pub fn state(&self, id: TaskId) -> State {
        let task = self.task(id);
        if task.children.is_empty() || task.state.is_problem() {
            return task.state;
        }
        aggregate(
            task.state,
            task.children.iter().map(|child| self.state(*child)),
        )
    }

    /// The state of the whole tree, aggregated from the top-level tasks.
    pub fn overall(&self) -> State {
        if self.roots.is_empty() {
            return State::Pending;
        }
        aggregate(
            State::Pending,
            self.roots.iter().map(|root| self.state(*root)),
        )
    }

    /// Whether every task has finished.
    pub fn is_finished(&self) -> bool {
        self.overall().is_finished()
    }

    fn span_of(&self, id: TaskId) -> (Option<Duration>, Option<Duration>) {
        let task = self.task(id);
        let (mut start, mut end) = (task.started, task.finished);
        for child in &task.children {
            let (s, e) = self.span_of(*child);
            start = min_opt(start, s);
            end = max_opt(end, e);
        }
        (start, end)
    }

    /// How long `id` has run: from its (or its first child's) start to its
    /// last finish, or to now while it is unfinished. `None` before it starts.
    pub fn elapsed(&self, id: TaskId) -> Option<Duration> {
        let (start, end) = self.span_of(id);
        let end = if self.state(id).is_finished() {
            end
        } else {
            None
        };
        let start = start?;
        Some(end.unwrap_or_else(|| self.now()).saturating_sub(start))
    }

    /// How long the whole tree has run, as [`elapsed`](Self::elapsed).
    pub fn total_elapsed(&self) -> Option<Duration> {
        let (mut start, mut end) = (None, None);
        for root in &self.roots {
            let (s, e) = self.span_of(*root);
            start = min_opt(start, s);
            end = max_opt(end, e);
        }
        let end = if self.is_finished() { end } else { None };
        Some(end.unwrap_or_else(|| self.now()).saturating_sub(start?))
    }

    /// How many leaf tasks are in each state.
    pub fn counts(&self) -> BTreeMap<State, usize> {
        let mut counts = BTreeMap::new();
        for leaf in self.leaves() {
            *counts.entry(self.state(leaf)).or_insert(0) += 1;
        }
        counts
    }

    /// A view of this tree with the default options.
    pub fn view(&self) -> TaskTreeView<'_> {
        TaskTreeView::new(self)
    }
}

fn min_opt(a: Option<Duration>, b: Option<Duration>) -> Option<Duration> {
    match (a, b) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (a, b) => a.or(b),
    }
}

fn max_opt(a: Option<Duration>, b: Option<Duration>) -> Option<Duration> {
    match (a, b) {
        (Some(a), Some(b)) => Some(a.max(b)),
        (a, b) => a.or(b),
    }
}

/// Combine child states under a parent whose own state is `own`.
pub(crate) fn aggregate(own: State, children: impl Iterator<Item = State>) -> State {
    let states: Vec<State> = children.collect();
    if states.is_empty() {
        return own;
    }
    let pending = states.iter().filter(|s| **s == State::Pending).count();
    if states.contains(&State::Running) || (pending > 0 && pending < states.len()) {
        return State::Running;
    }
    if pending == states.len() {
        return if own == State::Running {
            State::Running
        } else {
            State::Pending
        };
    }
    if states.iter().all(|s| *s == State::Skipped) {
        return State::Skipped;
    }
    let worst = states
        .into_iter()
        .filter(|s| *s != State::Skipped)
        .min()
        .unwrap_or(State::Succeeded);
    if own == State::Warning && worst > State::Warning {
        State::Warning
    } else {
        worst
    }
}

/// Guide pieces: (fork, last, continue, space).
fn guides(ascii: bool) -> [&'static str; 4] {
    if ascii {
        ["+-- ", "`-- ", "|   ", "    "]
    } else {
        ["├── ", "└── ", "│   ", "    "]
    }
}

/// How a [`TaskTree`] is rendered. See the [module docs](self).
#[derive(Clone, Debug)]
pub struct TaskTreeView<'a> {
    tree: &'a TaskTree,
    collapse_finished: bool,
    show_durations: bool,
    look: Look,
}

impl<'a> TaskTreeView<'a> {
    /// Every task expanded, with durations, Unicode markers and spinners.
    pub fn new(tree: &'a TaskTree) -> Self {
        TaskTreeView {
            tree,
            collapse_finished: false,
            show_durations: true,
            look: Look::default(),
        }
    }

    /// Show a finished subtree that succeeded (or was skipped) as its parent
    /// line alone, with a count of the hidden tasks. Subtrees with a
    /// failure, warning or cancellation stay expanded.
    pub fn collapse_finished(mut self, collapse: bool) -> Self {
        self.collapse_finished = collapse;
        self
    }

    /// Whether durations are shown (default true).
    pub fn show_durations(mut self, show: bool) -> Self {
        self.show_durations = show;
        self
    }

    /// Mark status with `set`; `Ascii` and `Words` also use ASCII guides
    /// and stop the spinner.
    pub fn symbols(mut self, set: SymbolSet) -> Self {
        self.look.symbols = set;
        self
    }

    /// Whether running tasks show a spinner (default true).
    pub fn animate(mut self, animate: bool) -> Self {
        self.look.animate = animate;
        self
    }

    /// Follow `policy`: its status symbols, and no spinner under reduced
    /// motion, no animation or a screen reader.
    pub fn policy(mut self, policy: &AccessibilityPolicy) -> Self {
        self.look = Look::from_policy(policy);
        self
    }

    fn collapsible(&self, id: TaskId) -> bool {
        let tree = self.tree;
        self.collapse_finished
            && !tree.children(id).is_empty()
            && matches!(tree.state(id), State::Succeeded | State::Skipped)
            && tree
                .iter_subtree(id)
                .all(|t| matches!(tree.state(t), State::Succeeded | State::Skipped))
    }

    fn line(&self, console: &Console, id: TaskId, prefix: &str) -> Text {
        let tree = self.tree;
        let state = tree.state(id);
        let elapsed = tree.elapsed(id);
        let mut text = Text::new("");
        if !prefix.is_empty() {
            text.append(prefix, span(console, "workflow.guide"));
        }
        text.append(
            &self.look.marker(state, elapsed.unwrap_or_default()),
            span(console, state.style_key()),
        );
        text.append(" ", None);
        let label_key = if state == State::Running {
            "workflow.task.running"
        } else {
            "workflow.task.label"
        };
        text.append(tree.label(id), span(console, label_key));
        if let Some((done, total)) = tree
            .get_progress(id)
            .filter(|_| !matches!(state, State::Succeeded | State::Skipped))
        {
            let progress = match total {
                Some(total) if total > 0 => format!(
                    "{done}/{total} ({})",
                    crate::format::percent(done as f64 / total as f64, 0)
                ),
                _ => done.to_string(),
            };
            text.append("  ", None);
            text.append(&progress, span(console, "workflow.task.progress"));
        }
        if self.collapsible(id) {
            let hidden = tree.iter_subtree(id).count() - 1;
            let noun = if hidden == 1 { "task" } else { "tasks" };
            text.append(
                &format!(" (+{hidden} {noun})"),
                span(console, "workflow.hidden"),
            );
        }
        if let Some(elapsed) = elapsed.filter(|_| self.show_durations) {
            text.append("  ", None);
            text.append(
                &self.look.duration(elapsed),
                span(console, "workflow.duration"),
            );
        }
        if let Some(note) = tree.get_note(id) {
            text.append("  ", None);
            text.append(note, span(console, "workflow.task.note"));
        }
        // Wrapping would break the guides: long lines end in an ellipsis.
        text.no_wrap(true).overflow(Overflow::Ellipsis)
    }

    fn walk(&self, console: &Console, id: TaskId, prefix: &str, first: &str, out: &mut Vec<Text>) {
        out.push(self.line(console, id, first));
        if self.collapsible(id) {
            return;
        }
        let [fork, last, cont, space] = guides(self.look.ascii());
        let children = self.tree.children(id);
        for (index, child) in children.iter().enumerate() {
            let is_last = index + 1 == children.len();
            let head = format!("{prefix}{}", if is_last { last } else { fork });
            let rest = format!("{prefix}{}", if is_last { space } else { cont });
            self.walk(console, *child, &rest, &head, out);
        }
    }
}

impl TaskTree {
    /// `id` and its descendants, depth first.
    fn iter_subtree(&self, id: TaskId) -> impl Iterator<Item = TaskId> + '_ {
        let mut stack = vec![id];
        std::iter::from_fn(move || {
            let id = stack.pop()?;
            stack.extend(self.task(id).children.iter().rev());
            Some(id)
        })
    }
}

impl Renderable for TaskTreeView<'_> {
    fn rich_render(&self, console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
        let mut lines = Vec::new();
        let [fork, last, cont, space] = guides(self.look.ascii());
        let roots = self.tree.roots();
        match &self.tree.title {
            Some(title) => {
                lines.push(Text::styled(
                    title.clone(),
                    super::style(console, "workflow.summary.title"),
                ));
                for (index, root) in roots.iter().enumerate() {
                    let is_last = index + 1 == roots.len();
                    let (head, rest) = if is_last { (last, space) } else { (fork, cont) };
                    self.walk(console, *root, rest, head, &mut lines);
                }
            }
            None => {
                for root in roots {
                    self.walk(console, *root, "", "", &mut lines);
                }
            }
        }
        render_texts(console, options, &lines)
    }
}

impl Renderable for TaskTree {
    fn rich_render(&self, console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
        self.view().rich_render(console, options)
    }
}
