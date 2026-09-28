//! Forms (#288): several fields answered together, with each field's
//! error shown under it (#472).
//!
//! Fields are text (an [`Input`], with its validation, placeholder and
//! default), masked fields for passwords, a choice among options, and yes/no toggles. Tab and
//! the arrows move between fields; Enter moves to the next field and, on the
//! last, submits. Submitting checks every field and focuses the first that
//! fails, with its message under it.

use std::time::Duration;

use rich::Segment;

use crate::component::{Component, Context, Flow, View};
use crate::components::{fit, plain, question, text, Input, Theme};
use crate::event::{Event, KeyCode};
use crate::policy::{LineIo, NotInteractive};

/// A field's answer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Value {
    Text(String),
    Flag(bool),
}

/// A submitted form's answers, by field name, in field order.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Answers(pub Vec<(String, Value)>);

impl Answers {
    pub fn get(&self, name: &str) -> Option<&Value> {
        self.0
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value)
    }

    /// A text or choice field's answer.
    pub fn text(&self, name: &str) -> Option<&str> {
        match self.get(name)? {
            Value::Text(text) => Some(text),
            Value::Flag(_) => None,
        }
    }

    /// A toggle's answer.
    pub fn flag(&self, name: &str) -> Option<bool> {
        match self.get(name)? {
            Value::Flag(flag) => Some(*flag),
            Value::Text(_) => None,
        }
    }
}

enum Kind {
    Text(Box<Input>),
    Choice { options: Vec<String>, index: usize },
    Toggle(bool),
}

struct Field {
    name: String,
    label: String,
    kind: Kind,
    error: Option<String>,
}

impl Field {
    fn value(&self) -> Result<Value, String> {
        match &self.kind {
            Kind::Text(input) => input.resolve().map(Value::Text),
            Kind::Choice { options, index } => Ok(Value::Text(
                options.get(*index).cloned().unwrap_or_default(),
            )),
            Kind::Toggle(on) => Ok(Value::Flag(*on)),
        }
    }

    /// The answer as shown once submitted.
    fn summary(&self) -> String {
        match (&self.kind, self.value()) {
            (Kind::Text(input), Ok(Value::Text(text))) => input.display_value(&text),
            (_, Ok(Value::Text(text))) => text,
            (_, Ok(Value::Flag(on))) => if on { "yes" } else { "no" }.to_string(),
            (_, Err(_)) => String::new(),
        }
    }
}

/// A form: named fields answered together. Returns [`Answers`].
pub struct Form {
    title: String,
    fields: Vec<Field>,
    focus: usize,
    theme: Theme,
    answer: Option<bool>,
    mouse: bool,
}

/// What a row of the form is, for a click.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Spot {
    Field(usize),
    Submit,
    Cancel,
}

const SUBMIT: &str = " Submit ";
const CANCEL: &str = " Cancel ";

impl Form {
    pub fn new(title: impl Into<String>) -> Form {
        Form {
            title: title.into(),
            fields: Vec::new(),
            focus: 0,
            theme: Theme::default(),
            answer: None,
            mouse: false,
        }
    }

    /// Report the mouse (#476): a click focuses a field (and flips a
    /// toggle or moves a choice on), and Submit and Cancel buttons show
    /// under the fields.
    pub fn with_mouse(mut self, on: bool) -> Self {
        self.mouse = on;
        self
    }

    /// What is at `row` and `column` of the view at `width`, as `render`
    /// lays it out.
    fn spot(&self, width: usize, row: usize, column: usize) -> Option<Spot> {
        let label_width = self
            .fields
            .iter()
            .map(|field| rich::cells::cell_len(&field.label))
            .max()
            .unwrap_or(0);
        let mut at = 1;
        for (index, field) in self.fields.iter().enumerate() {
            if row == at {
                return Some(Spot::Field(index));
            }
            at += 1;
            if let (true, Kind::Text(input)) = (index == self.focus, &field.kind) {
                at += input
                    .suggestion_rows(width, &" ".repeat(label_width + 2))
                    .len();
            }
            at += usize::from(field.error.is_some());
        }
        if !self.mouse || row != at {
            return None;
        }
        let submit = 2..2 + SUBMIT.len();
        let cancel = submit.end + 2..submit.end + 2 + CANCEL.len();
        if submit.contains(&column) {
            Some(Spot::Submit)
        } else if cancel.contains(&column) {
            Some(Spot::Cancel)
        } else {
            None
        }
    }

