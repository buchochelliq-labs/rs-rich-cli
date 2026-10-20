//! Editor keys: what a text editor's keys do, as actions in a
//! [`Keymap`], so they can be rebound, listed and swapped for a preset.
//!
//! [`editor_keymap`] is the default: the arrows and Home/End (Shift
//! selecting, Ctrl by word or to the ends), Ctrl+A to select all, Ctrl+C
//! and Ctrl+X to copy and cut, Ctrl+Z and Ctrl+Y to undo and redo, Ctrl+W,
//! Ctrl+U and Ctrl+K to delete. [`emacs_keymap`] is Emacs's and readline's:
//! Ctrl+F/B/N/P, Alt+F/B, Ctrl+A/E, and Ctrl+K, Ctrl+U, Ctrl+W and Alt+D
//! killing onto a kill ring that Ctrl+Y yanks back and Alt+Y cycles. Both
//! name the same actions (in context [`CONTEXT`]), so either can be
//! rebound one action at a time.
//!
//! A widget asks [`resolve`] what a key does, then [`perform`]s it on its
//! [`Buffer`]; only copying and cutting, which need the app's clipboard,
//! are its own. A modal preset (Vim's) is a [`KeyMode`]: it sees each key
//! first and edits the buffer itself.
//!
//! ```
//! use rich_interact::edit::Buffer;
//! use rich_interact::edit_keys::{emacs_keymap, perform, resolve, Act, Outcome};
//! use rich_interact::Key;
//!
//! let keymap = emacs_keymap();
//! let mut buffer = Buffer::new("hello world");
//! for key in ["alt+f", "ctrl+k", "ctrl+a", "ctrl+y"] {
//!     let (action, select) = resolve(&keymap, Key::parse(key).unwrap()).unwrap();
//!     let act = Act { select, ..Act::default() };
//!     assert_eq!(perform(&mut buffer, &action, act), Outcome::Used);
//! }
//! assert_eq!(buffer.text(), " worldhello");
//! ```

use crate::caret::CaretShape;
use crate::edit::Buffer;
use crate::event::{Key, KeyCode, Modifiers};
use crate::keymap::{keys, Keymap};

/// The context the editor keymaps put their bindings in: configuration
/// rebinds them as `text_area.undo = ctrl+z`.
pub const CONTEXT: &str = "text_area";

/// Ctrl+Shift+Z as a terminal that tells it apart reports it: the letter
/// in lower case, with Ctrl and Shift.
const CTRL_SHIFT_Z: Key = Key::with(
    KeyCode::Char('z'),
    Modifiers {
        shift: true,
        ctrl: true,
        alt: false,
    },
);

/// Every action, with what it does: the ones neither preset binds a key to
/// are listed too, to be rebound onto one.
const ACTIONS: &[(&str, &str)] = &[
    ("left", "move left"),
    ("right", "move right"),
    ("up", "line up"),
    ("down", "line down"),
    ("word-left", "word left"),
    ("word-right", "word right"),
    ("home", "start of the line"),
    ("end", "end of the line"),
    ("top", "start of the text"),
    ("bottom", "end of the text"),
    ("page-up", "page up"),
    ("page-down", "page down"),
    ("paragraph-up", "paragraph up"),
    ("paragraph-down", "paragraph down"),
    ("select-all", "select everything"),
    ("set-mark", "set the mark: moves select"),
    ("deselect", "drop the selection"),
    ("copy", "copy the selection"),
    ("cut", "cut the selection"),
    ("undo", "undo"),
    ("redo", "redo"),
    ("newline", "new line"),
    ("indent", "indent"),
    ("outdent", "outdent"),
    ("backspace", "delete back"),
    ("delete", "delete forward"),
    ("delete-word", "delete a word back"),
    ("delete-next-word", "delete a word forward"),
    ("delete-to-start", "delete to the line's start"),
    ("delete-to-end", "delete to the line's end"),
    ("kill-to-end", "kill to the line's end"),
    ("kill-to-start", "kill to the line's start"),
    ("kill-word", "kill the selection or a word back"),
    ("kill-next-word", "kill a word forward"),
    ("kill-previous-word", "kill to the word's start"),
    ("yank", "yank the last kill"),
    ("yank-pop", "swap the yank for an older kill"),
    ("find-next", "next match"),
    ("find-previous", "previous match"),
];

