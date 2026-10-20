//! Help for an [`input`](crate::input) as it is typed: completion (a
//! pop-up list of candidates and the top one's rest as ghost text), from
//! [`Node::complete`], and a history of what was entered (Up and Down),
//! from [`Node::history`], kept for the session or in the app's
//! [state](crate::state) between sessions. Generic: the candidates are
//! the app's (a command's arguments, field names, dates, paths).

use std::cell::RefCell;
use std::collections::HashMap;
use std::ops::Range;
use std::path::PathBuf;
use std::rc::Rc;

use crate::node::Node;
use crate::reactive::{signal, Runtime, Signal};
use crate::text_area::TextArea;

/// A candidate for completing an input: put `text` in place of the bytes
/// `replace` of what is typed (usually the word before the caret), with a
/// `detail` shown beside it in the pop-up (a type, a description).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Completion {
    pub replace: Range<usize>,
    pub text: String,
    pub detail: Option<String>,
}

impl Completion {
    /// `text` in place of bytes `replace`.
    pub fn new(replace: Range<usize>, text: impl Into<String>) -> Completion {
        Completion {
            replace,
            text: text.into(),
            detail: None,
        }
    }

    /// Show `detail` beside it in the pop-up, dimmed.
    #[must_use]
    pub fn detail(mut self, detail: impl Into<String>) -> Completion {
        self.detail = Some(detail.into());
        self
    }
}

/// The word before byte `caret` of `text`: from the last space before it.
pub fn word_before(text: &str, caret: usize) -> Range<usize> {
    let mut caret = caret.min(text.len());
    while !text.is_char_boundary(caret) {
        caret -= 1;
    }
    let start = text[..caret].rfind(char::is_whitespace).map_or(0, |at| {
        at + text[at..].chars().next().map_or(1, char::len_utf8)
    });
    start..caret
}

/// A completer from a fixed list: the words of `words` that start with
/// the word before the caret (ignoring case), in their order.
///
/// ```
/// use intuituive::complete::{complete_words, Completion};
///
/// let complete = complete_words(["project:", "priority:", "status:"]);
/// let found = complete("add pr", 6);
/// assert_eq!(found, vec![Completion::new(4..6, "project:"), Completion::new(4..6, "priority:")]);
/// ```
pub fn complete_words<S: Into<String>>(
    words: impl IntoIterator<Item = S>,
) -> impl Fn(&str, usize) -> Vec<Completion> + 'static {
    let words: Vec<String> = words.into_iter().map(Into::into).collect();
    move |text, caret| {
        let range = word_before(text, caret);
        let typed = text[range.clone()].to_lowercase();
        words
            .iter()
            .filter(|w| w.to_lowercase().starts_with(&typed) && w.len() > typed.len())
            .map(|w| Completion::new(range.clone(), w.clone()))
            .collect()
    }
}

/// What gives an input's candidates: the text and the caret's byte.
pub(crate) type Completer = Rc<dyn Fn(&str, usize) -> Vec<Completion>>;

/// How an input completes: whether the pop-up opens as you type (or only
/// on Tab, the default), whether the top candidate's rest shows as ghost
/// text (on by default), and how many rows the pop-up shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[must_use = "options do nothing until they are passed to what takes them"]
pub struct CompleteOptions {
    pub(crate) auto: bool,
    pub(crate) ghost: bool,
    pub(crate) rows: u16,
}

impl Default for CompleteOptions {
    fn default() -> CompleteOptions {
        CompleteOptions {
            auto: false,
            ghost: true,
            rows: 8,
        }
    }
}

impl CompleteOptions {
    /// The pop-up on Tab, ghost text, eight rows.
    pub fn new() -> CompleteOptions {
        CompleteOptions::default()
    }

    /// Open the pop-up as you type, whenever there are candidates.
    pub fn auto(mut self) -> CompleteOptions {
        self.auto = true;
        self
    }

    /// Show the top candidate's rest after the caret, dimmed (`on`), or
    /// not.
    pub fn ghost(mut self, on: bool) -> CompleteOptions {
        self.ghost = on;
        self
    }

