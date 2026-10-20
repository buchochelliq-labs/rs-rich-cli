# The text buffer and editor keys

`rich_interact::edit::Buffer` is the editing under every text editor in the
project: rs-rich-interact's own `TextArea`, and intuiTUIve's `text_area` and
`input`. A fix to how the caret moves or how text is killed lands in all of
them. `rich_interact::edit_keys` names what an editor's keys do, and
`rich_interact::vim` (the `vim` feature) is a Vim preset over the buffer.
For the intuiTUIve options built on them, see
[Editing text](../intuituive/text-editing.md).

## The buffer

```rust
use rich_interact::edit::{Buffer, Search};

let mut buffer = Buffer::new("one fish\ntwo fish");
buffer.kill_to_end();                 // "one fish" onto the kill ring
buffer.yank();                        // and back
buffer.set_search(Some(Search::plain("fish")));
buffer.select_match(true);            // the first "fish", selected
assert_eq!(buffer.match_count(), 2);
```

- **Editing:** `insert`, `backspace`, `delete`, `delete_word`,
  `delete_next_word`, `delete_to_start`, `delete_to_end`, `indent`,
  `outdent`, `indent_lines`, `outdent_lines` and `replace`, each one undo
  step (a typed run is one); `undo` and `redo`.
- **Moves,** each with a flag to select: `left`, `right`, `word_left`,
  `word_right`, `vertical`, `home`, `end`, `top`, `bottom`,
  `paragraph_up` and `paragraph_down` (to the blank line before or after
  the paragraph). `set_mark` makes every move select until an edit or
  `clear_mark`, as Emacs's mark does.
- **The kill ring:** `kill_to_end` (at a line's end, the line break),
  `kill_to_start`, `kill_word` (the selection, else back to a space),
  `kill_next_word`, `kill_previous_word` and `kill(start, end, forward)`
  delete and keep what they took; kills straight after each other join.
  `yank` puts the newest back, and `yank_pop` right after it swaps in the
  one before, round the ring (sixteen kills). `push_kill` puts text on it.
- **Search:** `Search::plain(text)`, `Search::regex(pattern)` (fancy-regex,
  an `Err` with the reason when it does not compile) and
  `.ignore_case(true)`. Set one with `set_search`; `matches(lines)` gives
  the matches on some lines (what a view highlights), `match_count` and
  `match_index` where the selection is among them, `search_forward` and
  `search_back` move the caret to the next or previous match and
  `select_match` selects it, round past the ends.
- **Settings:** `set_limit` (characters, grapheme clusters), 
  `set_history_limit` (1000 undo steps by default; `None` keeps them all),
  `set_tab_width` (4 cells by default, and the indent's width) and
  `set_hard_tabs` (indent with a tab). `set_text` keeps them, the ring and
  the search.
- **Wrapping:** `wrap_line(line, room, tab, mode)` gives the rows a line
  takes, as byte ranges: `WrapMode::Grapheme` breaks at a character,
  `WrapMode::Word` after a space, `WrapMode::None` not at all.

## Editor keys

```rust
use rich_interact::edit::Buffer;
use rich_interact::edit_keys::{emacs_keymap, perform, resolve, Act};
use rich_interact::Key;

let keymap = emacs_keymap();
let mut buffer = Buffer::new("hello world");
if let Some((action, select)) = resolve(&keymap, Key::parse("alt+d").unwrap()) {
    perform(&mut buffer, &action, Act { select, ..Act::default() });
}
```

- **`editor_keymap()`** is the default (the arrows, Ctrl+A select all,
  Ctrl+Z/Ctrl+Y undo and redo, Ctrl+W/U/K delete), and **`emacs_keymap()`**
  the Emacs preset (Ctrl+F/B/N/P, Alt+F/B, Ctrl+A/E, kills and yanks, the
  mark). Both declare every action, in context `text_area`, so either can
  be rebound one action at a time and listed in a help.
- **`resolve(keymap, key)`** says which action a key does, and whether it
  selects: a key bound to nothing as it is tries itself without Shift
  (which then selects), without Alt, then without Ctrl.
- **`perform(buffer, action, act)`** does it: `Outcome::Used`,
  `Outcome::Unused` (an edit on read-only text, a vertical move in one
  line, Esc with nothing selected: the key goes on), or
  `Outcome::Unknown` (`copy` and `cut`, which need the app's clipboard).
- **`KeyMode`** is the hook for a modal preset: it sees each key first with
  the buffer (`ModeCx`), and says `Used`, `Pass` (edit as usual),
  `Unused` (to the app) or `Copy(text)`; it can name a caret shape and a
  mode label.

`caret::CaretShape` (block, bar, underline) gives the DECSCUSR sequences
that set the terminal's cursor shape, and `caret::RESET` puts back the
user's.

## The Vim preset (the `vim` feature)

`vim::Vim` is a `KeyMode` with Vim's normal, insert, visual and visual line
modes, its motions (`hjkl wbe 0^$ gg G {} f t F T n N`), the operators
`d c y > <` with motions or doubled, `x X D C s S J r p P u` and Ctrl+R,
counts, and `.`. intuiTUIve's `.vim()` puts it on a text area.

## TextArea

The interact `TextArea` (`rich write`) runs on the same buffer, so the
kill ring, undo, paragraph moves and the rest are its actions too, bound to
no key by default: `TextArea::rebind("yank", keys("ctrl+y"))` binds one.
`.emacs_keys()` binds Emacs's: Ctrl+F/B/N/P and Alt+F/B move, Ctrl+K,
Ctrl+U, Ctrl+W, Alt+D and Alt+Backspace kill, Ctrl+Y and Alt+Y yank, and
Alt+{ and Alt+} move by paragraph.
