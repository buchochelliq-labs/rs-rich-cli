//! Text input with validation, history and suggestions (#457, #289).
//!
//! The line edits like a shell's: the arrows, Home/End, Ctrl+A/E, Ctrl+U,
//! Ctrl+W, Backspace and Delete. A validator's message shows under the line
//! and Enter waits until it passes. Up and Down walk the history, or the
//! suggestions when some are showing; Tab accepts one. Suggestions come from
//! a fixed list, filtered as you type, or from a provider called on a
//! background thread, so a slow lookup never stalls typing. The input has
//! one such thread; it runs one lookup at a time and, when it is free,
//! takes only the latest text, so typing never piles up lookups.

use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;
use std::time::Duration;

use rich::cells::{cell_len, split_graphemes};
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
    /// The caret, in characters; it moves by grapheme cluster.
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
    /// (generation, text) to the provider's thread, started on first use.
    requests: Option<Sender<(u64, String)>>,
    /// (generation, results) back from it.
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
            requests: None,
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

    /// A masked input, for passwords and tokens: what is typed shows as
    /// `•`, and a default only as `(default set)`.
    pub fn masked(prompt: impl Into<String>) -> Input {
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

    /// The character indices where graphemes start, and the end: where
    /// the caret may stop, so an accent is never split from its letter.
    fn stops(&self) -> Vec<usize> {
        let (spans, _) = split_graphemes(&self.value);
        let mut stops = Vec::with_capacity(spans.len() + 1);
        let (mut chars, mut byte) = (0, 0);
        for (start, _, _) in spans {
            chars += self.value[byte..start].chars().count();
            byte = start;
            stops.push(chars);
        }
        stops.push(self.value.chars().count());
        stops
    }

    /// The caret one grapheme back.
    fn previous_stop(&self) -> usize {
        let stops = self.stops();
        stops
            .iter()
            .rev()
            .copied()
            .find(|&stop| stop < self.caret)
            .unwrap_or(0)
    }

    /// The caret one grapheme on.
    fn next_stop(&self) -> usize {
        let stops = self.stops();
        stops
            .iter()
            .copied()
            .find(|&stop| stop > self.caret)
            .unwrap_or(self.caret)
    }

    /// The value as shown (masked or not), cut to `available` cells so the
    /// caret stays in view: a line longer than the space scrolls, and the
    /// caret's column in what is returned.
    fn window(&self, available: usize) -> (String, usize) {
        let shown = self.shown_value(&self.value);
        let before = cell_len(&self.shown_value(&self.value[..self.byte(self.caret)]));
        // One cell for the caret after the last character.
        let skip = (before + 1).saturating_sub(available.max(1));
        if skip == 0 {
            return (shown, before);
        }
        let (spans, _) = split_graphemes(&shown);
        let mut dropped = 0;
        let mut from = shown.len();
        for (start, _, cells) in spans {
            if dropped >= skip {
                from = start;
                break;
            }
            dropped += cells;
        }
        (shown[from..].to_string(), before - dropped)
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
            let requests = self.requests.get_or_insert_with(|| {
                let (requests, queue) = channel::<(u64, String)>();
                let (provider, sender) = (Arc::clone(provider), self.sender.clone());
                // Ends when the input, and with it `requests`, is dropped.
                std::thread::spawn(move || {
                    while let Ok(mut request) = queue.recv() {
                        // Skip to the latest text: older lookups are stale.
                        while let Ok(newer) = queue.try_recv() {
                            request = newer;
                        }
                        let (generation, query) = request;
                        if sender.send((generation, provider(&query))).is_err() {
                            return;
                        }
                    }
                });
                requests
            });
            let _ = requests.send((self.generation, self.value.clone()));
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

    /// The answer Enter would give: the text, or the default when empty,
    /// checked by the validator.
    pub(crate) fn resolve(&self) -> Result<String, String> {
        let mut answer = self.value.clone();
        if answer.is_empty() {
            if let Some(default) = &self.default {
                answer = default.clone();
            }
        }
        if let Some(validator) = &self.validator {
            validator(&answer)?;
        }
        Ok(answer)
    }

    /// The line's text as shown (masked, or the placeholder) in
    /// `available` cells, scrolled to keep the caret in view, and the
    /// caret's column in it.
    pub(crate) fn field(&self, available: usize) -> (Vec<Segment>, usize) {
        if self.value.is_empty() {
            let hint = self.placeholder.clone().or_else(|| self.default_hint());
            return (
                hint.map(|hint| vec![text(hint, &self.theme.hint)])
                    .unwrap_or_default(),
                0,
            );
        }
        let (shown, column) = self.window(available);
        (vec![plain(shown)], column)
    }

    /// How the default is shown when nothing is typed: `(value)`, or for
    /// a masked input only that there is one, so a secret default is never
    /// shown or written in clear.
    fn default_hint(&self) -> Option<String> {
        let default = self.default.as_ref()?;
        Some(match self.mask {
            Some(_) => "(default set)".to_string(),
            None => format!("({default})"),
        })
    }

    pub(crate) fn label(&self) -> &str {
        &self.prompt
    }

    /// Whether suggestions are showing: Tab, Up, Down and Enter (with one
    /// selected) are theirs.
    pub(crate) fn suggesting(&self) -> bool {
        !self.shown.is_empty()
    }

    /// Whether Enter would accept a selected suggestion rather than submit.
    pub(crate) fn has_selection(&self) -> bool {
        self.selected.is_some()
    }

    /// Accept the selected suggestion, as Enter does before submitting.
    pub(crate) fn accept_selected(&mut self) {
        if let Some(index) = self.selected {
            self.accept(index);
        }
    }

    /// The suggestion rows (or a pending marker), each after `indent`.
    pub(crate) fn suggestion_rows(&self, width: usize, indent: &str) -> Vec<Vec<Segment>> {
        let theme = &self.theme;
        let mut lines = Vec::new();
        for (index, (suggestion, positions)) in self.shown.iter().enumerate() {
            let selected = self.selected == Some(index);
            let mut row: Vec<Segment> = vec![plain(indent.to_string())];
            row.push(if selected {
                text(format!("  {} ", theme.pointer), &theme.pointer_style)
            } else {
                plain("    ")
            });
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
            lines.push(vec![text(format!("{indent}    …"), &theme.hint)]);
        }
        lines
    }

    pub(crate) fn display_value(&self, value: &str) -> String {
        self.shown_value(value)
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
            self.insert(&crate::components::pasted(text, " "));
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
            KeyCode::Left => self.caret = self.previous_stop(),
            KeyCode::Right => self.caret = self.next_stop().min(length),
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
                let previous = self.previous_stop();
                let (from, to) = (self.byte(previous), self.byte(self.caret));
                self.value.replace_range(from..to, "");
                self.caret = previous;
                self.changed();
            }
            KeyCode::Delete if self.caret < length => {
                let (from, to) = (self.byte(self.caret), self.byte(self.next_stop()));
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
        let start = crate::components::width(&line);
        let (field, caret) = self.field(width.saturating_sub(start));
        line.extend(field);
        let column = start + caret;
        let mut lines = vec![fit(line, width)];
        if let Some(error) = &self.error {
            lines.push(fit(vec![text(format!("  ✗ {error}"), &theme.error)], width));
        } else if let Some(help) = &self.help {
            lines.push(fit(vec![text(format!("  {help}"), &theme.hint)], width));
        }
        lines.extend(self.suggestion_rows(width, ""));
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
        if let Some(hint) = self.default_hint() {
            prompt.push_str(&format!(" [{}]", &hint[1..hint.len() - 1]));
        }
        io.write(&format!("{prompt}: "));
        // A masked answer is read without echo where the terminal has it.
        let read = if self.mask.is_some() {
            io.read_secret()
        } else {
            io.read_line().map(Some).ok_or(NotInteractive::Ended)
        };
        let mut line = match read {
            Ok(Some(line)) => line,
            Ok(None) => return Ok(None),
            // Input ended: the default is the answer, as for an empty line.
            Err(NotInteractive::Ended) => match &self.default {
                Some(default) => default.clone(),
                None => return Err(NotInteractive::Ended),
            },
            Err(error) => return Err(error),
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