    fn click(&mut self, width: usize, row: usize, column: usize) -> Flow<Answers> {
        match self.spot(width, row, column) {
            Some(Spot::Submit) => return self.submit(),
            Some(Spot::Cancel) => {
                self.answer = Some(false);
                return Flow::Cancel;
            }
            Some(Spot::Field(index)) => {
                let again = index == self.focus;
                self.focus = index;
                match &mut self.fields[index].kind {
                    Kind::Toggle(on) => *on = !*on,
                    Kind::Choice { options, index } if again => {
                        *index = (*index + 1) % options.len().max(1)
                    }
                    _ => {}
                }
            }
            None => {}
        }
        Flow::Continue
    }

    fn push(mut self, name: impl Into<String>, label: String, kind: Kind) -> Self {
        self.fields.push(Field {
            name: name.into(),
            label,
            kind,
            error: None,
        });
        self
    }

    /// A text field.
    pub fn text(self, name: impl Into<String>, label: impl Into<String>) -> Self {
        let label = label.into();
        let input = Input::new(label.clone());
        self.push(name, label, Kind::Text(Box::new(input)))
    }

    /// A text field from a configured [`Input`] (validation, placeholder,
    /// default, suggestions); its prompt is the label.
    pub fn input(self, name: impl Into<String>, input: Input) -> Self {
        let label = input.label().to_string();
        self.push(name, label, Kind::Text(Box::new(input)))
    }

    /// A masked text field, for passwords and tokens (see
    /// [`Input::masked`]).
    pub fn masked(self, name: impl Into<String>, label: impl Into<String>) -> Self {
        let label = label.into();
        let input = Input::masked(label.clone());
        self.push(name, label, Kind::Text(Box::new(input)))
    }

    /// One of `options`, changed with Left and Right (or Space).
    pub fn choice<I, S>(self, name: impl Into<String>, label: impl Into<String>, options: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        let options = options.into_iter().map(Into::into).collect();
        self.push(name, label.into(), Kind::Choice { options, index: 0 })
    }

    /// Yes or no, changed with Space, Left, Right, `y` and `n`.
    pub fn toggle(self, name: impl Into<String>, label: impl Into<String>, on: bool) -> Self {
        self.push(name, label.into(), Kind::Toggle(on))
    }

    pub fn theme(mut self, theme: Theme) -> Self {
        self.theme = theme;
        self
    }

    fn move_focus(&mut self, delta: isize) {
        let count = self.fields.len();
        if count > 0 {
            self.focus = (self.focus as isize + delta).rem_euclid(count as isize) as usize;
        }
    }

    fn submit(&mut self) -> Flow<Answers> {
        let mut answers = Vec::new();
        let mut first_error = None;
        for (index, field) in self.fields.iter_mut().enumerate() {
            match field.value() {
                Ok(value) => {
                    field.error = None;
                    answers.push((field.name.clone(), value));
                }
                Err(message) => {
                    field.error = Some(message);
                    first_error.get_or_insert(index);
                }
            }
        }
        if let Some(index) = first_error {
            self.focus = index;
            return Flow::Continue;
        }
        self.answer = Some(true);
        Flow::Done(Answers(answers))
    }
}

impl Component for Form {
    type Output = Answers;

    /// A form with no fields has nothing to ask: done at once.
    fn start(&mut self, _: &Context<'_>) -> Flow<Answers> {
        if self.fields.is_empty() {
            self.answer = Some(true);
            return Flow::Done(Answers::default());
        }
        Flow::Continue
    }

