//! Text input with validation, history and suggestions (#457, #289).
//!
//! The line edits like a shell's: the arrows, Home/End, Ctrl+A/E, Ctrl+U,
//! Ctrl+W, Backspace and Delete. A validator's message shows under the line
//! and Enter waits until it passes. Up and Down walk the history, or the
//! suggestions when some are showing; Tab accepts one. Suggestions come from
//! a fixed list, filtered as you type, or from a provider called on a
//! background thread, so a slow lookup never stalls typing.

use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;
use std::time::Duration;

use rich::cells::cell_len;
use rich::Segment;

use crate::component::{Component, Context, Flow, View};
use crate::components::{fit, highlight, plain, question, text, Theme};
use crate::event::{Event, KeyCode};
use crate::fuzzy::rank;
use crate::policy::{LineIo, NotInteractive};

/// A completion: the text it inserts, and an optional note beside it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Suggestion {
    pub value: String,
    pub description: Option<String>,
}

impl Suggestion {
    pub fn new(value: impl Into<String>) -> Suggestion {
        Suggestion {
            value: value.into(),
            description: None,
        }
    }

    pub fn description(mut self, description: impl Into<String>) -> Suggestion {
        self.description = Some(description.into());
        self
    }
}

impl From<&str> for Suggestion {
    fn from(value: &str) -> Suggestion {
        Suggestion::new(value)
    }
}

impl From<String> for Suggestion {
    fn from(value: String) -> Suggestion {
        Suggestion::new(value)
    }
}

/// Suggestions for what is typed so far, computed off the event thread.
pub type Provider = Arc<dyn Fn(&str) -> Vec<Suggestion> + Send + Sync>;
type Validator = Box<dyn Fn(&str) -> Result<(), String>>;

/// One line of text.
pub struct Input {
    prompt: String,
    value: String,
    /// The caret, in characters.
    caret: usize,
    placeholder: Option<String>,
    default: Option<String>,
    help: Option<String>,
    mask: Option<char>,
    validator: Option<Validator>,
    error: Option<String>,
    history: Vec<String>,
    /// Where Up has got to in the history, and what was typed before.
    recall: Option<(usize, String)>,
    suggestions: Vec<Suggestion>,
    provider: Option<Provider>,
    /// (generation, results) from provider threads.
    sender: Sender<(u64, Vec<Suggestion>)>,
    receiver: Receiver<(u64, Vec<Suggestion>)>,
    generation: u64,
    pending: bool,
    /// Suggestions showing, with the characters to highlight.
    shown: Vec<(Suggestion, Vec<usize>)>,
    selected: Option<usize>,
    limit: usize,
    theme: Theme,
    answer: Option<Option<String>>,
}

impl Input {
    pub fn new(prompt: impl Into<String>) -> Input {
        let (sender, receiver) = channel();
        Input {
            prompt: prompt.into(),
            value: String::new(),
            caret: 0,
            placeholder: None,
            default: None,
            help: None,
            mask: None,
            validator: None,
            error: None,
            history: Vec::new(),
            recall: None,
            suggestions: Vec::new(),
            provider: None,
            sender,
            receiver,
            generation: 0,
            pending: false,
            shown: Vec::new(),
            selected: None,
            limit: 5,
            theme: Theme::default(),
            answer: None,
        }
    }

    /// A password: shown as `•`, never echoed without a terminal's help.
    pub fn password(prompt: impl Into<String>) -> Input {
        Input::new(prompt).mask('•')
    }

    pub fn mask(mut self, mask: char) -> Self {
        self.mask = Some(mask);
        self
    }

    /// Start with this text.
    pub fn value(mut self, value: impl Into<String>) -> Self {
        self.value = value.into();
        self.caret = self.value.chars().count();
        self.refresh();
        self
    }

    /// Dim text shown while the line is empty.
    pub fn placeholder(mut self, placeholder: impl Into<String>) -> Self {
        self.placeholder = Some(placeholder.into());
        self
    }

