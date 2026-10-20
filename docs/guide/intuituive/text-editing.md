# Editing text: keys, search and presets

A [`text_area`](components.md#text-areas) and an `input` edit through
rs-rich-interact's `edit::Buffer`, and what their keys do is a keymap of
named actions from `rich_interact::edit_keys`. This page covers the options
on top: rebinding the keys, the Emacs and Vim presets, search with a find
bar, word wrap, the cursor line, the caret's shape, tabs, the undo history,
and an input's validation message. Nothing here changes an editor unless
it is asked for: a `text_area` with none of these options behaves as it
always has.

`cargo run -p rs-rich-intuituive --features vim --example textarea` runs a
gallery of them, one tab each (F1 to F7): ratatui-textarea's examples
rebuilt.

## Keys are actions

The default keymap (`edit_keys::editor_keymap()`) is the table in
[Text areas](components.md#text-areas), with each key named by its action:
`left`, `word-left`, `home`, `top`, `page-up`, `select-all`, `deselect`,
`copy`, `cut`, `undo`, `redo`, `newline`, `indent`, `backspace`,
`delete-word`, `delete-to-start`, `delete-to-end`, `find-next` and the rest.
A few actions have no key by default, to be bound: `delete-next-word`,
`paragraph-up` and `paragraph-down`, `set-mark`, and the kill ring's
`kill-to-end`, `kill-to-start`, `kill-word`, `kill-next-word`,
`kill-previous-word`, `yank` and `yank-pop`.

```rust
text_area(notes)
    .rebind_key("yank", "ctrl+y")            // Ctrl+Y leaves `redo`
    .rebind_key("kill-to-end", "ctrl+k")     // and takes `delete-to-end`'s key
    .rebind_key("delete-to-end", "")         // no key at all
    .rebind_key("paragraph-down", "alt+n")
```

- **`.rebind_key(action, keys)`** moves keys onto an action on this editor
  only; a key moved onto one action leaves the action that had it.
- **`.edit_keymap(keymap)`** replaces the whole keymap with one of your own
  built from rs-rich-interact's `Keymap`.
- A key with modifiers that is bound to nothing as it is does what it does
  without them: Shift selects (Shift+→, Ctrl+Shift+←), Alt+↑ is ↑. A
  character typed is text unless a binding names it.
- The context is `text_area`, so a configuration file's
  [installed](../interact/custom-components.md#the-keymap) overrides reach
  every editor: `text_area.undo = ctrl+z, ctrl+/`.

## The Emacs preset

`.emacs_keys()` takes `edit_keys::emacs_keymap()`: Emacs's and readline's
keys beside the arrows.

| Keys | What they do |
|---|---|
| Ctrl+F, Ctrl+B, Ctrl+N, Ctrl+P | right, left, down, up |
| Alt+F, Alt+B | a word right, left |
| Ctrl+A, Ctrl+E; Alt+<, Alt+> | the line's start and end; the text's |
| Alt+V, Ctrl+V; Alt+{, Alt+} | a page up and down; a paragraph |
| Ctrl+Space, Ctrl+G | set the mark (moves select from it); drop it |
| Ctrl+H, Ctrl+D | delete back, forward |
| Ctrl+K, Ctrl+U | kill to the line's end (at its end, the line break), to its start |
| Ctrl+W, Alt+D, Alt+Backspace | kill the selection or a word back, a word forward, to the word's start |
| Ctrl+Y, Alt+Y | yank the last kill; swap it for an older one |
| Alt+W | copy (and put it on the kill ring) |
| Ctrl+/, Ctrl+_; Ctrl+Shift+Z | undo; redo |
| Ctrl+S, Ctrl+R | the next and previous match of a search |

Kills one after another join: Ctrl+K twice takes a line and its break, and
Ctrl+Y puts both back. The ring keeps sixteen kills. Copying with Ctrl+C
(or Alt+W) puts the text on the ring too, so it can be yanked.

## The Vim preset (the `vim` feature)

```toml
rs-rich-intuituive = { version = "0.0.6", features = ["vim"] }
```

```rust
--8<-- "crates/rich-intuituive/examples/textarea.rs:vim"
```

`.vim()` (or `.vim_inserting()`, for a field typed into first) gives the
editor rs-rich-interact's `vim::Vim`, a **key mode**: a preset that sees
each key before the keymap and keeps its own state.

- **Modes:** normal, insert (`i a I A o O`, `s S C`, `c` with a motion),
  visual (`v`) and visual line (`V`); Esc goes back to normal.
- **Motions:** `h j k l` and the arrows, `w b e`, `0 ^ $`, `gg G` (with a
  count, that line), `{ }`, `f t F T` and a character, `n N` (the
  search's matches), Ctrl+D/U and Ctrl+F/B.
- **Operators:** `d c y > <` with a motion, doubled for lines (`dd`, `cc`,
  `yy`, `>>`), or on a visual selection; `x X D C s S J r p P u` and
  Ctrl+R.
- **Counts** before a command or a motion (`3dw`, `d3w`, `5j`), and `.`
  repeats the last change, typing included.

Deleted and yanked text goes on the buffer's kill ring, which `p` puts
back. The caret is a block in normal mode and a bar while inserting.
`.mode_status(signal)` writes `NORMAL`, `INSERT`, `VISUAL` or `V-LINE`, and
the keys of a command not yet complete (`NORMAL 2d`), for a status line.
`:`, `/` and keys Vim does not know go on to the app's bindings, so they
can open a command line or the find bar.

```text
╭─ vim ────────────────────────────────────────────────────╮
│1 │ fn main() {                                           │
│2 │     for word in words {                               │
│3 │         println!("{word}");                           │
│4 │     }                                                 │
│5 │ }                                                     │
│6 │                                                       │
╰──────────────────────────────────────────────────────────╯
 NORMAL  dw · 3x · yyp · cw · . repeats
```

Vim is a plugin over a generic hook: `.key_mode(mode)` takes any
`edit_keys::KeyMode`, so another modal preset is a type of your own.

## Search and the find bar

```rust
--8<-- "crates/rich-intuituive/examples/textarea.rs:editor"
```

`TextSearch::new()` is a search the app drives (it is `Copy`; make it in
the app's closure). `.search(search)` gives it to a text area:

- `search.set("fish")`, `search.set_regex(true)` (fancy-regex's syntax,
  look-around included) and `search.set_ignore_case(true)` set what is
  found; a new pattern selects the first match from the caret, as an
  editor's incremental search does. Matches are found line by line, so one
  never spans a line break.
- `search.next()` and `search.previous()` select the next or previous
  match, round past the ends; in the area F3 and Shift+F3 do the same.
- `search.found()` is `Some((3, 12))` (the third of twelve; 0 when the
  selection is not a match), and `search.error()` says why a regular
  expression does not compile. Read in a node, they update.
- Every match in view is drawn in the theme's `text_area.search` style,
  the selected one in `text_area.search.current`.

`search_bar(search)` is a find bar over it: `Find`, an input bound to the
pattern, where it is (`2/3`, `no matches`, or the error), and `.*` and `Aa`
while those flags are on. In it, Enter and ↓ go to the next match, ↑ to the
one before, Alt+R takes the pattern as a regular expression and Alt+C
matches either case. Its styles are `search_bar.label`, `search_bar.status`
and `search_bar.error`.

```text
╭─ main.rs ────────────────────────────────────────────────╮
│1 │ fn main() {                                           │
│2 │     let words = ["one", "fish", "two", "fish"];       │
│3 │     for word in words {                               │
╰──────────────────────────────────────────────────────────╯
Find fish                                     1/2
```

## Wrapping, the cursor line and the caret

- **`.wrap_mode(WrapMode::Word)`** breaks long lines after a space, so
  words stay whole (a word longer than the row breaks where it must);
  `WrapMode::Grapheme` is the default character wrap and `WrapMode::None`
  scrolls sideways, as `.soft_wrap(false)` does. `WrapMode` is
  `intuituive::interact::edit::WrapMode`.
- **`.cursor_line()`** draws the caret's line, all its rows, in the theme's
  `text_area.cursor_line` style: underlined unless the theme names it.
- **`.caret_shape(CaretShape::Bar)`** (or `Block`, `Underline`, from
  `intuituive::interact::caret`) sets the terminal's cursor to that shape
  (DECSCUSR) while the editor has the focus. The app puts back the user's
  own shape when the focus moves to a widget with none, when it hands the
  terminal over and when it ends; an app that never sets a shape never
  sends one. Any widget can ask for one through `Widget::caret_shape`.
- **`.draw_caret()`** draws the caret in the cells instead of using the
  terminal's cursor, in the theme's `text_area.caret` style (reversed, or
  underlined for `CaretShape::Underline`): it then shows in a screenshot,
  a page drawn from the cells, or a terminal with its cursor hidden.

## Tabs and the undo history

- **`.tab_width(n)`** sets how many cells a tab takes (4 by default) and
  how many spaces Tab indents with; **`.hard_tabs()`** indents with a tab
  character instead.
- **`.undo_limit(Some(n))`** keeps the last `n` undo steps (1000 by
  default; `None` keeps them all). A typed run is one step.

## Validating an input

```rust
--8<-- "crates/rich-intuituive/examples/textarea.rs:single"
```

`.validate(|text| Option<String>)` checks an `input`'s text: while it
returns a message, the node is in error (styled `:error` by a stylesheet,
as [`error_when`](panels.md) does), the message shows under the text in the
theme's `input.error` style when the input has two rows (beside it, at the
right, when it has one), and assistive technology is given it: linear mode
reads `Age, text box: x, invalid: a number`, and a DOM renderer gets
it as `aria-description`.

`Node::error_message(|| Option<String>)` does the same for any node, beside
`error_when` (either makes it invalid); `AccessNode::error` holds the
message.
