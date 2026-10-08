# Testing with screen readers

A screen reader cannot run in CI, so intuiTUIve's accessibility is checked
by hand at each release, with the `access` example and the checklist on
this page. The tests in `crates/rich-intuituive/tests/a11y.rs` and
`tests/linear.rs` pin what the app writes. This page checks what a person
hears.

See [Accessibility](index.md#accessibility) for what the two modes do.

## The example

`examples/access.rs` has a tab strip, a tree, a list, a check box and a
switch, three buttons, a live status line with a spinner that is hidden
from screen readers, a decorative rule that is hidden too, a toast and a
dialog.

```bash
cargo build -p rs-rich-intuituive --example access
target/debug/examples/access                # drawn as usual
target/debug/examples/access --accessible   # text mode, the cursor on the focus
target/debug/examples/access --linear       # lines of text, no screen
```

The environment variable does the same for any app:

```bash
INTUITUIVE_ACCESSIBLE=1 target/debug/examples/access       # the cursor mode
INTUITUIVE_ACCESSIBLE=linear target/debug/examples/access  # linear mode
```

On Windows, in PowerShell: `$env:INTUITUIVE_ACCESSIBLE = "linear"`, then
`target\debug\examples\access.exe`.

The keys:

| Key | Does |
|---|---|
| Tab, Shift+Tab | Moves between the tab strip, the tree, the list (or the settings) and the buttons |
| ← → | On the tab strip: switch between Files and Settings. In the tree: → opens a folder, ← closes it |
| ↑ ↓ | Moves in the tree and the list |
| Space | Ticks the focused setting |
| Enter | Presses the focused button |
| s | Saves (a toast) |
| d | Asks before deleting (a dialog: y or n) |
| r | Reloads (the status is busy for a second and a half, then changes) |
| q | Quits |

## What should be spoken

In **linear mode**, the screen reader reads each line as it is written.
The lines are the same on every screen reader:

| Step | Keys | Lines written |
|---|---|---|
| Start | (none) | `Notes`, `→ Sections, tab list, 1 of 2: Files, selected`, then the rest of the screen: the regions, the tree, the list, `10 files, loaded once`, the three buttons, the help line. The decorative rule is not written. |
| Move to the tree | Tab | `→ Folders, tree, 1 of 3: src, selected, collapsed` |
| Open a tree node | → | `Folders, tree, 1 of 5: src, selected, expanded` |
| Move through a list | Tab, ↓ | `→ Files, list, 1 of 10: notes-1.md, selected`, then `Files, list, 2 of 10: notes-2.md, selected` |
| A toast | s | `Saved` |
| Switch tabs | Shift+Tab twice, → | `Sections, tab list, 2 of 2: Settings, selected`, `Settings, region`, `Wrap lines, check box, checked`, `Dark theme, switch, not checked` |
| Toggle a check box | Tab, Space | `→ Wrap lines, check box, checked`, then `Wrap lines, check box, not checked` |
| Open a dialog | d | `Delete, dialog`, `Delete notes-1.md? y / n` |
| Close it | n | `→ Wrap lines, check box, not checked` (only the focus: nothing else changed) |
| A live status | r | `Loading…, busy`, then after a second and a half `10 files, loaded 2 times` (the spinner's glyph is never read) |

In **the cursor mode** (`--accessible`), the screen is drawn as text, with
no box lines and no colour, and a `>` on the selected item. The terminal's
cursor sits on the focused item. A screen reader that follows the cursor
reads the line it lands on. That is the whole terminal row, so on the Files
tab the tree's row and the list's row are read together. Announcements
(toasts, a dialog opening, the live status) reach a screen reader only as
the text that appears on the screen. Linear mode writes them as lines.

| Step | Keys | Expected |
|---|---|---|
| Start | (none) | The cursor is on `>Files` in the tab strip; that row is read |
| Move to the tree | Tab | The row with `> ▸ src` is read |
| Open a tree node | → | The row with `> ▾ src` is read, and the cursor stays on it |
| Move through a list | Tab, ↓ | Each ↓ reads the row with the new `> notes-N.md` |
| Toggle a check box | Shift+Tab twice, →, Tab, Space | `[ ] Wrap lines` (the focused row) is read |
| Open a dialog | d | The dialog's text appears; the reader may read it as new text |
| A toast | s | `Saved` appears at the bottom right; the reader may read it as new text |
| A live status | r | `Loading…`, then `10 files, loaded 2 times`, appear on the status row |
| Frames do not move the cursor | wait while the status spins | Nothing new is read: the cursor stays on the focus while the spinner is drawn |

## Orca (Linux, GNOME Terminal)

1. Start Orca: Super+Alt+S, or `orca` from a terminal. In Orca's
   preferences, under Speech, set the verbosity to Verbose for the first
   run.
2. Open GNOME Terminal, build the example as above, and run
   `target/debug/examples/access --linear`.
3. Go through the linear-mode table above. Orca speaks text that is added
   to a terminal as it arrives. Check that each step's lines are spoken,
   in order, and that nothing else is: no rule of `≈` characters, and no
   spinner glyphs while the status is busy.
4. Press q, then run `target/debug/examples/access --accessible` and go
   through the cursor-mode table. Orca follows the caret in a terminal. To
   hear the line the cursor is on again, use Orca's "speak current line":
   KP_8 on the desktop layout, Orca+I on the laptop layout.
5. While the status spins (r), listen for the cursor wandering. Nothing
   should be read between the spinner's frames.
6. Note anything spoken twice, anything missing, and anything read out of
   order in the Results table.

## NVDA (Windows, Windows Terminal)

- Run the example in Windows Terminal (PowerShell or cmd). NVDA reads new
  text in Windows Terminal as it arrives. In NVDA's settings, under Object
  Presentation, keep "Report dynamic content changes" on.
- In linear mode, every step's lines should be read as in the table.
- In the cursor mode, NVDA+↑ (desktop layout) or NVDA+L (laptop layout)
  reads the line under the cursor. Check that it is the focused item's row
  after each key.
- Windows Terminal's own accessibility settings can change what is read:
  note its version, and NVDA's, in the Results table.

## VoiceOver (macOS, Terminal.app)

- Turn VoiceOver on with Cmd+F5. Run the example in Terminal.app.
- VoiceOver reads new output in Terminal.app. In linear mode, check every
  step's lines in the table.
- In the cursor mode, VO+L (VO is Control+Option) reads the current line.
  Check it after each key. VO+A reads everything from the cursor on.
- Terminal.app moves VoiceOver's cursor with the text cursor only while
  the window has the keyboard focus: keep it focused while testing.

## Results

Fill this in at the release test: one row per check and mode, with what
was heard, or ✓ when it matched the tables above. Note the screen reader's
version and the terminal's version under the table.

| Check | Mode | Orca, GNOME Terminal | NVDA, Windows Terminal | VoiceOver, Terminal.app | Notes |
|---|---|---|---|---|---|
| Start: the screen is read once | linear | | | | |
| Moving through a list | linear | | | | |
| Opening a tree node | linear | | | | |
| Toggling a check box | linear | | | | |
| Opening and closing a dialog | linear | | | | |
| A toast | linear | | | | |
| The live status, busy then done | linear | | | | |
| Decoration and the spinner are not read | linear | | | | |
| The cursor is on the focus at the start | cursor | | | | |
| Moving through a list | cursor | | | | |
| Opening a tree node | cursor | | | | |
| Toggling a check box | cursor | | | | |
| Opening a dialog | cursor | | | | |
| A toast | cursor | | | | |
| The live status | cursor | | | | |
| No cursor travel while the status spins | cursor | | | | |

Versions:

- Orca: … on …, GNOME Terminal: …
- NVDA: …, Windows Terminal: …
- VoiceOver on macOS …, Terminal.app: …
