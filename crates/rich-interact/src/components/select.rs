//! Fuzzy selection (#287), single and multiple, with a preview pane
//! (#454).
//!
//! Typing filters the items with the [fuzzy](crate::fuzzy) matcher and
//! highlights what matched; the arrows move; Enter picks. A multi-select
//! marks items with Tab. When the focused item has a
//! [`Preview`](crate::Preview), a pane shows it beside the list on a wide
//! terminal and below it on a narrow one. An item's
//! [`Action`](crate::Action) keys pick it and record which action was asked
//! for.

use rich::Segment;

use crate::component::{Component, Context, Flow, View};
use crate::components::{fit, highlight, pad, plain, question, text, Theme};
use crate::event::{Event, KeyCode, MouseKind};
use crate::fuzzy::rank;
use crate::item::Item;
use crate::policy::{LineIo, NotInteractive};

/// Where the preview pane goes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PreviewLayout {
    /// Beside the list from 72 columns, below it when narrower.
    #[default]
    Auto,
    Right,
    Below,
    Hidden,
}

/// A fuzzy single choice among items. Enter returns the focused item's
/// value; Escape cancels.
pub struct Select<T> {
    prompt: String,
    items: Vec<Item<T>>,
    query: String,
    /// Item indices that match the query, best first, with the label
    /// characters to highlight.
    matches: Vec<(usize, Vec<usize>)>,
    focus: usize,
    offset: usize,
    height: usize,
    multi: bool,
    marked: Vec<bool>,
    preview: PreviewLayout,
    preview_height: usize,
    theme: Theme,
    action: Option<String>,
    default: Option<usize>,
    /// Once finished: the answer shown in place of the list (`None` inside
    /// means cancelled).
    answer: Option<Option<String>>,
}

impl<T> Select<T> {
    pub fn new<I, V>(prompt: impl Into<String>, items: I) -> Select<T>
    where
        I: IntoIterator<Item = V>,
        V: Into<Item<T>>,
    {
        let items: Vec<Item<T>> = items.into_iter().map(Into::into).collect();
        let mut select = Select {
            prompt: prompt.into(),
            marked: vec![false; items.len()],
            items,
            query: String::new(),
            matches: Vec::new(),
            focus: 0,
            offset: 0,
            height: 10,
            multi: false,
            preview: PreviewLayout::Auto,
            preview_height: 10,
            theme: Theme::default(),
            action: None,
            default: None,
            answer: None,
        };
        select.refilter();
        select
    }

    /// Show at most `rows` items at once (default 10).
    pub fn height(mut self, rows: usize) -> Self {
        self.height = rows.max(1);
        self
    }

    pub fn preview(mut self, layout: PreviewLayout) -> Self {
        self.preview = layout;
        self
    }

    /// Rows the preview takes when it is below the list (default 10).
    pub fn preview_height(mut self, rows: usize) -> Self {
        self.preview_height = rows.max(1);
        self
    }

    pub fn theme(mut self, theme: Theme) -> Self {
        self.theme = theme;
        self
    }

    /// Start with this filter text.
    pub fn query(mut self, query: impl Into<String>) -> Self {
        self.query = query.into();
        self.refilter();
        self
    }

    /// The item returned without a terminal when the policy asks for
    /// defaults, and focused first.
    pub fn default(mut self, index: usize) -> Self {
        if index < self.items.len() {
            self.default = Some(index);
            if let Some(position) = self.matches.iter().position(|(i, _)| *i == index) {
                self.focus = position;
            }
        }
        self
    }

    /// The id of the [`Action`](crate::Action) whose key picked the item,
    /// if one did.
    pub fn action(&self) -> Option<&str> {
        self.action.as_deref()
    }

    pub fn items(&self) -> &[Item<T>] {
        &self.items
    }

    /// The focused item's index, if any item matches.
    pub fn focused(&self) -> Option<usize> {
        self.matches.get(self.focus).map(|(index, _)| *index)
    }