    /// The answer when Enter is pressed on an empty line, and without a
    /// terminal when the policy asks for defaults.
    pub fn default(mut self, default: impl Into<String>) -> Self {
        self.default = Some(default.into());
        self
    }

    /// A line of help under the input.
    pub fn help(mut self, help: impl Into<String>) -> Self {
        self.help = Some(help.into());
        self
    }

    /// Check the answer on Enter; an `Err` message shows under the line.
    pub fn validate(mut self, validator: impl Fn(&str) -> Result<(), String> + 'static) -> Self {
        self.validator = Some(Box::new(validator));
        self
    }

    /// Earlier answers, oldest first, for Up and Down.
    pub fn history<I: IntoIterator<Item = S>, S: Into<String>>(mut self, history: I) -> Self {
        self.history = history.into_iter().map(Into::into).collect();
        self
    }

    /// Fixed suggestions, filtered by what is typed.
    pub fn suggestions<I: IntoIterator<Item = S>, S: Into<Suggestion>>(
        mut self,
        suggestions: I,
    ) -> Self {
        self.suggestions = suggestions.into_iter().map(Into::into).collect();
        self.refresh();
        self
    }

    /// Suggestions from `provider`, called on a background thread after
    /// each change; results for text since changed are dropped.
    pub fn provider(
        mut self,
        provider: impl Fn(&str) -> Vec<Suggestion> + Send + Sync + 'static,
    ) -> Self {
        self.provider = Some(Arc::new(provider));
        self.refresh();
        self
    }

    /// Show at most `rows` suggestions (default 5).
    pub fn limit(mut self, rows: usize) -> Self {
        self.limit = rows.max(1);
        self
    }

    pub fn theme(mut self, theme: Theme) -> Self {
        self.theme = theme;
        self
    }

    /// The text typed so far.
    pub fn text(&self) -> &str {
        &self.value
    }

    fn byte(&self, caret: usize) -> usize {
        self.value
            .char_indices()
            .nth(caret)
            .map_or(self.value.len(), |(index, _)| index)
    }

    fn insert(&mut self, text: &str) {
        let at = self.byte(self.caret);
        self.value.insert_str(at, text);
        self.caret += text.chars().count();
        self.changed();
    }

    fn changed(&mut self) {
        self.error = None;
        self.recall = None;
        self.refresh();
    }

    /// Recompute the suggestions for the current text.
    fn refresh(&mut self) {
        self.selected = None;
        if let Some(provider) = &self.provider {
            self.generation += 1;
            self.pending = true;
            let (provider, sender, generation) =
                (Arc::clone(provider), self.sender.clone(), self.generation);
            let query = self.value.clone();
            std::thread::spawn(move || {
                let _ = sender.send((generation, provider(&query)));
            });
            // Until the new results come, keep only what still fits.
            let current: Vec<Suggestion> = self.shown.iter().map(|(s, _)| s.clone()).collect();
            self.show(current);
            return;
        }
        self.show(self.suggestions.clone());
    }

    fn show(&mut self, suggestions: Vec<Suggestion>) {
        let texts: Vec<&str> = suggestions.iter().map(|s| s.value.as_str()).collect();
        self.shown = if self.value.is_empty() {
            Vec::new()
        } else {
            rank(&self.value, texts)
                .into_iter()
                .map(|(index, found)| (suggestions[index].clone(), found.positions))
                .filter(|(suggestion, _)| suggestion.value != self.value)
                .take(self.limit)
                .collect()
        };
    }

    /// Take results a provider thread has sent, keeping only the latest.
    fn receive(&mut self) {
        let mut latest = None;
        // A moment's wait for a pending lookup, so a fast provider's
        // results show on the next tick rather than the one after.
        if self.pending {
            if let Ok((generation, results)) = self.receiver.recv_timeout(Duration::from_millis(5))
            {
                if generation == self.generation {
                    latest = Some(results);
                }
            }
        }
        while let Ok((generation, results)) = self.receiver.try_recv() {
            if generation == self.generation {
                latest = Some(results);
            }
        }
        if let Some(results) = latest {
            self.pending = false;
            self.show(results);
        }
    }

