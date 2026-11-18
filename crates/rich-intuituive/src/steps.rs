//! A checklist of steps, or a wizard's stepper: [`steps`], each step done
//! (`✓`), current (`▸`), pending (`○`) or failed (`✗`).

use rich::Segment;
use rich_interact::{Button as MouseButton, KeyCode, MouseKind};

use crate::a11y::Role;
use crate::layout::Size;
use crate::node::{Axis, Node};
use crate::reactive::Signal;
use crate::widget::{Canvas, DrawCx, EventCx, MeasureCx, Used, Widget, WidgetEvent, widget};

/// Where a step of [`steps`] is.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum StepState {
    /// Not reached yet: `○`.
    #[default]
    Pending,
    /// The one being done: `▸`.
    Current,
    /// Done: `✓`.
    Done,
    /// Tried, and it failed: `✗`.
    Failed,
}

impl StepState {
    /// Step `index`'s state in a wizard at step `current`: those before
    /// it done, those after it pending.
    pub fn at(index: usize, current: usize) -> StepState {
        match index.cmp(&current) {
            std::cmp::Ordering::Less => StepState::Done,
            std::cmp::Ordering::Equal => StepState::Current,
            std::cmp::Ordering::Greater => StepState::Pending,
        }
    }

    /// Its mark, and its theme style's name with the style used when the
    /// theme has none.
    fn look(self) -> (&'static str, &'static str, &'static str, &'static str) {
        match self {
            StepState::Pending => ("○", "[ ]", "steps.pending", "dim"),
            StepState::Current => ("▸", "[>]", "steps.current", "bold"),
            StepState::Done => ("✓", "[x]", "steps.done", "green"),
            StepState::Failed => ("✗", "[!]", "steps.failed", "bold red"),
        }
    }

    /// How a screen reader is told it.
    fn spoken(self) -> &'static str {
        match self {
            StepState::Pending => "pending",
            StepState::Current => "current",
            StepState::Done => "done",
            StepState::Failed => "failed",
        }
    }
}

type Items = Box<dyn Fn() -> Vec<String>>;
type States = Box<dyn Fn(usize) -> StepState>;

/// A [`steps`]'s widget.
struct Steps {
    items: Items,
    status: States,
    across: bool,
    separator: String,
    /// The step moved with the keys and clicks ([`Node::step_nav`]).
    nav: Option<Signal<usize>>,
    /// Where each step was drawn: its first row, its rows, its first
    /// column and the column after its widest row.
    hits: Vec<(u16, u16, u16, u16)>,
}

/// A list of steps, each on its own rows: each `items` entry (console
/// markup, wrapped to the width under its text, its own line breaks kept)
/// after
/// the mark of its state, `status(i)`: `✓` done, `▸` current, `○` pending
/// and `✗` failed (`[x]`, `[>]`, `[ ]` and `[!]` in text mode). Both are
/// read again whenever a signal they read changes. Each mark is in the
/// theme's `steps.done` (green), `steps.current` (bold, its text too),
/// `steps.pending` (dim, its text too) or `steps.failed` (bold red) style.
/// A plan's checklist, a job's stages, a wizard's pages:
/// [`StepState::at`] gives a wizard's states from its current page, and
/// [`step_nav`](Node::step_nav) moves it with the keys and clicks.
/// [`steps_across`](Node::steps_across) lays them in one row.
///
/// ```
/// use intuituive::prelude::*;
/// use intuituive::{steps, StepState};
///
/// let app = App::new(|| {
///     steps(
///         || vec!["Fetch".into(), "Build".into(), "Test".into(), "Deploy".into()],
///         |i| [StepState::Done, StepState::Failed, StepState::Current, StepState::Pending][i],
///     )
/// });
/// let mut driver = app.driver(12, 4);
/// driver.settle();
/// assert_eq!(driver.screen().plain(), ["✓ Fetch     ", "✗ Build     ", "▸ Test      ", "○ Deploy    "]);
/// assert_eq!(
///     driver.accessibility()[0].describe(),
///     "Fetch done, Build failed, Test current, Deploy pending, list"
/// );
/// ```
pub fn steps(
    items: impl Fn() -> Vec<String> + 'static,
    status: impl Fn(usize) -> StepState + 'static,
) -> Node {
    let mut node = widget(Steps {
        items: Box::new(items),
        status: Box::new(status),
        across: false,
        separator: " ─ ".into(),
        nav: None,
        hits: Vec::new(),
    })
    .what("steps");
    node.size = Size::Auto;
    node
}