    fn refilter(&mut self) {
        let focused = self.focused();
        let texts: Vec<String> = self.items.iter().map(Item::search_text).collect();
        self.matches = rank(&self.query, texts.iter().map(String::as_str))
            .into_iter()
            .map(|(index, found)| {
                let label = self.items[index].label.chars().count();
                let mut positions = found.positions;
                positions.retain(|position| *position < label);
                (index, positions)
            })
            .collect();
        // Keep the focused item when it still matches, else the best match.
        self.focus = focused
            .and_then(|index| self.matches.iter().position(|(i, _)| *i == index))
            .filter(|_| self.query.is_empty())
            .unwrap_or(0);
        self.offset = 0;
        self.scroll();
    }

    fn scroll(&mut self) {
        if self.focus < self.offset {
            self.offset = self.focus;
        } else if self.focus >= self.offset + self.height {
            self.offset = self.focus + 1 - self.height;
        }
    }

    fn step(&mut self, delta: isize) {
        if self.matches.is_empty() {
            return;
        }
        let last = self.matches.len() - 1;
        self.focus = self.focus.saturating_add_signed(delta).min(last);
        self.scroll();
    }

    /// Handle an event; `Some` finishes with item indices, and the view
    /// collapses to the answer.
    fn event(&mut self, event: &Event) -> Option<Flow<Vec<usize>>> {
        let flow = self.event_inner(event);
        match &flow {
            Some(Flow::Done(indices)) => {
                let labels: Vec<&str> = indices
                    .iter()
                    .map(|&index| self.items[index].label.as_str())
                    .collect();
                self.answer = Some(Some(labels.join(", ")));
            }
            Some(Flow::Cancel) => self.answer = Some(None),
            _ => {}
        }
        flow
    }

    fn event_inner(&mut self, event: &Event) -> Option<Flow<Vec<usize>>> {
        if let Event::Mouse(mouse) = event {
            match mouse.kind {
                MouseKind::ScrollUp => self.step(-1),
                MouseKind::ScrollDown => self.step(1),
                _ => {}
            }
            return None;
        }
        if let Event::Paste(text) = event {
            self.query.push_str(&text.replace(['\n', '\r'], " "));
            self.refilter();
            return None;
        }
        let key = event.key()?;
        if let Some(index) = self.focused() {
            if let Some(action) = self.items[index].action_for(key) {
                self.action = Some(action.id.clone());
                return Some(Flow::Done(vec![index]));
            }
        }
        let ctrl = key.modifiers.ctrl;
        match key.code {
            KeyCode::Enter => {
                let index = self.focused()?;
                let marked: Vec<usize> =
                    (0..self.items.len()).filter(|&i| self.marked[i]).collect();
                return Some(Flow::Done(if self.multi && !marked.is_empty() {
                    marked
                } else {
                    vec![index]
                }));
            }
            KeyCode::Escape => return Some(Flow::Cancel),
            KeyCode::Up => self.step(-1),
            KeyCode::Down => self.step(1),
            KeyCode::Char('p') if ctrl => self.step(-1),
            KeyCode::Char('n') if ctrl => self.step(1),
            KeyCode::PageUp => self.step(-(self.height as isize)),
            KeyCode::PageDown => self.step(self.height as isize),
            KeyCode::Home => self.step(isize::MIN / 2),
            KeyCode::End => self.step(isize::MAX / 2),
            KeyCode::Tab | KeyCode::BackTab if self.multi => {
                if let Some(index) = self.focused() {
                    self.marked[index] = !self.marked[index];
                    self.step(if key.code == KeyCode::Tab { 1 } else { -1 });
                }
            }
            KeyCode::Char('a') if ctrl && self.multi => {
                let all = self.matches.iter().all(|(i, _)| self.marked[*i]);
                for (index, _) in &self.matches {
                    self.marked[*index] = !all;
                }
            }
            KeyCode::Char('u') if ctrl => {
                self.query.clear();
                self.refilter();
            }
            KeyCode::Backspace => {
                if self.query.pop().is_some() {
                    self.refilter();
                }
            }
            KeyCode::Char(c) if !ctrl && !key.modifiers.alt => {
                self.query.push(c);
                self.refilter();
            }
            _ => {}
        }
        None
    }