    fn accept(&mut self, index: usize) {
        if let Some((suggestion, _)) = self.shown.get(index) {
            self.value = suggestion.value.clone();
            self.caret = self.value.chars().count();
            self.changed();
        }
    }

    fn recall(&mut self, older: bool) {
        if self.history.is_empty() {
            return;
        }
        let next = match (&self.recall, older) {
            (None, true) => Some(self.history.len() - 1),
            (None, false) => return,
            (Some((0, _)), true) => Some(0),
            (Some((index, _)), true) => Some(index - 1),
            (Some((index, _)), false) if index + 1 < self.history.len() => Some(index + 1),
            (Some(_), false) => None,
        };
        let typed = match self.recall.take() {
            Some((_, typed)) => typed,
            None => self.value.clone(),
        };
        self.value = match next {
            Some(index) => self.history[index].clone(),
            None => typed.clone(),
        };
        self.caret = self.value.chars().count();
        self.error = None;
        self.recall = next.map(|index| (index, typed));
    }

    fn submit(&mut self) -> Flow<String> {
        if let Some(index) = self.selected {
            self.accept(index);
        }
        let mut answer = self.value.clone();
        if answer.is_empty() {
            if let Some(default) = &self.default {
                answer = default.clone();
            }
        }
        if let Some(validator) = &self.validator {
            if let Err(message) = validator(&answer) {
                self.error = Some(message);
                return Flow::Continue;
            }
        }
        self.answer = Some(Some(answer.clone()));
        Flow::Done(answer)
    }

    fn shown_value(&self, value: &str) -> String {
        match self.mask {
            Some(mask) => std::iter::repeat_n(mask, value.chars().count()).collect(),
            None => value.to_string(),
        }
    }
}

impl Component for Input {
    type Output = String;

    fn handle(&mut self, event: &Event, _: &Context<'_>) -> Flow<String> {
        if let Event::Tick = event {
            self.receive();
            return Flow::Continue;
        }
        if let Event::Paste(text) = event {
            self.insert(&text.replace(['\n', '\r'], " "));
            return Flow::Continue;
        }
        let Some(key) = event.key() else {
            return Flow::Continue;
        };
        let ctrl = key.modifiers.ctrl;
        let length = self.value.chars().count();
        match key.code {
            KeyCode::Enter => return self.submit(),
            KeyCode::Escape => {
                self.answer = Some(None);
                return Flow::Cancel;
            }
            KeyCode::Tab => {
                let index = self.selected.unwrap_or(0);
                self.accept(index);
            }
            KeyCode::Up | KeyCode::Down if !self.shown.is_empty() => {
                let last = self.shown.len() - 1;
                self.selected = Some(match (self.selected, key.code == KeyCode::Up) {
                    (None, true) => last,
                    (None, false) => 0,
                    (Some(0), true) => last,
                    (Some(index), true) => index - 1,
                    (Some(index), false) => (index + 1) % (last + 1),
                });
            }
            KeyCode::Up => self.recall(true),
            KeyCode::Down => self.recall(false),
            KeyCode::Left => self.caret = self.caret.saturating_sub(1),
            KeyCode::Right => self.caret = (self.caret + 1).min(length),
            KeyCode::Home => self.caret = 0,
            KeyCode::End => self.caret = length,
            KeyCode::Char('a') if ctrl => self.caret = 0,
            KeyCode::Char('e') if ctrl => self.caret = length,
            KeyCode::Char('u') if ctrl => {
                let at = self.byte(self.caret);
                self.value.replace_range(..at, "");
                self.caret = 0;
                self.changed();
            }
            KeyCode::Char('w') if ctrl => {
                let chars: Vec<char> = self.value.chars().collect();
                let mut start = self.caret;
                while start > 0 && chars[start - 1] == ' ' {
                    start -= 1;
                }
                while start > 0 && chars[start - 1] != ' ' {
                    start -= 1;
                }
                let (from, to) = (self.byte(start), self.byte(self.caret));
                self.value.replace_range(from..to, "");
                self.caret = start;
                self.changed();
            }
            KeyCode::Backspace if self.caret > 0 => {
                let (from, to) = (self.byte(self.caret - 1), self.byte(self.caret));
                self.value.replace_range(from..to, "");
                self.caret -= 1;
                self.changed();
            }
            KeyCode::Delete if self.caret < length => {
                let (from, to) = (self.byte(self.caret), self.byte(self.caret + 1));
                self.value.replace_range(from..to, "");
                self.changed();
            }
            KeyCode::Char(c) if !ctrl && !key.modifiers.alt => {
                let mut buffer = [0; 4];
                self.insert(c.encode_utf8(&mut buffer));
            }
            _ => {}
        }
        Flow::Continue
    }

