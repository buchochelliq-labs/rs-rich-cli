//! A confirmation sheet (#470): what will happen, then a choice.
//!
//! The body is any renderables (a diff, a table of affected files, a
//! summary) in a scrollable viewport, warnings are listed under it, and the
//! choices are more than yes and no when the caller needs them (`Apply`,
//! `Apply all`, `Skip`, `Cancel`). A choice's key picks it directly; Left,
//! Right and Tab move between them and Enter picks the focused one.

use std::sync::Arc;

use rich::{Renderable, Segment, Style};

use crate::component::{Component, Context, Flow, View};
use crate::components::{fit, plain, question, text, Theme};
use crate::event::{Event, KeyCode};
use crate::policy::{LineIo, NotInteractive};
use crate::viewport::Viewport;

/// One answer: an id the caller matches on, its label and its key.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Choice {
    pub id: String,
    pub label: String,
    pub key: char,
}

impl Choice {
    pub fn new(id: impl Into<String>, label: impl Into<String>, key: char) -> Choice {
        Choice {
            id: id.into(),
            label: label.into(),
            key,
        }
    }
}

/// A question with a body, warnings and choices. Returns the chosen id.
pub struct Confirm {
    title: String,
    body: Vec<Arc<dyn Renderable + Send + Sync>>,
    warnings: Vec<String>,
    choices: Vec<Choice>,
    focus: usize,
    default: Option<usize>,
    viewport: Viewport,
    /// Rows the body may take; the rest of the terminal otherwise.
    body_height: Option<usize>,
    theme: Theme,
    answer: Option<Option<String>>,
}

impl Confirm {
    /// A yes-or-no question (`y`, `n`), answering `"yes"` or `"no"`.
    pub fn new(title: impl Into<String>) -> Confirm {
        Confirm {
            title: title.into(),
            body: Vec::new(),
            warnings: Vec::new(),
            choices: vec![Choice::new("yes", "Yes", 'y'), Choice::new("no", "No", 'n')],
            focus: 0,
            default: None,
            viewport: Viewport::default(),
            body_height: None,
            theme: Theme::default(),
            answer: None,
        }
    }

    /// Show `renderable` in the body, after anything added before.
    pub fn body(mut self, renderable: impl Renderable + Send + Sync + 'static) -> Self {
        self.body.push(Arc::new(renderable));
        self
    }

    /// A warning line under the body.
    pub fn warning(mut self, warning: impl Into<String>) -> Self {
        self.warnings.push(warning.into());
        self
    }

    /// Replace the choices.
    pub fn choices(mut self, choices: impl IntoIterator<Item = Choice>) -> Self {
        self.choices = choices.into_iter().collect();
        self.focus = 0;
        self.default = None;
        self
    }

    /// Focus this choice first, and answer it without a terminal when the
    /// policy asks for defaults.
    pub fn default(mut self, id: &str) -> Self {
        if let Some(index) = self.choices.iter().position(|choice| choice.id == id) {
            self.focus = index;
            self.default = Some(index);
        }
        self
    }

    /// Let the body take at most `rows` rows.
    pub fn body_height(mut self, rows: usize) -> Self {
        self.body_height = Some(rows.max(1));
        self
    }

    pub fn theme(mut self, theme: Theme) -> Self {
        self.theme = theme;
        self
    }

    fn page(&self, context: &Context<'_>) -> usize {
        // The question, warnings, the choices and the hint line stay.
        let fixed = 3 + self.warnings.len();
        let room = context.height.saturating_sub(fixed).max(1);
        self.body_height.unwrap_or(room).min(room)
    }

    fn lines(&self, context: &Context<'_>) -> Vec<Vec<Segment>> {
        let width = context.width.saturating_sub(2).max(1);
        self.body
            .iter()
            .flat_map(|renderable| context.lines_at(&**renderable, width))
            .collect()
    }

    fn pick(&mut self, index: usize) -> Flow<String> {
        let id = self.choices[index].id.clone();
        self.answer = Some(Some(self.choices[index].label.clone()));
        Flow::Done(id)
    }
}

impl Component for Confirm {
    type Output = String;