/// A keymap of every action with `bound`'s keys, the rest with none.
fn keymap_of(bound: &[(&str, Vec<Key>)]) -> Keymap {
    let mut keymap = Keymap::new(CONTEXT);
    for (action, description) in ACTIONS {
        let keys = bound
            .iter()
            .find(|(name, _)| name == action)
            .map(|(_, keys)| keys.clone())
            .unwrap_or_default();
        keymap = keymap.bind(*action, keys, *description);
    }
    keymap
}

/// The default editor keys, those `rs-rich-intuituive`'s `text_area` and
/// `input` have always had.
///
/// | Keys | Action |
/// |---|---|
/// | ←, →, ↑, ↓, Ctrl+←/→, Home, End, Ctrl+Home/End, PgUp, PgDn | `left`, `right`, `up`, `down`, `word-left`, `word-right`, `home`, `end`, `top`, `bottom`, `page-up`, `page-down`; with Shift they select |
/// | Ctrl+A | `select-all` |
/// | Esc | `deselect` (unused with nothing selected) |
/// | Ctrl+C, Ctrl+X | `copy`, `cut` |
/// | Ctrl+Z; Ctrl+Y, Ctrl+Shift+Z | `undo`; `redo` |
/// | Enter, Tab, Shift+Tab | `newline`, `indent`, `outdent` |
/// | Backspace, Delete | `backspace`, `delete` |
/// | Ctrl+W, Ctrl+U, Ctrl+K | `delete-word`, `delete-to-start`, `delete-to-end` |
/// | F3, Shift+F3 | `find-next`, `find-previous` (unused without a search) |
///
/// A key with modifiers that is bound to nothing as it is does what it
/// does without them ([`resolve`]): Shift selects, Alt+← is ←.
pub fn editor_keymap() -> Keymap {
    let mut redo = keys("ctrl+y ctrl+shift+z");
    redo.push(CTRL_SHIFT_Z);
    keymap_of(&[
        ("left", keys("left")),
        ("right", keys("right")),
        ("up", keys("up")),
        ("down", keys("down")),
        ("word-left", keys("ctrl+left")),
        ("word-right", keys("ctrl+right")),
        ("home", keys("home")),
        ("end", keys("end")),
        ("top", keys("ctrl+home")),
        ("bottom", keys("ctrl+end")),
        ("page-up", keys("pageup")),
        ("page-down", keys("pagedown")),
        ("select-all", keys("ctrl+a")),
        ("deselect", keys("escape")),
        ("copy", keys("ctrl+c")),
        ("cut", keys("ctrl+x")),
        ("undo", keys("ctrl+z")),
        ("redo", redo),
        ("newline", keys("enter")),
        ("indent", keys("tab")),
        ("outdent", keys("backtab")),
        ("backspace", keys("backspace")),
        ("delete", keys("delete")),
        ("delete-word", keys("ctrl+w")),
        ("delete-to-start", keys("ctrl+u")),
        ("delete-to-end", keys("ctrl+k")),
        ("find-next", keys("f3")),
        ("find-previous", keys("shift+f3")),
    ])
}