impl Node {
    /// For [`steps`]: lay them in one row, `separator` between them (` ─ `
    /// by default, in the theme's `steps.separator` style, dim): a
    /// wizard's stepper over its pages. Ignored on other nodes.
    ///
    /// ```
    /// use intuituive::prelude::*;
    /// use intuituive::{steps, StepState};
    ///
    /// let app = App::new(|| {
    ///     let page = signal(1usize);
    ///     steps(|| vec!["Account".into(), "Profile".into(), "Done".into()], move |i| StepState::at(i, page.get()))
    ///         .steps_across(" › ")
    /// });
    /// let mut driver = app.driver(34, 1);
    /// driver.settle();
    /// assert_eq!(driver.screen().plain()[0].trim_end(), "✓ Account › ▸ Profile › ○ Done");
    /// ```
    pub fn steps_across(self, separator: &str) -> Node {
        let separator = separator.to_string();
        let mut set = false;
        self.widget_mut(|steps: &mut Steps| {
            steps.across = true;
            steps.separator = separator;
            set = true;
        });
        if set { self.fixed(1) } else { self }
    }

    /// For [`steps`]: move `current` through them, a wizard's Next and
    /// Back: with the focus, → or ↓ goes on a step and ← or ↑ back (Home
    /// and End to the ends), and a click goes to the step clicked. The
    /// current step is drawn reversed (the theme's `steps.focus`) while
    /// the list has the focus. The states still come from the `status`
    /// closure, so read `current` there ([`StepState::at`]). Ignored on
    /// other nodes.
    ///
    /// ```
    /// use intuituive::prelude::*;
    /// use intuituive::{steps, StepState};
    ///
    /// let app = App::new(|| {
    ///     let page = signal(0usize);
    ///     column([
    ///         steps(|| vec!["One".into(), "Two".into(), "Three".into()], move |i| StepState::at(i, page.get()))
    ///             .step_nav(page),
    ///         text!("page {page}"),
    ///     ])
    /// });
    /// let mut driver = app.driver(12, 4);
    /// driver.press("down down down");
    /// assert_eq!(driver.screen().plain()[3].trim_end(), "page 2");
    /// assert!(driver.click_text("One"));
    /// assert_eq!(driver.screen().plain()[0].trim_end(), "▸ One");
    /// ```
    pub fn step_nav(self, current: Signal<usize>) -> Node {
        let mut set = false;
        self.widget_mut(|steps: &mut Steps| {
            steps.nav = Some(current);
            set = true;
        });
        if set { self.focusable() } else { self }
    }
}

impl Steps {
    /// Each step's lines: its mark and its text, in its state's style.
    /// With `wrap`, the text wraps to that many cells, the lines after the
    /// first indented under it; without, it is one line, cut with `…` at
    /// its own width (the one-row stepper of [`Node::steps_across`]).
    fn rows(&self, console: &rich::Console, wrap: Option<usize>) -> Vec<Vec<Vec<Segment>>> {
        let text_mode = crate::a11y::text_mode();
        (self.items)()
            .into_iter()
            .enumerate()
            .map(|(i, item)| {
                let state = (self.status)(i);
                let (mark, text_mark, name, fallback) = state.look();
                let style = crate::widget::theme_style(console, name, fallback);
                let mark = format!("{} ", if text_mode { text_mark } else { mark });
                let indent = rich::cells::cell_len(&mark);
                let whole = matches!(state, StepState::Current | StepState::Pending);
                let restyle = |piece: Segment| match (&piece.style, whole) {
                    (Some(own), true) => Segment::new(piece.text, Some(style.combine(own))),
                    (None, true) => Segment::new(piece.text, Some(style.clone())),
                    _ => piece,
                };
                let texts: Vec<Vec<Segment>> = match wrap {
                    Some(width) => {
                        let text = rich::Text::from_markup(&item)
                            .unwrap_or_else(|_| rich::Text::new(item.clone()));
                        let options = console
                            .options()
                            .update_width(width.saturating_sub(indent).max(1));
                        let mut lines = console.render_lines(&text, &options, false);
                        while lines.len() > 1
                            && lines
                                .last()
                                .is_some_and(|l| cells_text(l).trim().is_empty())
                        {
                            lines.pop();
                        }
                        if lines.is_empty() {
                            vec![Vec::new()]
                        } else {
                            lines
                        }
                    }
                    None => {
                        let width = crate::widget::markup_width(&item);
                        vec![crate::widget::markup_line(console, &item, width)]
                    }
                };
                texts
                    .into_iter()
                    .enumerate()
                    .map(|(k, text)| {
                        let lead = if k == 0 {
                            Segment::new(mark.clone(), Some(style.clone()))
                        } else {
                            Segment::new(" ".repeat(indent), None)
                        };
                        std::iter::once(lead)
                            .chain(text.into_iter().map(restyle))
                            .collect()
                    })
                    .collect()
            })
            .collect()
    }
}

