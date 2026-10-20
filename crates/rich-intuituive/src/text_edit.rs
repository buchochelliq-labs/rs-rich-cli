//! The text editors' editing options: rebindable keys and presets, a
//! modal preset's hook, search with a find bar, word wrap, the cursor line,
//! the caret's shape, tabs, the undo history and an input's validation
//! message. Builders on [`Node`] for a [`text_area`](crate::text_area) or
//! an [`input`](crate::input); on any other node they do nothing.
//!
//! The editing itself is rs-rich-interact's: [`Buffer`] for the text and
//! [`edit_keys`] for what the keys do, so `rich_interact`'s own editors
//! share it.

use std::cell::RefCell;
use std::fmt;
use std::rc::Rc;

use rich_interact::caret::CaretShape;
use rich_interact::edit::{Search, WrapMode};
use rich_interact::edit_keys::{self, KeyMode};
use rich_interact::keymap::{keys, Keymap};

use crate::node::Node;
use crate::reactive::{signal, Signal};
use crate::text_area::TextArea;

/// What validates an input: its text in, a message out when it is wrong.
pub(crate) type Validator = Rc<dyn Fn(&str) -> Option<String>>;

/// The editing options of a text area or input, beside its others.
#[derive(Clone)]
pub(crate) struct Editing {
    /// What the keys do: [`edit_keys::editor_keymap`] unless set.
    pub keymap: Keymap,
    /// A modal preset, which sees each key first.
    pub mode: Option<Rc<RefCell<dyn KeyMode>>>,
    /// Where the mode's name and pending keys are written.
    pub mode_status: Option<Signal<String>>,
    pub search: Option<TextSearch>,
    /// The search as last applied: pattern, regex, any case, and the
    /// requests seen.
    pub applied: Option<(String, bool, bool, u64)>,
    pub validate: Option<Validator>,
    pub cursor_line: bool,
    pub caret_shape: Option<CaretShape>,
    pub draw_caret: bool,
}

impl Default for Editing {
    fn default() -> Editing {
        Editing {
            keymap: edit_keys::editor_keymap(),
            mode: None,
            mode_status: None,
            search: None,
            applied: None,
            validate: None,
            cursor_line: false,
            caret_shape: None,
            draw_caret: false,
        }
    }
}

impl fmt::Debug for Editing {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Editing")
            .field("keymap", &self.keymap.len())
            .field("mode", &self.mode.is_some())
            .field("search", &self.search.is_some())
            .field("validate", &self.validate.is_some())
            .field("cursor_line", &self.cursor_line)
            .field("caret_shape", &self.caret_shape)
            .field("draw_caret", &self.draw_caret)
            .finish()
    }
}

/// A search in a text area that the app drives: the pattern (a signal, so
/// an [`input`](crate::input) can edit it), plain or a regular expression,
/// any case or not, and requests to go to the next or previous match. Give
/// it to [`Node::search`]; [`search_bar`] is a find bar over it. It is
/// `Copy`; make it inside the app.
///
/// Every match in view is highlighted (`text_area.search`), the one
/// selected in `text_area.search.current`; a new pattern selects the first
/// match from the caret, as an editor's incremental search does. F3 and
/// Shift+F3 in the area go to the next and previous match.
///
/// ```
/// use intuituive::prelude::*;
/// use intuituive::TextSearch;
///
/// let app = App::new(|| {
///     let body = signal("one fish\ntwo fish".to_string());
///     let search = TextSearch::new();
///     search.set("fish");
///     column([
///         text_area(body).search(search).autofocus(),
///         text(move || format!("{:?}", search.found())).fixed(1),
///     ])
///     .on_key("ctrl+n", move |_| search.next())
/// });
/// let mut driver = app.driver(20, 3);
/// driver.settle();
/// assert_eq!(driver.screen().plain()[2].trim_end(), "Some((1, 2))");
/// driver.press("ctrl+n");
/// driver.settle();
/// assert_eq!(driver.screen().plain()[2].trim_end(), "Some((2, 2))");
/// ```
#[derive(Clone, Copy)]
pub struct TextSearch {
    pattern: Signal<String>,
    regex: Signal<bool>,
    ignore_case: Signal<bool>,
    /// Requests to move: how many, and the last one's way.
    moves: Signal<(u64, bool)>,
    /// The match selected (from 1, 0 for none) and how many there are.
    found: Signal<Option<(usize, usize)>>,
    error: Signal<Option<String>>,
}