/// Emacs's keys, as readline and ratatui-textarea have them, beside the
/// arrows and the other keys of [`editor_keymap`].
///
/// | Keys | Action |
/// |---|---|
/// | Ctrl+F, Ctrl+B, Ctrl+N, Ctrl+P | `right`, `left`, `down`, `up` |
/// | Alt+F, Alt+B | `word-right`, `word-left` |
/// | Ctrl+A, Ctrl+E | `home`, `end` |
/// | Alt+<, Alt+> | `top`, `bottom` |
/// | Alt+V, Ctrl+V | `page-up`, `page-down` |
/// | Alt+{, Alt+} | `paragraph-up`, `paragraph-down` |
/// | Ctrl+Space; Ctrl+G | `set-mark`; `deselect` |
/// | Ctrl+H, Ctrl+D | `backspace`, `delete` |
/// | Ctrl+K, Ctrl+U | `kill-to-end`, `kill-to-start` |
/// | Ctrl+W, Alt+D, Alt+Backspace | `kill-word` (the selection, else a word back), `kill-next-word`, `kill-previous-word` |
/// | Ctrl+Y, Alt+Y | `yank`, `yank-pop` |
/// | Alt+W | `copy` |
/// | Ctrl+/, Ctrl+_ (and Ctrl+Z); Ctrl+Shift+Z | `undo`; `redo` |
/// | Ctrl+J | `newline` |
/// | Ctrl+S, Ctrl+R (and F3, Shift+F3) | `find-next`, `find-previous` |
///
/// Ctrl+A is the line's start here, so nothing selects everything; Ctrl+C
/// and Ctrl+X still copy and cut, as there are no prefix keys.
pub fn emacs_keymap() -> Keymap {
    keymap_of(&[
        ("left", keys("left ctrl+b")),
        ("right", keys("right ctrl+f")),
        ("up", keys("up ctrl+p")),
        ("down", keys("down ctrl+n")),
        ("word-left", keys("ctrl+left alt+b")),
        ("word-right", keys("ctrl+right alt+f")),
        ("home", keys("home ctrl+a")),
        ("end", keys("end ctrl+e")),
        ("top", keys("ctrl+home alt+<")),
        ("bottom", keys("ctrl+end alt+>")),
        ("page-up", keys("pageup alt+v")),
        ("page-down", keys("pagedown ctrl+v")),
        ("paragraph-up", keys("alt+{")),
        ("paragraph-down", keys("alt+}")),
        ("set-mark", keys("ctrl+space")),
        ("deselect", keys("escape ctrl+g")),
        ("copy", keys("ctrl+c alt+w")),
        ("cut", keys("ctrl+x")),
        ("undo", keys("ctrl+z ctrl+/ ctrl+_ ctrl+7")),
        (
            "redo",
            vec![Key::parse("ctrl+shift+z").expect("a key"), CTRL_SHIFT_Z],
        ),
        ("newline", keys("enter ctrl+j")),
        ("indent", keys("tab")),
        ("outdent", keys("backtab")),
        ("backspace", keys("backspace ctrl+h")),
        ("delete", keys("delete ctrl+d")),
        ("kill-to-end", keys("ctrl+k")),
        ("kill-to-start", keys("ctrl+u")),
        ("kill-word", keys("ctrl+w")),
        ("kill-next-word", keys("alt+d")),
        ("kill-previous-word", keys("alt+backspace")),
        ("yank", keys("ctrl+y")),
        ("yank-pop", keys("alt+y")),
        ("find-next", keys("f3 ctrl+s")),
        ("find-previous", keys("shift+f3 ctrl+r")),
    ])
}

/// What `key` does in `keymap`: the action, and whether it selects. A key
/// bound as it is does its action. Otherwise, a key that does not type
/// tries itself with fewer modifiers: without Shift (which then selects,
/// for a move), without Alt, then (unless it is a character) without Ctrl
/// too. So Shift+→ selects, Ctrl+Shift+← selects a word, Alt+↑ moves up,
/// and Ctrl+Alt+W deletes a word, while Alt with a character stays
/// unbound and Ctrl+Tab is never Tab (it moves the focus). A character
/// typed (no Ctrl, no Alt) is only ever an action when one binds it.
///
/// Bindings match keys exactly: a legacy terminal's Ctrl+H is only
/// Backspace where a binding says so (the Emacs preset does).
pub fn resolve(keymap: &Keymap, key: Key) -> Option<(String, bool)> {
    let bindings = keymap.bindings();
    let find = |key: Key| {
        bindings
            .iter()
            .find(|binding| !binding.sequence && binding.keys.contains(&key))
            .map(|binding| binding.action.clone())
    };
    let m = key.modifiers;
    let is_char = matches!(key.code, KeyCode::Char(_));
    let mut tries = vec![(key, false)];
    if m.shift {
        let plain = Modifiers { shift: false, ..m };
        tries.push((Key::with(key.code, plain), !is_char));
    }
    let ctrl_tab = key.code == KeyCode::Tab && m.ctrl;
    if !ctrl_tab && (!is_char || m.ctrl) {
        if m.alt {
            let ctrl = Modifiers {
                ctrl: m.ctrl,
                ..Modifiers::NONE
            };
            tries.push((Key::with(key.code, ctrl), m.shift && !is_char));
        }
        if !is_char && m.ctrl {
            tries.push((Key::new(key.code), m.shift));
        }
    }
    tries
        .into_iter()
        .find_map(|(key, select)| find(key).map(|action| (action, select)))
}

/// How an action is done: [`perform`]'s options.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Act {
    /// A move extends the selection (Shift was held).
    pub select: bool,
    /// The text may change: without it the edits are unused.
    pub editable: bool,
    /// One line (an input): the vertical moves, a new line and indenting
    /// are unused, so they reach the app.
    pub single_line: bool,
    /// The lines a page move goes.
    pub page: usize,
}

impl Default for Act {
    fn default() -> Act {
        Act {
            select: false,
            editable: true,
            single_line: false,
            page: 10,
        }
    }
}