    fn handle(&mut self, event: &Event, context: &Context<'_>) -> Flow<Answers> {
        if self.fields.is_empty() {
            return Flow::Done(Answers::default());
        }
        if let Some(mouse) = event.mouse() {
            if mouse.is_click() {
                return self.click(context.width, mouse.row as usize, mouse.column as usize);
            }
            return Flow::Continue;
        }
        if let Event::Tick = event {
            // Provider results for every text field, not only the focused.
            for field in &mut self.fields {
                if let Kind::Text(input) = &mut field.kind {
                    input.handle(event, context);
                }
            }
            return Flow::Continue;
        }
        // While a text field shows suggestions, the keys that pick one are
        // its own: Tab and the arrows, and Enter with one selected.
        let field = &mut self.fields[self.focus];
        if let (Some(key), Kind::Text(input)) = (event.key(), &mut field.kind) {
            if input.suggesting() {
                match key.code {
                    KeyCode::Tab | KeyCode::Up | KeyCode::Down => {
                        input.handle(event, context);
                        field.error = None;
                        return Flow::Continue;
                    }
                    KeyCode::Enter if input.has_selection() => {
                        input.accept_selected();
                        field.error = None;
                        return Flow::Continue;
                    }
                    _ => {}
                }
            }
        }
        if let Some(key) = event.key() {
            let last = self.focus + 1 == self.fields.len();
            match key.code {
                KeyCode::Escape => {
                    self.answer = Some(false);
                    return Flow::Cancel;
                }
                KeyCode::Enter if last => return self.submit(),
                KeyCode::Enter | KeyCode::Tab | KeyCode::Down => {
                    self.move_focus(1);
                    return Flow::Continue;
                }
                KeyCode::BackTab | KeyCode::Up => {
                    self.move_focus(-1);
                    return Flow::Continue;
                }
                KeyCode::Char('s') if key.modifiers.ctrl => return self.submit(),
                _ => {}
            }
        }
        let field = &mut self.fields[self.focus];
        match &mut field.kind {
            Kind::Text(input) => {
                let before = input.text().to_string();
                input.handle(event, context);
                if input.text() != before {
                    field.error = None;
                }
            }
            Kind::Choice { options, index } => {
                let count = options.len().max(1);
                match event.key().map(|key| key.code) {
                    Some(KeyCode::Right | KeyCode::Char(' ')) => *index = (*index + 1) % count,
                    Some(KeyCode::Left) => *index = (*index + count - 1) % count,
                    _ => {}
                }
            }
            Kind::Toggle(on) => match event.key().map(|key| key.code) {
                Some(KeyCode::Char(' ') | KeyCode::Left | KeyCode::Right) => *on = !*on,
                Some(KeyCode::Char('y')) => *on = true,
                Some(KeyCode::Char('n')) => *on = false,
                _ => {}
            },
        }
        Flow::Continue
    }