impl Default for TextSearch {
    fn default() -> TextSearch {
        TextSearch::new()
    }
}

impl TextSearch {
    /// An empty, plain, case-sensitive search.
    pub fn new() -> TextSearch {
        TextSearch {
            pattern: signal(String::new()),
            regex: signal(false),
            ignore_case: signal(false),
            moves: signal((0, true)),
            found: signal(None),
            error: signal(None),
        }
    }

    /// The pattern, to bind an input to.
    pub fn pattern(&self) -> Signal<String> {
        self.pattern
    }

    /// Search for `pattern`.
    pub fn set(&self, pattern: &str) {
        self.pattern.set(pattern.to_string());
    }

    /// Take the pattern as a regular expression (fancy-regex's syntax).
    pub fn set_regex(&self, on: bool) {
        self.regex.set(on);
    }

    pub fn is_regex(&self) -> bool {
        self.regex.get()
    }

    /// Match letters of either case.
    pub fn set_ignore_case(&self, on: bool) {
        self.ignore_case.set(on);
    }

    pub fn is_ignore_case(&self) -> bool {
        self.ignore_case.get()
    }

    /// Select the next match, round past the end.
    pub fn next(&self) {
        self.moves
            .update(|(n, forward)| (*n, *forward) = (*n + 1, true));
    }

    /// Select the match before, round past the start.
    pub fn previous(&self) {
        self.moves
            .update(|(n, forward)| (*n, *forward) = (*n + 1, false));
    }

    /// Which match is selected (from 1; 0 when the selection is not one)
    /// and how many there are, once the area has drawn; `None` with no
    /// pattern. Read in a node, it updates.
    pub fn found(&self) -> Option<(usize, usize)> {
        self.found.get()
    }

    /// Why the pattern does not compile as a regular expression.
    pub fn error(&self) -> Option<String> {
        self.error.get()
    }

    /// The search to set on the buffer, and the key it is applied by.
    pub(crate) fn wanted(&self) -> (String, bool, bool, u64, bool) {
        let (count, forward) = self.moves.get();
        (
            self.pattern.get(),
            self.regex.get(),
            self.ignore_case.get(),
            count,
            forward,
        )
    }

    pub(crate) fn report(&self, found: Option<(usize, usize)>, error: Option<String>) {
        if self.found.with_untracked(|f| *f != found) {
            self.found.set(found);
        }
        if self.error.with_untracked(|e| *e != error) {
            self.error.set(error);
        }
    }
}