fn cells_text(line: &[Segment]) -> String {
    line.iter().map(|s| s.text.as_str()).collect()
}

fn cells(line: &[Segment]) -> usize {
    line.iter().map(Segment::cell_length).sum()
}

impl Widget for Steps {
    fn name(&self) -> &'static str {
        "steps"
    }

    fn role(&self) -> Role {
        Role::List
    }

    fn access_text(&self) -> Option<String> {
        let said: Vec<String> = (self.items)()
            .iter()
            .enumerate()
            .map(|(i, item)| {
                let plain = rich::Text::from_markup(item)
                    .map(|t| t.plain().to_string())
                    .unwrap_or_else(|_| item.clone());
                format!("{plain} {}", (self.status)(i).spoken())
            })
            .collect();
        Some(said.join(", "))
    }

    fn measure(&mut self, cx: &MeasureCx, axis: Axis, width: u16, _height: u16) -> u16 {
        // Unwrapped for the width it would take, wrapped to `width` for the
        // rows it takes there.
        let wrap = (axis == Axis::Vertical && !self.across && width > 0).then_some(width as usize);
        let rows = self.rows(cx.console(), wrap);
        let first = |item: &Vec<Vec<Segment>>| item.first().map_or(0, |l| cells(l));
        let wanted = match (axis, self.across) {
            (Axis::Vertical, true) => 1,
            (Axis::Vertical, false) => rows.iter().map(Vec::len).sum(),
            (Axis::Horizontal, false) => rows.iter().flatten().map(|l| cells(l)).max().unwrap_or(0),
            (Axis::Horizontal, true) => {
                let gaps = rows.len().saturating_sub(1) * rich::cells::cell_len(&self.separator);
                rows.iter().map(first).sum::<usize>() + gaps
            }
        }
        .min(u16::MAX as usize) as u16;
        match axis {
            Axis::Horizontal if width > 0 => wanted.min(width),
            _ => wanted,
        }
    }

    fn draw(&mut self, cx: &mut DrawCx, canvas: &mut Canvas) {
        let wrap = (!self.across).then_some(canvas.width().max(1) as usize);
        let rows = self.rows(cx.console(), wrap);
        let mut hits = Vec::new();
        let cap = |n: usize| n.min(u16::MAX as usize) as u16;
        let lines: Vec<Vec<Segment>> = if self.across {
            let sep = cx.style("steps.separator", "dim");
            let mut line = Vec::new();
            for (i, row) in rows.into_iter().enumerate() {
                if i > 0 {
                    line.push(Segment::new(self.separator.clone(), Some(sep.clone())));
                }
                let start = cells(&line);
                line.extend(row.into_iter().next().unwrap_or_default());
                hits.push((0, 1, cap(start), cap(cells(&line))));
            }
            vec![line]
        } else {
            let mut at = 0usize;
            for item in &rows {
                let widest = item.iter().map(|l| cells(l)).max().unwrap_or(0);
                hits.push((cap(at), cap(item.len()), 0, cap(widest)));
                at += item.len();
            }
            rows.into_iter().flatten().collect()
        };
        canvas.lines(&lines);
        if let (Some(current), true) = (self.nav, cx.focused())
            && let Some(&(row, count, start, end)) = hits.get(current.get())
        {
            let style = cx.style("steps.focus", "reverse");
            canvas.restyle(start, row, end - start, count, &style);
        }
        self.hits = hits;
    }

    fn event(&mut self, cx: &mut EventCx, event: &WidgetEvent) -> Used {
        let Some(current) = self.nav else {
            return Used::No;
        };
        let count = self.hits.len();
        if count == 0 {
            return Used::No;
        }
        let at = current.get_untracked().min(count - 1);
        let to = match event {
            WidgetEvent::Mouse(mouse) if mouse.kind == MouseKind::Down(MouseButton::Left) => {
                let hit = self.hits.iter().position(|&(row, count, start, end)| {
                    (row..row.saturating_add(count)).contains(&mouse.row)
                        && (start..end).contains(&mouse.column)
                });
                match hit {
                    Some(hit) => hit,
                    None => return Used::No,
                }
            }
            WidgetEvent::Key(key) if !key.modifiers.ctrl && !key.modifiers.alt => match key.code {
                KeyCode::Right | KeyCode::Down if at + 1 < count => at + 1,
                KeyCode::Left | KeyCode::Up if at > 0 => at - 1,
                KeyCode::Home => 0,
                KeyCode::End => count - 1,
                _ => return Used::No,
            },
            _ => return Used::No,
        };
        if to != current.get_untracked() {
            current.set(to);
        }
        cx.redraw();
        Used::Yes
    }
}