    fn row(&self, position: usize, width: usize) -> Vec<Segment> {
        let (index, positions) = &self.matches[position];
        let item = &self.items[*index];
        let theme = &self.theme;
        let focused = position == self.focus;
        let mut line = if focused {
            vec![text(format!("{} ", theme.pointer), &theme.pointer_style)]
        } else {
            vec![plain("  ")]
        };
        if self.multi {
            line.push(if self.marked[*index] {
                text(format!("{} ", theme.checked), &theme.checked_style)
            } else {
                text(format!("{} ", theme.unchecked), &theme.hint)
            });
        }
        let base = focused.then_some(&theme.focused);
        line.extend(highlight(&item.label, positions, base, &theme.matched));
        if let Some(description) = &item.description {
            line.push(text(format!("  {description}"), &theme.hint));
        }
        fit(line, width)
    }

    fn list(&self, width: usize) -> Vec<Vec<Segment>> {
        let rows = self.height.min(self.items.len()).max(1);
        let mut lines: Vec<Vec<Segment>> = (self.offset..self.matches.len())
            .take(rows)
            .map(|position| self.row(position, width))
            .collect();
        if self.matches.is_empty() {
            lines.push(vec![text("  no matches", &self.theme.hint)]);
        }
        // A steady height, so the footer does not jump as the list filters.
        lines.resize_with(rows, Vec::new);
        lines
    }

    fn footer(&self, width: usize) -> Vec<Segment> {
        let mut hint = format!("{}/{}", self.matches.len(), self.items.len());
        if self.multi {
            let marked = self.marked.iter().filter(|m| **m).count();
            hint.push_str(&format!(" · {marked} marked · tab mark"));
        }
        hint.push_str(" · ↑↓ move · enter pick · esc cancel");
        fit(vec![text(format!("  {hint}"), &self.theme.hint)], width)
    }

    fn render_view(&self, context: &Context<'_>) -> View {
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
        let column = crate::components::width(&header) + rich::cells::cell_len(&self.query);
        header.push(plain(self.query.clone()));
        let mut lines = vec![fit(header, width)];
        let preview = self
            .focused()
            .and_then(|index| self.items[index].preview.as_ref())
            .filter(|_| self.preview != PreviewLayout::Hidden);
        let layout = match self.preview {
            PreviewLayout::Auto if width >= 72 => PreviewLayout::Right,
            PreviewLayout::Auto => PreviewLayout::Below,
            layout => layout,
        };
        match preview {
            Some(preview) if layout == PreviewLayout::Right => {
                let left = (width * 45 / 100).max(20).min(width);
                let right = width.saturating_sub(left + 3).max(1);
                let list = self.list(left);
                let shown = context.lines_at(&*preview.renderable(), right);
                for (row, line) in list.into_iter().enumerate() {
                    let mut line = pad(line, left);
                    line.push(text(" │ ", &theme.border));
                    if let Some(preview) = shown.get(row) {
                        line.extend(fit(preview.clone(), right));
                    }
                    lines.push(line);
                }
                lines.push(self.footer(width));
            }
            Some(preview) => {
                lines.extend(self.list(width));
                lines.push(self.footer(width));
                lines.push(vec![text("─".repeat(width), &theme.border)]);
                let shown = context.lines_at(&*preview.renderable(), width);
                lines.extend(
                    shown
                        .into_iter()
                        .take(self.preview_height)
                        .map(|line| fit(line, width)),
                );
            }
            None => {
                lines.extend(self.list(width));
                lines.push(self.footer(width));
            }
        }
        View::new(lines).with_cursor(0, column.min(width.saturating_sub(1)))
    }

    /// Ask without a terminal: list the items, read a number or text.
    fn ask(&mut self, io: &mut dyn LineIo) -> Result<Option<Vec<usize>>, NotInteractive> {
        io.write(&format!("{}\n", self.prompt));
        for (index, item) in self.items.iter().enumerate() {
            let mut row = format!("  {}) {}", index + 1, item.label);
            if let Some(description) = &item.description {
                row.push_str(&format!(" — {description}"));
            }
            io.write(&format!("{row}\n"));
        }
        io.write(if self.multi {
            "Numbers or names, separated by commas: "
        } else {
            "Number or name: "
        });
        let Some(line) = io.read_line() else {
            return Ok(None);
        };
        let answers: Vec<&str> = if self.multi {
            line.split(',')
                .map(str::trim)
                .filter(|a| !a.is_empty())
                .collect()
        } else {
            vec![line.trim()]
        };
        let texts: Vec<String> = self.items.iter().map(Item::search_text).collect();
        let mut chosen = Vec::new();
        for answer in answers {
            let index = match answer.parse::<usize>() {
                Ok(number) if (1..=self.items.len()).contains(&number) => number - 1,
                _ => rank(answer, texts.iter().map(String::as_str))
                    .first()
                    .map(|(index, _)| *index)
                    .filter(|_| !answer.is_empty())
                    .ok_or_else(|| {
                        NotInteractive::Invalid(format!("no item matches {answer:?}"))
                    })?,
            };
            if !chosen.contains(&index) {
                chosen.push(index);
            }
        }
        if chosen.is_empty() {
            return Err(NotInteractive::Invalid("nothing chosen".into()));
        }
        Ok(Some(chosen))
    }
}