/// A find bar for `search`: `Find` and an input bound to its pattern, the
/// match it is on and how many there are (or why a regular expression does
/// not compile), and the flags. In it, Enter and ↓ go to the next match, ↑
/// to the one before, Alt+R takes the pattern as a regular expression and
/// Alt+C matches either case. Its styles are `search_bar.label` (bold),
/// `search_bar.status` (dim) and `search_bar.error` (red).
///
/// ```
/// use intuituive::prelude::*;
/// use intuituive::{search_bar, TextSearch};
///
/// let app = App::new(|| {
///     let text = signal("cat hat bat".to_string());
///     let search = TextSearch::new();
///     column([text_area(text).search(search), search_bar(search).fixed(1).autofocus()])
/// });
/// let mut driver = app.driver(30, 2);
/// driver.type_text("at");
/// driver.settle();
/// assert_eq!(driver.screen().plain()[1].trim_end(), "Find at         1/3");
/// driver.press("enter");
/// driver.settle();
/// assert_eq!(driver.screen().plain()[1].trim_end(), "Find at         2/3");
/// ```
pub fn search_bar(search: TextSearch) -> Node {
    use crate::node::{label, row, text};
    let status = text(move || {
        let flags = format!(
            "{}{}",
            if search.is_regex() { " .*" } else { "" },
            if search.is_ignore_case() { " Aa" } else { "" }
        );
        if let Some(error) = search.error() {
            let error = rich::markup::escape(&error);
            return format!("[search_bar.error]{error}[/]{flags}");
        }
        let count = match search.found() {
            None => String::new(),
            Some((_, 0)) => "no matches".to_string(),
            Some((0, of)) => format!("{of}"),
            Some((at, of)) => format!("{at}/{of}"),
        };
        format!("[search_bar.status]{count}{flags}[/]")
    });
    row([
        label("[search_bar.label]Find[/] ").fixed(5),
        crate::input::input(search.pattern()).on_submit(move |_, _| search.next()),
        status.fixed(14),
    ])
    .on_key("down", move |_| search.next())
    .on_key("up", move |_| search.previous())
    .on_key("alt+r", move |_| search.set_regex(!search.is_regex()))
    .on_key("alt+c", move |_| {
        search.set_ignore_case(!search.is_ignore_case())
    })
    .theme_styles([
        ("search_bar.label", "bold"),
        ("search_bar.status", "dim"),
        ("search_bar.error", "red"),
    ])
}

/// Apply `search` to the area's buffer as it wants it now: the pattern
/// rebuilt when it changed (selecting the first match from the caret), a
/// requested move made. Whether the caret moved.
pub(crate) fn apply_search(area: &mut TextArea) -> bool {
    let Some(search) = area.options.editing.search else {
        return false;
    };
    let (pattern, regex, any_case, moves, forward) = search.wanted();
    let key = (pattern.clone(), regex, any_case, moves);
    let before = area.options.editing.applied.clone();
    if before.as_ref() == Some(&key) {
        // Nothing new from the app; the area's own keys may have moved.
        let buffer = &area.buffer_ref();
        let error = search.error.get_untracked();
        search.report(found_in(buffer, &pattern, error.is_some()), error);
        return false;
    }
    let buffer = area.buffer_mut();
    let changed = before
        .as_ref()
        .is_none_or(|(p, r, i, _)| (p, *r, *i) != (&pattern, regex, any_case));
    let mut error = None;
    let mut moved = false;
    if changed {
        match Search::new(&pattern, regex) {
            Ok(found) => buffer.set_search(Some(found.ignore_case(any_case))),
            Err(why) => {
                buffer.set_search(None);
                error = Some(why);
            }
        }
        if !pattern.is_empty() && error.is_none() {
            // Typing on: the match that starts where the last one did.
            let from = buffer
                .selection()
                .map_or(buffer.caret(), |(start, _)| start);
            buffer.set_caret(from, false);
            moved = buffer.select_match(true);
        }
    } else if before.is_some_and(|(_, _, _, seen)| seen != moves) && !pattern.is_empty() {
        moved = buffer.select_match(forward);
    }
    area.options.editing.applied = Some(key);
    let found = found_in(area.buffer_ref(), &pattern, error.is_some());
    search.report(found, error);
    moved
}

/// Which match is selected (from 1, 0 for none) and how many there are.
fn found_in(
    buffer: &rich_interact::edit::Buffer,
    pattern: &str,
    failed: bool,
) -> Option<(usize, usize)> {
    (!pattern.is_empty() && !failed).then(|| {
        let count = buffer.match_count();
        let at = buffer
            .selection()
            .and_then(|_| buffer.match_index())
            .map_or(0, |i| i + 1);
        (at, count)
    })
}