    /// Show at most `rows` candidates at once (they scroll).
    pub fn rows(mut self, rows: u16) -> CompleteOptions {
        self.rows = rows.max(1);
        self
    }
}

/// The pop-up while it is open: the candidates, which is selected, and
/// the first shown.
#[derive(Clone, Debug)]
pub(crate) struct Popup {
    pub items: Vec<Completion>,
    pub selected: usize,
    pub top: usize,
}

impl Popup {
    /// Move the selection `by` (wrapping), keeping it in view of `rows`.
    pub fn step(&mut self, by: isize, rows: usize) {
        let len = self.items.len() as isize;
        if len == 0 {
            return;
        }
        self.selected = (self.selected as isize + by).rem_euclid(len) as usize;
        if self.selected < self.top {
            self.top = self.selected;
        } else if self.selected >= self.top + rows {
            self.top = self.selected + 1 - rows;
        }
    }
}

/// What gives an input's history now (a command line's changes by mode).
pub(crate) type HistoryFn = Rc<dyn Fn() -> Option<InputHistory>>;

/// An input's completion and history state.
#[derive(Default)]
pub(crate) struct Assist {
    pub completer: Option<Completer>,
    pub options: CompleteOptions,
    pub popup: Option<Popup>,
    /// The top candidate at a revision of the text, for the ghost.
    pub top: Option<(u64, Option<Completion>)>,
    /// A float is set for the pop-up.
    pub floated: bool,
    pub history: Option<HistoryFn>,
    /// Browsing the history: how far back, and what was typed before.
    pub browse: Option<(usize, String)>,
}

impl Assist {
    /// The candidates for `text` with the caret at byte `caret`, those
    /// that fit the text.
    pub fn candidates(&self, text: &str, caret: usize) -> Vec<Completion> {
        let Some(completer) = &self.completer else {
            return Vec::new();
        };
        completer(text, caret)
            .into_iter()
            .filter(|c| {
                c.replace.start <= c.replace.end
                    && c.replace.end <= text.len()
                    && text.is_char_boundary(c.replace.start)
                    && text.is_char_boundary(c.replace.end)
            })
            .collect()
    }

    pub fn history(&self) -> Option<InputHistory> {
        self.history.as_ref().and_then(|h| h())
    }
}

/// Entries entered in an input, oldest first: Up and Down go through
/// them in an input with [`Node::history`]. Kept for the session
/// ([`history_session`], or by name: `.history("find")`) or in a file
/// ([`history_in`] an app's state, [`history_file`]), read when made and
/// written on each entry, merged with what another session wrote. `Copy`.
#[derive(Clone, Copy)]
pub struct InputHistory {
    entries: Signal<Vec<String>>,
    file: Signal<Option<PathBuf>>,
    limit: Signal<usize>,
    status: Signal<Option<(usize, usize)>>,
}

/// A history kept for the session only.
pub fn history_session() -> InputHistory {
    history_at(None)
}

/// History `name` of app `app`, in its [state directory](crate::state_dir)
/// (for the session only where there is none).
pub fn history_in(app: &str, name: &str) -> InputHistory {
    history_at(crate::state::state_file(app, name))
}

/// A history kept in `file`, one entry a line.
pub fn history_file(file: impl Into<PathBuf>) -> InputHistory {
    history_at(Some(file.into()))
}

fn history_at(file: Option<PathBuf>) -> InputHistory {
    let entries = file
        .as_deref()
        .and_then(|f| std::fs::read_to_string(f).ok())
        .map(|text| parse(&text))
        .unwrap_or_default();
    InputHistory {
        entries: signal(entries),
        file: signal(file),
        limit: signal(500),
        status: signal(None),
    }
}

fn parse(text: &str) -> Vec<String> {
    text.lines()
        .filter(|l| !l.is_empty())
        .map(String::from)
        .collect()
}

thread_local! {
    /// The session histories made by name, by app.
    static NAMED: RefCell<HashMap<(u32, String), InputHistory>> = RefCell::new(HashMap::new());
}