/// What [`perform`] did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// It was done (an undo with nothing to undo is still done).
    Used,
    /// It does not apply now (an edit on read-only text, Esc with nothing
    /// selected, a search with no pattern): the key goes on to the app.
    Unused,
    /// Not a buffer action: `copy` and `cut` (the widget's own, through
    /// the app's clipboard), or one this crate does not know.
    Unknown,
}

/// Whether `action` changes the text, and so is refused read-only.
pub fn is_edit(action: &str) -> bool {
    matches!(
        action,
        "undo"
            | "redo"
            | "newline"
            | "indent"
            | "outdent"
            | "backspace"
            | "delete"
            | "cut"
            | "yank"
            | "yank-pop"
    ) || action.starts_with("delete-")
        || action.starts_with("kill-")
}

/// Do `action` (one of [`editor_keymap`]'s) on `buffer`.
pub fn perform(buffer: &mut Buffer, action: &str, act: Act) -> Outcome {
    let select = act.select;
    let vertical = matches!(
        action,
        "up" | "down"
            | "page-up"
            | "page-down"
            | "paragraph-up"
            | "paragraph-down"
            | "newline"
            | "indent"
            | "outdent"
    );
    if act.single_line && vertical || !act.editable && is_edit(action) {
        return if ACTIONS.iter().any(|(name, _)| *name == action) {
            Outcome::Unused
        } else {
            Outcome::Unknown
        };
    }
    let page = act.page.max(1) as isize;
    match action {
        "left" => buffer.left(select),
        "right" => buffer.right(select),
        "up" => buffer.vertical(-1, select),
        "down" => buffer.vertical(1, select),
        "word-left" => buffer.word_left(select),
        "word-right" => buffer.word_right(select),
        "home" => buffer.home(select),
        "end" => buffer.end(select),
        "top" => buffer.top(select),
        "bottom" => buffer.bottom(select),
        "page-up" => buffer.vertical(-page, select),
        "page-down" => buffer.vertical(page, select),
        "paragraph-up" => buffer.paragraph_up(select),
        "paragraph-down" => buffer.paragraph_down(select),
        "select-all" => buffer.select_all(),
        "set-mark" => buffer.set_mark(),
        "deselect" => {
            if buffer.selection().is_none() && !buffer.marking() {
                return Outcome::Unused;
            }
            buffer.clear_mark();
        }
        "undo" => {
            buffer.undo();
        }
        "redo" => {
            buffer.redo();
        }
        "newline" => buffer.insert("\n"),
        "indent" => buffer.indent(),
        "outdent" => buffer.outdent(),
        "backspace" => buffer.backspace(),
        "delete" => buffer.delete(),
        "delete-word" => buffer.delete_word(),
        "delete-next-word" => buffer.delete_next_word(),
        "delete-to-start" => buffer.delete_to_start(),
        "delete-to-end" => buffer.delete_to_end(),
        "kill-to-end" => buffer.kill_to_end(),
        "kill-to-start" => buffer.kill_to_start(),
        "kill-word" => buffer.kill_word(),
        "kill-next-word" => buffer.kill_next_word(),
        "kill-previous-word" => buffer.kill_previous_word(),
        "yank" => {
            buffer.yank();
        }
        "yank-pop" => {
            buffer.yank_pop();
        }
        "find-next" | "find-previous" => {
            if buffer.search().is_none_or(|s| s.pattern().is_empty()) {
                return Outcome::Unused;
            }
            buffer.select_match(action == "find-next");
        }
        _ => return Outcome::Unknown,
    }
    Outcome::Used
}

/// What a [`KeyMode`] sees with a key: the buffer, and how it is edited.
pub struct ModeCx<'a> {
    pub buffer: &'a mut Buffer,
    /// The text may change.
    pub editable: bool,
    /// One line (an input).
    pub single_line: bool,
    /// The lines in view, for page moves.
    pub page: usize,
}

/// What a [`KeyMode`] did with a key.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ModeKey {
    /// It used the key.
    Used,
    /// The widget handles it as it would without the mode: through the
    /// keymap, or typed (a Vim insert mode passes most keys).
    Pass,
    /// Nobody here wants it: it goes on to the app (`:` or `/` opening a
    /// command or a search bar).
    Unused,
    /// Copy this text to the app's clipboard; the key was used.
    Copy(String),
}