/// The editing options of a [`text_area`](crate::text_area) or an
/// [`input`](crate::input). On any other node they do nothing.
impl Node {
    /// Use `keymap` for the editing keys (its actions are
    /// [`edit_keys`]'s, in context `text_area`):
    /// [`emacs_keys`](Self::emacs_keys) is one, and a configuration file's
    /// [installed](rich_interact::keymap::install) overrides apply to it.
    pub fn edit_keymap(self, keymap: Keymap) -> Node {
        self.editing(|e| e.keymap = keymap)
    }

    /// Emacs's keys ([`edit_keys::emacs_keymap`]): Ctrl+F/B/N/P, Alt+F/B,
    /// Ctrl+A/E, Ctrl+K, Ctrl+U, Ctrl+W and Alt+D killing onto a kill ring
    /// that Ctrl+Y yanks back and Alt+Y cycles, Ctrl+Space setting the
    /// mark. The arrows and the rest still work.
    ///
    /// ```
    /// use intuituive::prelude::*;
    ///
    /// let app = App::new(|| input(signal("hello world".to_string())).emacs_keys());
    /// let mut driver = app.driver(20, 1);
    /// for key in ["ctrl+a", "alt+d", "ctrl+e", "ctrl+y"] {
    ///     driver.press(key);
    /// }
    /// assert_eq!(driver.screen().plain()[0].trim_end(), " worldhello");
    /// ```
    pub fn emacs_keys(self) -> Node {
        self.edit_keymap(edit_keys::emacs_keymap())
    }

    /// Make the keys `keys` (names, as [`on_key`](Self::on_key) takes
    /// them) do the editing `action` (one of [`edit_keys`]'s: `undo`,
    /// `yank`, `word-left`), on this editor only; `""` unbinds it. A key
    /// moved onto an action leaves the one that had it.
    ///
    /// ```
    /// use intuituive::prelude::*;
    ///
    /// let app = App::new(|| {
    ///     input(signal("one two".to_string()))
    ///         .rebind_key("delete-word", "alt+backspace")
    ///         .rebind_key("yank", "ctrl+y")
    /// });
    /// let mut driver = app.driver(20, 1);
    /// driver.press("alt+backspace");
    /// assert_eq!(driver.screen().plain()[0].trim_end(), "one");
    /// ```
    ///
    /// # Panics
    /// On a key name that does not parse, as `on_key` does.
    pub fn rebind_key(self, action: &str, keys_named: &str) -> Node {
        let keys = keys(keys_named);
        self.editing(|e| e.keymap.rebind(action, keys))
    }

    /// Hand each key to `mode` first: a modal preset such as Vim's
    /// ([`vim`](Self::vim), behind the `vim` feature). A key it passes is
    /// edited as without it.
    pub fn key_mode(self, mode: impl KeyMode + 'static) -> Node {
        let mode: Rc<RefCell<dyn KeyMode>> = Rc::new(RefCell::new(mode));
        self.editing(|e| e.mode = Some(mode))
    }

    /// Write the [key mode](Self::key_mode)'s name and the keys of a
    /// command not yet complete (`NORMAL 2d`) to `status`, for a status
    /// bar.
    pub fn mode_status(self, status: Signal<String>) -> Node {
        self.widget_mut(|area: &mut TextArea| {
            area.options.editing.mode_status = Some(status);
            area.tell_mode();
        });
        self
    }

    /// Search with `search`, which the app (or a [`search_bar`]) drives.
    pub fn search(self, search: TextSearch) -> Node {
        self.editing(|e| {
            e.search = Some(search);
            e.applied = None;
        })
    }