    fn handle(&mut self, event: &Event, context: &Context<'_>) -> Flow<String> {
        if self.viewport.is_empty() && !self.body.is_empty() {
            self.viewport.set_lines(self.lines(context));
        }
        if let Event::Resize { .. } = event {
            self.viewport.set_lines(self.lines(context));
        }
        let page = self.page(context);
        let Some(key) = event.key() else {
            self.viewport.handle_scroll(event, page);
            return Flow::Continue;
        };
        if !key.modifiers.ctrl && !key.modifiers.alt {
            if let KeyCode::Char(c) = key.code {
                let lower = c.to_ascii_lowercase();
                if let Some(index) = self
                    .choices
                    .iter()
                    .position(|choice| choice.key.to_ascii_lowercase() == lower)
                {
                    return self.pick(index);
                }
            }
        }
        let count = self.choices.len();
        match key.code {
            KeyCode::Enter if count > 0 => return self.pick(self.focus),
            KeyCode::Escape => {
                self.answer = Some(None);
                return Flow::Cancel;
            }
            KeyCode::Left | KeyCode::BackTab => {
                self.focus = (self.focus + count - 1) % count.max(1)
            }
            KeyCode::Right | KeyCode::Tab => self.focus = (self.focus + 1) % count.max(1),
            _ => {
                self.viewport.handle_scroll(event, page);
            }
        }
        Flow::Continue
    }

    fn render(&self, context: &Context<'_>) -> View {
        let width = context.width;
        let theme = &self.theme;
        let mut title = question(theme, &self.title);
        // `question` ends with ` › ` for an answer; drop it until one.
        title.truncate(title.len() - 3);
        if let Some(answer) = &self.answer {
            title.push(plain(" "));
            title.push(text("›", &theme.hint));
            title.push(plain(" "));
            title.push(match answer {
                Some(answer) => text(answer.clone(), &theme.answer),
                None => text("cancelled", &theme.hint),
            });
            return View::new(vec![fit(title, width)]);
        }
        let mut lines = vec![fit(title, width)];
        let page = self.page(context);
        // Before the first event the viewport is empty: render the body
        // directly, from the top.
        let body = if self.viewport.is_empty() {
            self.lines(context)
        } else {
            self.viewport.lines().to_vec()
        };
        let offset = if self.viewport.is_empty() {
            0
        } else {
            self.viewport.offset()
        };
        for line in body.iter().skip(offset).take(page) {
            let mut row = vec![plain("  ")];
            row.extend(line.iter().cloned());
            lines.push(fit(row, width));
        }
        for warning in &self.warnings {
            let style = Style::parse("yellow").expect("style");
            lines.push(fit(vec![text(format!("  ⚠ {warning}"), &style)], width));
        }
        let mut row = vec![plain("  ")];
        for (index, choice) in self.choices.iter().enumerate() {
            let label = format!(" {} ", choice.label);
            row.push(if index == self.focus {
                text(
                    label,
                    &theme
                        .focused
                        .combine(&Style::parse("reverse").expect("style")),
                )
            } else {
                plain(label)
            });
            row.push(plain(" "));
        }
        lines.push(fit(row, width));
        let keys: Vec<String> = self
            .choices
            .iter()
            .map(|choice| choice.key.to_string())
            .collect();
        let mut hint = format!("  {} · ←→ move · enter choose · esc cancel", keys.join("/"));
        if body.len() > page {
            hint.push_str(&format!(" · ↑↓ scroll ({})", self.viewport.status(page)));
        }
        lines.push(fit(vec![text(hint, &theme.hint)], width));
        View::new(lines)
    }

    fn default_value(&self) -> Option<String> {
        self.default.map(|index| self.choices[index].id.clone())
    }

    fn prompt(&mut self, io: &mut dyn LineIo) -> Result<Option<String>, NotInteractive> {
        for warning in &self.warnings {
            io.write(&format!("warning: {warning}\n"));
        }
        let keys: Vec<String> = self
            .choices
            .iter()
            .map(|choice| format!("{}={}", choice.key, choice.label))
            .collect();
        io.write(&format!("{} [{}]: ", self.title, keys.join(", ")));
        let Some(line) = io.read_line() else {
            return Ok(None);
        };
        let answer = line.trim().to_lowercase();
        if answer.is_empty() {
            if let Some(index) = self.default {
                return Ok(Some(self.choices[index].id.clone()));
            }
        }
        self.choices
            .iter()
            .find(|choice| {
                answer == choice.key.to_string()
                    || answer == choice.id.to_lowercase()
                    || answer == choice.label.to_lowercase()
            })
            .map(|choice| Some(choice.id.clone()))
            .ok_or_else(|| NotInteractive::Invalid(format!("not one of the choices: {line:?}")))
    }
}