    fn render(&self, context: &Context<'_>) -> View {
        let width = context.width;
        let theme = &self.theme;
        let mut title = question(theme, &self.title);
        title.truncate(title.len() - 3);
        let label_width = self
            .fields
            .iter()
            .map(|field| rich::cells::cell_len(&field.label))
            .max()
            .unwrap_or(0);
        if let Some(submitted) = self.answer {
            if !submitted {
                title.push(text(" › cancelled", &theme.hint));
                return View::new(vec![fit(title, width)]);
            }
            let mut lines = vec![fit(title, width)];
            for field in &self.fields {
                let label = format!("  {:width$}  ", field.label, width = label_width);
                lines.push(fit(
                    vec![
                        text(label, &theme.hint),
                        text(field.summary(), &theme.answer),
                    ],
                    width,
                ));
            }
            return View::new(lines);
        }
        let mut lines = vec![fit(title, width)];
        let mut cursor = None;
        for (index, field) in self.fields.iter().enumerate() {
            let focused = index == self.focus;
            let mut row: Vec<Segment> = if focused {
                vec![text(format!("{} ", theme.pointer), &theme.pointer_style)]
            } else {
                vec![plain("  ")]
            };
            let padding = label_width - rich::cells::cell_len(&field.label);
            row.push(if focused {
                text(field.label.clone(), &theme.focused)
            } else {
                plain(field.label.clone())
            });
            row.push(plain(format!("{}  ", " ".repeat(padding))));
            let start = crate::components::width(&row);
            match &field.kind {
                Kind::Text(input) => {
                    let (segments, column) = input.field(width.saturating_sub(start));
                    row.extend(segments);
                    if focused {
                        cursor = Some((lines.len(), (start + column).min(width.saturating_sub(1))));
                    }
                }
                Kind::Choice { options, index } => {
                    let option = options.get(*index).cloned().unwrap_or_default();
                    if focused {
                        row.push(text("◂ ", &theme.hint));
                        row.push(text(option, &theme.answer));
                        row.push(text(" ▸", &theme.hint));
                    } else {
                        row.push(text(option, &theme.answer));
                    }
                }
                Kind::Toggle(on) => row.push(if *on {
                    text(format!("{} yes", theme.checked), &theme.checked_style)
                } else {
                    text(format!("{} no", theme.unchecked), &theme.hint)
                }),
            }
            lines.push(fit(row, width));
            if let (true, Kind::Text(input)) = (focused, &field.kind) {
                lines.extend(input.suggestion_rows(width, &" ".repeat(label_width + 2)));
            }
            if let Some(error) = &field.error {
                let indent = " ".repeat(label_width + 4);
                lines.push(fit(
                    vec![text(format!("{indent}✗ {error}"), &theme.error)],
                    width,
                ));
            }
        }
        if self.mouse {
            let button = theme
                .focused
                .combine(&rich::Style::parse("reverse").expect("style"));
            lines.push(fit(
                vec![
                    plain("  "),
                    text(SUBMIT, &button),
                    plain("  "),
                    text(CANCEL, &button),
                ],
                width,
            ));
        }
        let last = self.focus + 1 == self.fields.len();
        let enter = if last { "enter submit" } else { "enter next" };
        lines.push(fit(
            vec![text(
                format!("  tab/↑↓ move · {enter} · ctrl+s submit · esc cancel"),
                &theme.hint,
            )],
            width,
        ));
        let view = View::new(lines);
        match cursor {
            Some((row, column)) => view.with_cursor(row, column),
            None => view,
        }
    }

    fn mouse(&self) -> bool {
        self.mouse
    }

    /// Ticks for text fields with a suggestion provider, so its results
    /// arrive.
    fn tick(&self) -> Option<Duration> {
        self.fields
            .iter()
            .filter_map(|field| match &field.kind {
                Kind::Text(input) => input.tick(),
                _ => None,
            })
            .min()
    }

    fn prompt(&mut self, io: &mut dyn LineIo) -> Result<Option<Answers>, NotInteractive> {
        io.write(&format!("{}\n", self.title));
        let mut answers = Vec::new();
        for field in &mut self.fields {
            let value = match &mut field.kind {
                Kind::Text(input) => match input.prompt(io)? {
                    Some(text) => Value::Text(text),
                    None => return Ok(None),
                },
                Kind::Choice { options, index } => {
                    io.write(&format!(
                        "{}: {} [{}]: ",
                        field.label,
                        options.join(" / "),
                        options[*index]
                    ));
                    // At the end of input, the current option answers, as
                    // for an empty line.
                    let line = io.read_line().unwrap_or_default();
                    let line = line.trim();
                    if line.is_empty() {
                        Value::Text(options[*index].clone())
                    } else {
                        let found = options
                            .iter()
                            .find(|option| option.eq_ignore_ascii_case(line))
                            .ok_or_else(|| {
                                NotInteractive::Invalid(format!("not an option: {line:?}"))
                            })?;
                        Value::Text(found.clone())
                    }
                }
                Kind::Toggle(on) => {
                    io.write(&format!(
                        "{} [{}]: ",
                        field.label,
                        if *on { "Y/n" } else { "y/N" }
                    ));
                    let line = io.read_line().unwrap_or_default();
                    Value::Flag(match line.trim().to_lowercase().as_str() {
                        "" => *on,
                        "y" | "yes" => true,
                        "n" | "no" => false,
                        other => {
                            return Err(NotInteractive::Invalid(format!(
                                "not yes or no: {other:?}"
                            )))
                        }
                    })
                }
            };
            answers.push((field.name.clone(), value));
        }
        Ok(Some(Answers(answers)))
    }
}