impl From<&str> for InputHistory {
    /// The session history named `name`: every input given the same name
    /// in the app shares it.
    fn from(name: &str) -> InputHistory {
        let app = Runtime::current().id;
        NAMED.with(|named| {
            *named
                .borrow_mut()
                .entry((app, name.to_string()))
                .or_insert_with(history_session)
        })
    }
}

impl InputHistory {
    /// Keep at most `entries` (500 by default); the oldest go.
    // A handle: the change is made through shared state, so the
    // returned copy is only for chaining and may be dropped.
    #[allow(clippy::return_self_not_must_use)]
    pub fn limit(self, entries: usize) -> InputHistory {
        self.limit.set(entries.max(1));
        self
    }

    /// The entries, oldest first, subscribing.
    pub fn entries(&self) -> Vec<String> {
        self.entries.get()
    }

    /// Add `entry` as the newest (moving it there if it was older), and
    /// write the file. Empty entries and line breaks are left out.
    pub fn push(&self, entry: &str) {
        if entry.trim().is_empty() || entry.contains(['\n', '\r']) {
            return;
        }
        let limit = self.limit.get_untracked();
        let add = |entries: &mut Vec<String>| {
            entries.retain(|e| e != entry);
            entries.push(entry.to_string());
            let over = entries.len().saturating_sub(limit);
            entries.drain(..over);
        };
        if let Some(file) = self.file.get_untracked() {
            let mut merged = None;
            let _ = crate::state::update_file(&file, |old| {
                let mut entries = old
                    .map(|b| parse(&String::from_utf8_lossy(&b)))
                    .unwrap_or_default();
                add(&mut entries);
                let body: String = entries.iter().map(|e| format!("{e}\n")).collect();
                merged = Some(entries);
                body.into_bytes()
            });
            if let Some(entries) = merged {
                self.entries.set(entries);
                return;
            }
        }
        self.entries.update(add);
    }

    /// Where an input browsing it is: the entry shown (1 the newest) and
    /// how many match, as taskwarrior-tui's `[3 / 7]`; `None` when none
    /// is browsing. Subscribing.
    pub fn status(&self) -> Option<(usize, usize)> {
        self.status.get()
    }

    /// The entries starting with `prefix`, newest first.
    pub(crate) fn matching(&self, prefix: &str) -> Vec<String> {
        self.entries.with_untracked(|entries| {
            entries
                .iter()
                .rev()
                .filter(|e| e.starts_with(prefix))
                .cloned()
                .collect()
        })
    }

    pub(crate) fn set_status(&self, status: Option<(usize, usize)>) {
        if self.status.get_untracked() != status {
            self.status.set(status);
        }
    }
}

/// An input's completion and history. On any other node they do nothing.
impl Node {
    /// Complete an [`input`](crate::input) with the candidates `complete`
    /// gives for its text and the caret's byte: Tab opens a pop-up of them
    /// under the input (one candidate is taken at once), Tab and ↓ or
    /// Shift+Tab and ↑ move through it, Enter takes the selected one, Esc
    /// closes it, and typing narrows it. The top candidate's rest shows
    /// after the caret, dimmed, and → or End at the end takes it.
    ///
    /// ```
    /// use intuituive::prelude::*;
    /// use intuituive::complete::complete_words;
    ///
    /// let app = App::new(|| {
    ///     let typed = signal(String::new());
    ///     input(typed).complete(complete_words(["status:", "start", "project:"]))
    /// });
    /// let mut driver = app.driver(30, 5);
    /// driver.type_text("st");
    /// // The ghost: the top candidate's rest.
    /// assert_eq!(driver.screen().plain()[0].trim_end(), "status:");
    /// driver.press("tab");
    /// assert_eq!(driver.screen().plain()[1].trim_end(), " status:");
    /// assert_eq!(driver.screen().plain()[2].trim_end(), " start");
    /// driver.press("down enter");
    /// assert_eq!(driver.screen().plain()[0].trim_end(), "start");
    /// assert_eq!(driver.screen().plain()[1].trim_end(), "");
    /// ```
    pub fn complete(self, complete: impl Fn(&str, usize) -> Vec<Completion> + 'static) -> Node {
        self.complete_with(complete, CompleteOptions::default())
    }