    /// How long lines are shown: wrapped at a character
    /// ([`WrapMode::Grapheme`], the default), at a space so words stay
    /// whole ([`WrapMode::Word`]), or not at all, scrolled sideways
    /// ([`WrapMode::None`], as [`soft_wrap(false)`](Self::soft_wrap)).
    ///
    /// ```
    /// use intuituive::prelude::*;
    /// use intuituive::interact::edit::WrapMode;
    ///
    /// let app = App::new(|| text_area(signal("the quick brown fox".into())).wrap_mode(WrapMode::Word));
    /// let mut driver = app.driver(11, 3);
    /// driver.settle();
    /// assert_eq!(driver.screen().plain(), ["the quick  ", "brown fox  ", "           "]);
    /// ```
    pub fn wrap_mode(self, mode: WrapMode) -> Node {
        self.widget_mut(|area: &mut TextArea| area.set_wrap(mode));
        self
    }

    /// Highlight the caret's line (all its rows) in the theme's
    /// `text_area.cursor_line` style, underlined unless the theme names it.
    pub fn cursor_line(self) -> Node {
        self.editing(|e| e.cursor_line = true)
    }

    /// The caret's shape while the editor has the focus: the terminal's
    /// cursor is set to it (DECSCUSR), and put back as the user had it
    /// when the focus leaves or the app ends. A [key mode](Self::key_mode)
    /// sets its own.
    pub fn caret_shape(self, shape: CaretShape) -> Node {
        self.editing(|e| e.caret_shape = Some(shape))
    }

    /// Draw the caret in the cells rather than with the terminal's cursor,
    /// in the theme's `text_area.caret` style (reversed for a block or a
    /// bar, underlined for an underline): it shows in a screenshot, on a
    /// web page drawn from the cells, and where the terminal's cursor is
    /// hidden.
    pub fn draw_caret(self) -> Node {
        self.editing(|e| e.draw_caret = true)
    }

    /// The cells a tab takes (4 by default), and the spaces Tab indents
    /// with.
    pub fn tab_width(self, cells: usize) -> Node {
        self.widget_mut(|area: &mut TextArea| area.buffer_mut().set_tab_width(cells));
        self
    }

    /// Indent with a tab character rather than spaces.
    pub fn hard_tabs(self) -> Node {
        self.widget_mut(|area: &mut TextArea| area.buffer_mut().set_hard_tabs(true));
        self
    }

    /// Keep at most `steps` undo steps (1000 by default); `None` keeps
    /// them all.
    pub fn undo_limit(self, steps: Option<usize>) -> Node {
        self.widget_mut(|area: &mut TextArea| area.buffer_mut().set_history_limit(steps));
        self
    }

    /// Validate an [`input`](crate::input): `check` is given the text and
    /// returns why it is wrong, or `None`. While it returns a message the
    /// node is [in error](Self::error_when) (styled `:error`, read out as
    /// invalid), the message shows under the text when the input has two
    /// rows (`.fixed(2)`), else beside it, right aligned, in the theme's
    /// `input.error` style, and assistive technology is given it.
    ///
    /// ```
    /// use intuituive::prelude::*;
    ///
    /// let app = App::new(|| {
    ///     let age = signal("abc".to_string());
    ///     input(age).label("Age").validate(|text| {
    ///         text.parse::<u8>().is_err().then(|| "a whole number, 0 to 255".to_string())
    ///     })
    /// });
    /// let mut driver = app.driver(30, 2);
    /// driver.settle();
    /// assert_eq!(driver.screen().plain()[1].trim_end(), "a whole number, 0 to 255");
    /// let node = &driver.accessibility()[0];
    /// assert_eq!(node.describe(), "Age, text box: abc, invalid: a whole number, 0 to 255");
    /// ```
    pub fn validate(self, check: impl Fn(&str) -> Option<String> + 'static) -> Node {
        let check: Validator = Rc::new(check);
        let text = self.widget_mut(|area: &mut TextArea| {
            area.options.editing.validate = Some(check.clone());
            area.text_signal()
        });
        match text {
            Some(text) => self.error_message(move || text.with(|value| check(value))),
            None => self,
        }
    }

    fn editing(self, f: impl FnOnce(&mut Editing)) -> Node {
        self.widget_mut(|area: &mut TextArea| {
            f(&mut area.options.editing);
            area.touch();
        });
        self
    }
}