/// A modal key handler over a buffer: a preset that is more than a keymap,
/// such as Vim's (the `vim` feature's [`Vim`](crate::vim::Vim)). It sees
/// each key before the widget's keymap.
pub trait KeyMode {
    /// Handle `key`.
    fn key(&mut self, key: Key, cx: &mut ModeCx<'_>) -> ModeKey;

    /// The caret's shape now (a block in Vim's normal mode, a bar while
    /// inserting); `None` leaves it to the widget.
    fn caret_shape(&self) -> Option<CaretShape> {
        None
    }

    /// The mode's name to show (`NORMAL`, `INSERT`, `VISUAL`), if any.
    fn label(&self) -> Option<String> {
        None
    }

    /// Keys typed so far of a command not yet complete (`2d`), to show.
    fn pending(&self) -> Option<String> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::edit::Pos;

    fn run(keymap: &Keymap, buffer: &mut Buffer, keys: &str) {
        for name in keys.split(' ') {
            let key = Key::parse(name).expect("a key");
            let (action, select) = resolve(keymap, key).unwrap_or_else(|| panic!("{name}"));
            let act = Act {
                select,
                ..Act::default()
            };
            perform(buffer, &action, act);
        }
    }

    #[test]
    fn modifiers_fall_back_as_the_hard_coded_keys_did() {
        let keymap = editor_keymap();
        let key = |name: &str| resolve(&keymap, Key::parse(name).unwrap());
        let is = |name: &str, action: &str, select: bool| {
            assert_eq!(key(name), Some((action.to_string(), select)), "{name}");
        };
        is("shift+left", "left", true);
        is("ctrl+shift+left", "word-left", true);
        is("ctrl+alt+left", "word-left", false);
        is("alt+left", "left", false);
        is("alt+up", "up", false);
        is("ctrl+shift+home", "top", true);
        is("ctrl+enter", "newline", false);
        is("ctrl+backspace", "backspace", false);
        is("ctrl+alt+w", "delete-word", false);
        let ctrl_shift_y = Key::with(
            KeyCode::Char('y'),
            Modifiers {
                shift: true,
                ..Modifiers::CTRL
            },
        );
        assert_eq!(resolve(&keymap, ctrl_shift_y), Some(("redo".into(), false)));
        is("ctrl+shift+z", "redo", false);
        assert_eq!(resolve(&keymap, CTRL_SHIFT_Z), Some(("redo".into(), false)));
        assert_eq!(key("ctrl+tab"), None, "Ctrl+Tab moves the focus");
        assert_eq!(key("alt+x"), None);
        assert_eq!(key("x"), None, "typed");
        assert_eq!(key("ctrl+h"), None, "Ctrl+H is not Backspace by default");
    }

    #[test]
    fn rebinding_moves_a_key() {
        let mut keymap = editor_keymap();
        keymap.rebind("yank", keys("ctrl+y"));
        let key = Key::parse("ctrl+y").unwrap();
        assert_eq!(resolve(&keymap, key), Some(("yank".into(), false)));
        assert_eq!(
            keymap.keys("redo"),
            keys("ctrl+shift+z")
                .into_iter()
                .chain([CTRL_SHIFT_Z])
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn emacs_kills_and_yanks() {
        let keymap = emacs_keymap();
        let mut b = Buffer::new("one two three");
        run(&keymap, &mut b, "alt+f alt+d alt+d ctrl+e ctrl+y");
        assert_eq!(b.text(), "one two three");
        run(&keymap, &mut b, "ctrl+a ctrl+k ctrl+y ctrl+y");
        assert_eq!(b.text(), "one two threeone two three");
        run(&keymap, &mut b, "ctrl+a ctrl+f ctrl+space alt+f");
        assert_eq!(b.selected_text().as_deref(), Some("ne"));
        run(&keymap, &mut b, "ctrl+g ctrl+b ctrl+h");
        assert_eq!(b.caret(), Pos::new(0, 1));
        assert!(b.text().starts_with("oe two"));
    }

    #[test]
    fn read_only_and_single_line_leave_keys_to_the_app() {
        let mut b = Buffer::new("x");
        let read_only = Act {
            editable: false,
            ..Act::default()
        };
        assert_eq!(perform(&mut b, "backspace", read_only), Outcome::Unused);
        assert_eq!(perform(&mut b, "end", read_only), Outcome::Used);
        let line = Act {
            single_line: true,
            ..Act::default()
        };
        assert_eq!(perform(&mut b, "up", line), Outcome::Unused);
        assert_eq!(perform(&mut b, "deselect", line), Outcome::Unused);
        assert_eq!(perform(&mut b, "copy", line), Outcome::Unknown);
        assert_eq!(perform(&mut b, "find-next", line), Outcome::Unused);
        assert_eq!(perform(&mut b, "no-such", line), Outcome::Unknown);
    }
}