impl<T: Clone> Component for Select<T> {
    type Output = T;

    fn handle(&mut self, event: &Event, _: &Context<'_>) -> Flow<T> {
        match self.event(event) {
            Some(Flow::Done(indices)) => Flow::Done(self.items[indices[0]].value.clone()),
            Some(Flow::Cancel) => Flow::Cancel,
            _ => Flow::Continue,
        }
    }

    fn render(&self, context: &Context<'_>) -> View {
        self.render_view(context)
    }

    fn default_value(&self) -> Option<T> {
        self.default.map(|index| self.items[index].value.clone())
    }

    fn prompt(&mut self, io: &mut dyn LineIo) -> Result<Option<T>, NotInteractive> {
        Ok(self
            .ask(io)?
            .map(|indices| self.items[indices[0]].value.clone()))
    }
}

/// A fuzzy multiple choice: Tab marks items (Shift+Tab marks and moves up,
/// Ctrl+A marks every match), Enter returns the marked items in list
/// order, or the focused one when none is marked.
pub struct MultiSelect<T>(Select<T>);

impl<T> MultiSelect<T> {
    pub fn new<I, V>(prompt: impl Into<String>, items: I) -> MultiSelect<T>
    where
        I: IntoIterator<Item = V>,
        V: Into<Item<T>>,
    {
        let mut select = Select::new(prompt, items);
        select.multi = true;
        MultiSelect(select)
    }

    pub fn height(self, rows: usize) -> Self {
        MultiSelect(self.0.height(rows))
    }

    pub fn preview(self, layout: PreviewLayout) -> Self {
        MultiSelect(self.0.preview(layout))
    }

    pub fn theme(self, theme: Theme) -> Self {
        MultiSelect(self.0.theme(theme))
    }

    pub fn query(self, query: impl Into<String>) -> Self {
        MultiSelect(self.0.query(query))
    }

    /// Mark these items to begin with; they are also the default without a
    /// terminal.
    pub fn marked(mut self, indices: impl IntoIterator<Item = usize>) -> Self {
        for index in indices {
            if let Some(mark) = self.0.marked.get_mut(index) {
                *mark = true;
            }
        }
        self
    }

    pub fn action(&self) -> Option<&str> {
        self.0.action()
    }

    pub fn items(&self) -> &[Item<T>] {
        self.0.items()
    }
}

impl<T: Clone> Component for MultiSelect<T> {
    type Output = Vec<T>;

    fn handle(&mut self, event: &Event, _: &Context<'_>) -> Flow<Vec<T>> {
        match self.0.event(event) {
            Some(Flow::Done(indices)) => Flow::Done(
                indices
                    .into_iter()
                    .map(|index| self.0.items[index].value.clone())
                    .collect(),
            ),
            Some(Flow::Cancel) => Flow::Cancel,
            _ => Flow::Continue,
        }
    }

    fn render(&self, context: &Context<'_>) -> View {
        self.0.render_view(context)
    }

    fn default_value(&self) -> Option<Vec<T>> {
        Some(
            (0..self.0.items.len())
                .filter(|&index| self.0.marked[index])
                .map(|index| self.0.items[index].value.clone())
                .collect(),
        )
    }

    fn prompt(&mut self, io: &mut dyn LineIo) -> Result<Option<Vec<T>>, NotInteractive> {
        Ok(self.0.ask(io)?.map(|indices| {
            indices
                .into_iter()
                .map(|index| self.0.items[index].value.clone())
                .collect()
        }))
    }
}