    fn render(&self, context: &Context<'_>) -> View {
        let width = context.width;
        let theme = &self.theme;
        let mut line = question(theme, &self.prompt);
        if let Some(answer) = &self.answer {
            line.push(match answer {
                Some(answer) => text(self.shown_value(answer), &theme.answer),
                None => text("cancelled", &theme.hint),
            });
            return View::new(vec![fit(line, width)]);
        }
        let before: String = self.value.chars().take(self.caret).collect();
        let column = crate::components::width(&line) + cell_len(&self.shown_value(&before));
        if self.value.is_empty() {
            let hint = self
                .placeholder
                .clone()
                .or_else(|| self.default.clone().map(|default| format!("({default})")));
            if let Some(hint) = hint {
                line.push(text(hint, &theme.hint));
            }
        } else {
            line.push(plain(self.shown_value(&self.value)));
        }
        let mut lines = vec![fit(line, width)];
        if let Some(error) = &self.error {
            lines.push(fit(vec![text(format!("  ✗ {error}"), &theme.error)], width));
        } else if let Some(help) = &self.help {
            lines.push(fit(vec![text(format!("  {help}"), &theme.hint)], width));
        }
        for (index, (suggestion, positions)) in self.shown.iter().enumerate() {
            let selected = self.selected == Some(index);
            let mut row: Vec<Segment> = if selected {
                vec![text(format!("  {} ", theme.pointer), &theme.pointer_style)]
            } else {
                vec![plain("    ")]
            };
            let base = selected.then_some(&theme.focused);
            row.extend(highlight(
                &suggestion.value,
                positions,
                base,
                &theme.matched,
            ));
            if let Some(description) = &suggestion.description {
                row.push(text(format!("  {description}"), &theme.hint));
            }
            lines.push(fit(row, width));
        }
        if self.pending && self.shown.is_empty() && !self.value.is_empty() {
            lines.push(vec![text("    …", &theme.hint)]);
        }
        View::new(lines).with_cursor(0, column.min(width.saturating_sub(1)))
    }

    fn tick(&self) -> Option<Duration> {
        self.provider.as_ref().map(|_| Duration::from_millis(30))
    }

    fn default_value(&self) -> Option<String> {
        self.default.clone()
    }

    fn prompt(&mut self, io: &mut dyn LineIo) -> Result<Option<String>, NotInteractive> {
        let mut prompt = self.prompt.clone();
        if let Some(default) = &self.default {
            prompt.push_str(&format!(" [{default}]"));
        }
        io.write(&format!("{prompt}: "));
        let Some(mut line) = io.read_line() else {
            return Ok(None);
        };
        if line.is_empty() {
            if let Some(default) = &self.default {
                line = default.clone();
            }
        }
        if let Some(validator) = &self.validator {
            validator(&line).map_err(NotInteractive::Invalid)?;
        }
        Ok(Some(line))
    }
}