    /// [`complete`](Self::complete) as `options` say: the pop-up as you
    /// type, no ghost, how many rows.
    pub fn complete_with(
        self,
        complete: impl Fn(&str, usize) -> Vec<Completion> + 'static,
        options: CompleteOptions,
    ) -> Node {
        self.completer(Some(Rc::new(complete)), options)
    }

    pub(crate) fn completer(self, completer: Option<Completer>, options: CompleteOptions) -> Node {
        self.widget_mut(|area: &mut TextArea| {
            area.assist.completer = completer;
            area.assist.options = options;
        });
        self
    }

    /// Go through `history` with ↑ (older) and ↓ (newer) in an
    /// [`input`](crate::input): the entries that start with what was typed
    /// before the first ↑, as taskwarrior-tui's; ↓ past the newest puts
    /// back what was typed. Enter adds the text as the newest entry. A name
    /// (`"find"`) is a history for the session, shared by name;
    /// [`history_in`] keeps one in the app's state between sessions.
    ///
    /// ```
    /// use intuituive::prelude::*;
    /// use intuituive::complete::history_session;
    ///
    /// let app = App::new(|| {
    ///     let typed = signal(String::new());
    ///     let history = history_session();
    ///     history.push("project:home");
    ///     history.push("+next");
    ///     input(typed).history(history).on_submit(move |_, _| typed.set(String::new()))
    /// });
    /// let mut driver = app.driver(20, 1);
    /// driver.press("up");
    /// assert_eq!(driver.screen().plain()[0].trim_end(), "+next");
    /// driver.press("up");
    /// assert_eq!(driver.screen().plain()[0].trim_end(), "project:home");
    /// driver.press("down down");
    /// assert_eq!(driver.screen().plain()[0].trim_end(), "");
    /// // Only the entries that start with what is typed.
    /// driver.type_text("pro");
    /// driver.press("up");
    /// assert_eq!(driver.screen().plain()[0].trim_end(), "project:home");
    /// ```
    pub fn history(self, history: impl Into<InputHistory>) -> Node {
        let history = history.into();
        self.history_by(Some(Rc::new(move || Some(history))))
    }

    pub(crate) fn history_by(self, history: Option<HistoryFn>) -> Node {
        self.widget_mut(|area: &mut TextArea| area.assist.history = history);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_word_before_the_caret() {
        assert_eq!(word_before("add pro", 7), 4..7);
        assert_eq!(word_before("add pro", 5), 4..5);
        assert_eq!(word_before("pro", 3), 0..3);
        assert_eq!(word_before("a ", 2), 2..2);
        assert_eq!(word_before("é ü", 5), 3..5);
    }

    #[test]
    fn a_popup_wraps_and_scrolls() {
        let items = (0..5)
            .map(|i| Completion::new(0..0, i.to_string()))
            .collect();
        let mut popup = Popup {
            items,
            selected: 0,
            top: 0,
        };
        popup.step(-1, 3);
        assert_eq!((popup.selected, popup.top), (4, 2));
        popup.step(1, 3);
        assert_eq!((popup.selected, popup.top), (0, 0));
        popup.step(3, 3);
        assert_eq!((popup.selected, popup.top), (3, 1));
    }

    #[test]
    fn a_history_file_keeps_entries_once_and_newest_last() {
        let file = std::env::temp_dir().join(format!("intuituive-history-{}", std::process::id()));
        let _ = std::fs::remove_file(&file);
        let runtime = crate::reactive::Runtime::new();
        runtime.enter(|| {
            let history = history_file(&file).limit(3);
            for entry in ["a", "b", "a", "c", "d", "", "x\ny"] {
                history.push(entry);
            }
            assert_eq!(history.entries(), vec!["a", "c", "d"]);
            assert_eq!(history.matching(""), vec!["d", "c", "a"]);
            // The next session reads it.
            assert_eq!(history_file(&file).entries(), vec!["a", "c", "d"]);
        });
        let _ = std::fs::remove_file(&file);
    }
}
