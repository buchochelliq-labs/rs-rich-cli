# intuiTUIve against Textual: a parity audit

Status: planning note, 2026-10-27. Each gap worth doing has an issue,
labelled `intuituive`; the numbers are in the [gap list](#prioritised-gaps).

## Why this note

[Textual](https://github.com/Textualize/textual) is the most complete
terminal UI framework in Python, by Will McGugan, the author of rich. Since
intuiTUIve is built on our port of rich, Textual is the closest comparison.
This note goes through Textual's widgets and framework features one by one.
It records what intuiTUIve has, what it lacks, and where each gap belongs.
[Cornacai (#794)](https://github.com/buchochelliq-labs/rs-rich-cli/issues/794),
the agent front end, will be intuiTUIve's showcase app, so its needs set
the order of the gap list.

- **Compared against:** Textual 8.2.8, `main` at
  [`5e5b7ef`](https://github.com/Textualize/textual/tree/5e5b7ef58b8c5572c13a3ff9027d756623661495)
  (2026-10-09). Source links below point at that commit. Doc links point at
  [textual.textualize.io](https://textual.textualize.io).
- **The intuiTUIve side** was checked by reading `crates/rich-intuituive/src`
  at 0.0.23's extras branch (`f899726c`), not by matching names. Where a
  Textual widget maps to something in another shape, the table says so.
- **Licence:** Textual is MIT. We may follow its designs closely, with
  credit to Textual in the module docs and the guide. Its built-in themes'
  palettes may be reused, with credit. Our code is still written fresh, in
  Rust and in intuiTUIve's own style: signals and closures, builders on
  `Node`. We do not translate its Python. This differs from Toad, which is
  AGPL: #794 forbids reading Toad's source. Textual's source is fine to
  read.
- **Core or plugin:** AGENTS.md's rule applies. intuiTUIve's core holds
  only generic components. A domain component is a plugin: an
  off-by-default feature module or another crate, built on a generic core
  piece. For each gap below, a domain proposal also names the generic shape
  underneath it. Most of Textual's widgets are generic and belong in core.
  The exception is `DirectoryTree`, which is the `files` plugin's.

Status key: **✓** has it (or more); **partial** has it with a gap, and the
gap is named; **✗** missing; **won't-do** not worth building, with the
reason.

## Widgets

Every widget in `src/textual/widgets/_*.py`. Constructors are intuiTUIve's.
Modules are under `crates/rich-intuituive/src/`.

| Textual widget | intuiTUIve equivalent | Status | Gap, and where it goes |
|---|---|---|---|
| `Button` | `button`, `button_of`, `ButtonVariant` (`controls.rs`) | ✓ | Three variants are themed (default, primary, danger); Textual has five (it adds success and warning). Two theme names add them. |
| `Checkbox` | `checkbox`, `checkbox_tri`, `checkbox_of` (`controls.rs`) | ✓ | Has a mixed state, which Textual's lacks |
| `Collapsible` | `collapsible(title, open, child)` (`collapsible.rs`) | partial | The title is a fixed `String`. A tool-call card needs a title that follows state, plus a status slot (spinner, ✓/✗) and a summary line. Core. |
| `ContentSwitcher` | `switch(key, build)`, `keyed`, `.keep_last(n)` (`builtin.rs`) | ✓ | |
| `DataTable` | `table`, `table_with`, `virtual_table`, `TableOptions` (`widgets.rs`) | partial | Already has sorting, a row cursor, a cell cursor (`.cells`), column highlight, frozen columns, zebra rows (`row_style`), resizable columns, rows from a closure (so cell updates are signal writes) and virtual rows. Missing: fixed (frozen) **rows**, row labels, and cells holding renderables rather than markup. Core. |
| `Digits` | `digits` (`digits.rs`) | ✓ | |
| `DirectoryTree` | `files::miller`, `files::file_list`, `files::pick_file` (`files` feature); generic `tree_lazy` | partial | No tree view of a directory. Plugin: `files::dir_tree` on the core's `tree_lazy`. |
| `Footer` | `key_hints`, `key_hints_with` (`bars.rs`) | ✓ | Built from described bindings, greyed when unavailable |
| `Header` | `status_bar` segments (`bars.rs`) | partial | Nothing gives an app a title and subtitle (set per screen) with an optional clock and a tall mode. Core: `header()` on `status_bar`. |
| `HelpPanel` | `cx.help()`, `HelpOptions` (`help.rs`) | partial | The help is a modal. Textual's panel is docked beside the app and follows the focus. Core: `key_panel()` node. |
| `Input` | `input`, `number_input`, `.validate`, `.complete`, `.history`, `.mask`, `.max_length` (`input.rs`, `complete.rs`, `text_edit.rs`) | ✓ | Ghost-text completion is the equivalent of Textual's `Suggester` |
| `KeyPanel` | `cx.help_with(HelpOptions::new().all_panes(true))` | partial | Same gap as `HelpPanel`: a docked panel, opened by the system command "Keys" |
| `Label` | `label` (`node.rs`) | ✓ | |
| `Link` | `[link=url]` markup gives an OSC 8 hyperlink (`screen.rs` keeps it on the style) | partial | No focusable link that Enter or a click opens (through `open`), and markup cannot run an action (Textual's `@click`). Core. |
| `ListItem` | `ListRow` (markup, trailing, marker, style) (`list.rs`) | partial | A row is markup, not a node. See `ListView`. |
| `ListView` | `list`, `list_rows`, `virtual_list` (`list.rs`, `widgets.rs`) | partial | The cursor, scrolling and context menu are there. Rows cannot be arbitrary nodes (a card, a row with a button), as a `ListItem` holding widgets can. Core: `list_of(len, \|i\| Node, selected)`. |
| `LoadingIndicator` | `spinner`, `.loading_when`, `fx::placeholder` (`spinner.rs`, `fx.rs`) | ✓ | |
| `Log` | `Log::new(cap).view()` (`log.rs`) | ✓ | Adds levels, targets, filters and follow, which Textual's lacks |
| `Markdown` | `renderable(Markdown::new(..))`, rs-rich's port (`crates/rich/src/markdown.rs`) | partial | Static only. The whole document is parsed and drawn again on each change. Links cannot be clicked or focused, code blocks are not retained nodes, and nothing streams: Textual has `Markdown.append` and `MarkdownStream`. Core. **Cornacai's first need.** |
| `MarkdownViewer` | — | ✗ | Missing a table of contents, link-following, history (back and forward) and anchors. Core, on the streaming `markdown` node. |
| `MaskedInput` | — | ✗ | `.mask(char)` is a password mask, not a template such as `9999-99-99` or `>AAA`. Core. |
| `OptionList` | `list`, `.multiline()`, `list_search`, `.row_style` | ✓ | Missing separators between options. Small, tracked with `ListView`. |
| `Placeholder` | `fx::placeholder` | partial | `fx::placeholder` is a loading skeleton. Textual's is for laying out a prototype: each one labelled with its id, in cycling colours. Core, small. |
| `Pretty` | `renderable(Pretty::new(&value))` or `Pretty::from_value` (rs-rich's port) | ✓ | Rust has no reflection, so it pretty-prints `Debug` or a `PrettyValue` |
| `ProgressBar` | `renderable(rich::ProgressBar)`, `seg_gauge`, `jobs_panel` | partial | No progress node that shows a bar, a percentage and an ETA, with an indeterminate pulse. Core, small. |
| `RadioButton` | `radio` rows | ✓ | |
| `RadioSet` | `radio`, `radio_keyed` (`controls.rs`) | ✓ | |
| `RichLog` | `Log` | partial | A log line is markup only. Textual's `RichLog.write` takes any renderable (a `Table`, `Syntax`, `Pretty`), which a tool's output needs. Core. |
| `Rule` | `rule`, `.divider()` (`rule.rs`) | ✓ | Adds titles and border merging |
| `Select` | `select_field`, `select_field_keyed`, `SelectOption::disabled` (`select_field.rs`) | ✓ | |
| `SelectionList` | `multi_select` (`select.rs`) | ✓ | Adds a visual range |
| `Sparkline` | `sparkline` (`viz` feature) | ✓ | A plugin here, by the core rule |
| `Static` | `text`, `text!`, `label`, `renderable` | ✓ | |
| `Switch` | `switch_toggle`, `switch_of` | ✓ | |
| `Tab` | `tabs` titles (`widgets.rs`) | ✓ | |
| `TabPane` | a `switch` page, kept with `keep_last` | ✓ | |
| `TabbedContent` | `tabs` + `switch`; `tab_set`, `tabs_for` for tabs that open and close (`tab_set.rs`) | ✓ | Composed from two nodes, not one widget. The guide shows how. |
| `Tabs` | `tabs`, `.divider`, `.fit`, `.auto_hide`, `.on_select` | ✓ | |
| `TextArea` | `text_area`, `.language`, `.line_numbers`, `.soft_wrap`, `.read_only`, `.cursor_line`, `search_bar`, `.vim` (`text_area.rs`, `text_edit.rs`) | ✓ | Already has undo and redo, selection with the mouse, highlighting (syntect, or tree-sitter through rs-rich-lumis), a find bar and Vim mode. Missing: bracket matching, an editor theme (the gutter, the cursor line and the selection as one theme), and highlighting state carried from above the view. Core. |
| `Toast` | `cx.toast`, `cx.notify(Level, ..)`, `App::toasts_at` (`notify.rs`) | ✓ | |
| `ToggleButton` | the base of `checkbox`, `radio`, `switch_toggle` | ✓ | |
| `Tooltip` | `.tooltip(markup)` (`node.rs`) | ✓ | |
| `Tree` | `tree`, `tree_keyed`, `tree_lazy`, `tree_reveal`, `tree_table` (`tree.rs`, `tree_table.rs`) | ✓ | Adds rows that nest inside a table |
| `Welcome` | — | won't-do | A demo splash for Textual's own `textual` command, with no use in an app |

**Counts:** 44 widgets. 28 ✓, 13 partial, 2 ✗, 1 won't-do.

## Framework features

| Area | Textual | intuiTUIve | Status | Gap |
|---|---|---|---|---|
| **TCSS selectors** | type, `#id`, `.class`, descendant, child `>`, `*`, `&` nesting | `Stylesheet` (`sheet.rs`): kind, `#name`, `.class`, descendant only. The parser rejects `>` `+` `~` | partial | Combinators and nesting |
| **Pseudo-classes** | `:focus`, `:focus-within`, `:hover`, `:disabled`, `:blur`, `:can-focus`, `:dark`, `:light`, `:first-child`, `:last-child`, `:odd`, `:even`, `:empty`, `:inline`, … | `:focus`, `:focus-within`, `:hover`, `:selected`, `:disabled`, `:error` | partial | Structural (first, last, odd, even, empty) and theme (dark, light) pseudo-classes |
| **Variables** | `$name: value;` declared in CSS; theme variables | `$name` names a theme style; CSS cannot declare variables | partial | Declared variables |
| **Properties** | about 90 (width, height, margin, offset, opacity, tint, visibility, outline, scrollbar colours, hatch, keyline, layer, …) | about 30: colours, text style, border (titles, sides, collapse), text align, wrap and overflow, size and its limits, padding, gap, grid tracks, display, dock, flex alignment | partial | Margin, offset, visibility, opacity and tint, outline, scrollbar styles |
| **@media / responsive** | horizontal and vertical breakpoints that set classes (`-narrow`); no `@media` | `@media (min-width: N) and …`, `.hide_below`, `viewport()` | ✓ | More than Textual |
| **Live CSS reload** | `textual run --dev` | `App::stylesheet_file`, `App::theme_file` are read again when the file changes | ✓ | |
| **Layout: grid, dock, layers, align, fr** | all | `grid` (tracks, spans), `dock`, `layers`, `Align` and `Justify`, `Size::Flex`/`3fr`, `place` (`Origin`, `Offset`), the `Solver`, `treemap`, draggable `split` | ✓ | |
| **Screens** | push, pop, switch, `ModalScreen`, a result through `dismiss(value)` and a callback or `push_screen_wait` | `cx.push`, `pop`, `replace`, `modal`, `modal_with` (shadow, backdrop); dialogs `confirm`, `prompt` and `pick` return through callbacks | partial | A screen cannot hand a value back in general: there is no `push_for` and `dismiss` |
| **Modes** | `MODES`, `switch_mode`: named screen stacks | `modes`/`in_mode` are binding modes, not screen stacks | ✗ | Named screen stacks |
| **Command palette** | `Provider` classes (`startup`, `search`, `discover`), fuzzy matching with highlights, system commands (Theme, Keys, Maximize, Screenshot, Quit, Bell) | `cx.palette` over the described bindings, `action_overlay`/`cx.action_palette` for actions of your own, live while open, greyed with a reason | partial | A provider trait for searches that run in the background (files, symbols), and the system commands |
| **Bindings and keymaps** | `Binding(id=…)`, `App.set_keymap` remaps by id, `priority`, `show` | `.bind`, `.bind_in`, `.bind_when`, `.bind_unless`, sequences, which-key, `bind_scaled`, `bind_cycle`; `rebind_key` for the editing actions only | partial | Users cannot remap the app's own bindings: bindings have no ids and there is no keymap file |
| **Focus** | Tab chain, `can_focus`, `focus_next` | Tab, focus scopes, by direction, `autofocus`, `focus_within`, `assert_focused` | ✓ | |
| **Mouse** | click, double-click, drag, scroll, hover, enter and leave | `on_click`, `on_mouse`, drag-and-drop, hover styles, wheel, text selection with the mouse captured | ✓ | |
| **Scrolling** | `ScrollableContainer`, smooth scroll, `anchor()` | `scroll`, `scroll_x`, `scroll_both`, `scroll_virtual` (keyed, measured, stand-ins), `scrollbar` | partial | No follow-the-end anchor: a chat that stays at the bottom until the reader scrolls up |
| **Reactive, watch, compute** | `reactive`, `var`, `watch_`, `compute_`, `validate_`, `data_bind`, `recompose` | `signal`, `memo`, `watch`, `on_write`; the nodes that read a signal redraw | ✓ | Finer grained than Textual |
| **Messages and events** | `Message` classes that bubble, `@on(Event, selector)` | closures on the node (`on_key`, `on_click`, `on_select`, …) and signals | won't-do | A different model by design. Signals and closures do what messages do, typed, with no dispatch by class name. |
| **Workers** | `@work` (async or a thread), `exclusive`, cancellation, `WorkerState` | `spawn`, `spawn_future`, `spawn_stream`, `latest`, `resource`, `jobs` (progress, cancel), `Proxy` | ✓ | |
| **Timers, animation, easing** | `set_interval`, `set_timer`, `animate` on any attribute, about 30 easings | `every`, `after`, `fixed_step`, `Timer` (pause, period); `cx.animate(Signal<f64>, ..)`; 4 easings; `fx` presets | partial | Only 4 easings |
| **Themes** | about 20 built-in themes (textual-dark, nord, gruvbox, dracula, tokyo-night, monokai, catppuccin ×4, solarized ×2, rose-pine ×3, flexoki, atom-one ×2, ansi ×2); semantic colours (`$primary`, `$secondary`, `$accent`, `$surface`, `$panel`, `$boost`, `$error`, …) with generated shades (`$primary-lighten-2`, `$text-muted`); switched from the palette | `Theme::dark`, `light`, `mono`; named styles (`accent`, `muted`, `good`, `warn`, `bad`, `selected`, `button.*`); `Ctx::set_theme`; theme files | partial | No semantic palette, no shades, 3 themes, no theme command |
| **Notifications** | `notify(severity, title, timeout)` | `cx.notify(Level, title, body)`, `toast`, placement | ✓ | |
| **Tooltips** | `tooltip` | `.tooltip` | ✓ | |
| **Suspend** | `App.suspend()` | Ctrl+Z suspends; `cx.handoff(Command)`, `cx.edit_text` (`$EDITOR`) | ✓ | |
| **Maximize** | `screen.maximize(widget)`, `allow_maximize`, a system command | build it by hand with `size_with` and `focus_scope_reveal` | ✗ | One call to show the focused pane alone |
| **Clipboard** | `copy_to_clipboard` (OSC 52) | `cx.copy` (OSC 52), selection with the mouse, `take_copies` | ✓ | |
| **Devtools: console, log** | `textual console`: `log` and `print` sent to a second terminal | the `tracing` plugin feeds a `Log` inside the app | partial | Logs sent to a second terminal, which matters most when the app owns the screen |
| **Devtools: DOM and style inspector** | none in the app (`textual console` only) | `App::inspector` / F12: tree, node details (classes, states, rules), timings, trace, pick, redraw flash | ✓ | More than Textual |
| **Testing: Pilot** | `run_test`, `pilot.press`, `click(selector, offset)`, `hover`, `double_click`, `resize_terminal`, `pause`, `wait_for_animation` | `App::driver`: `press`, `type_text`, `send(Event)`, `resize`, `advance`, `settle`, `run_named`, `assert_focused` | partial | Clicks and hovers by node name (today `send` takes raw coordinates) |
| **Snapshot testing** | `pytest-textual-snapshot`: SVG screenshots with an HTML diff report | `assert_screen!` (plain text), `assert_screen_styled!` (markup); unified diff; `UPDATE_SNAPSHOTS=1` | partial | No SVG snapshot or visual report. Text snapshots read better in review, so SVG would be added alongside them. |
| **Screenshots** | `save_screenshot`, `deliver_screenshot`, a system command | `rich record` tapes, from outside the app (rs-rich-record) | partial | No screenshot from inside the app or from a `Driver`, although rs-rich-ext's `frame::Frame::to_svg` exists |
| **Web** | `textual serve`, textual-web | `rich serve` / rs-rich-web: xterm.js or a DOM renderer with ARIA, a session per tab | ✓ | |
| **Accessibility** | limited | roles, states, announcements, linear text mode, screen-reader guide (`a11y.rs`) | ✓ | More than Textual |
| **Markdown streaming** | `Markdown.append`, `Markdown.get_stream` → `MarkdownStream.write` (batches fragments that arrive faster than they can be drawn) | — | ✗ | The top gap for Cornacai |
| **TextArea** | tree-sitter highlighting, `TextAreaTheme`s, undo and redo, selection, soft wrap, read-only, line numbers, bracket matching, tab behaviour | all but bracket matching and editor themes; highlighting through the console's highlighter (syntect, or tree-sitter through rs-rich-lumis); Vim mode | partial | Bracket matching, editor themes, highlighting state from above the view |
| **DataTable** | sorting, cell, row, column and no cursor, fixed rows and columns, zebra stripes, `update_cell`, virtual size | all but fixed rows, row labels and renderable cells | partial | Fixed rows |
| **Inline apps** | `run(inline=True)` | `App::inline(height)`, `print_above` | ✓ | |

**Counts:** 35 areas. 15 ✓, 16 partial, 3 ✗, 1 won't-do.

## Prioritised gaps

Value comes first, and Cornacai's needs (#794) count most: a conversation
view (streaming Markdown, a transcript that follows its end, tool-call
cards, diffs) and a prompt editor (`@` mentions, history, a box that grows
as you type). Effort: **S** a day or two, **M** about a week, **L** more.
Every entry is core unless it says plugin, and each one is generic.

| # | Gap | Value | Effort | Where | Depends on | Issue |
|---|---|---|---|---|---|---|
| 1 | **Streaming Markdown node**: `markdown(signal)` with retained blocks, `MarkdownStream::append` that parses again only the last open block, links you can click and focus, highlighted code blocks | Cornacai's answers | L | core | — | #797 |
| 2 | **A scroll that follows its end**: `scroll(..).follow_end()` and `scroll_virtual(..).follow_end()`, which stay at the bottom as content grows until the reader scrolls up | Cornacai's transcript; logs; chats | S | core | — | #798 |
| 3 | **Diff view node**: rs-rich-ext's `DiffView` as an interactive node with hunks (move between them, fold, accept or reject each, unified or side by side) | Cornacai's edit review | M | core | — | #799 |
| 4 | **Prompt editor features on `text_area`**: `.complete_at(trigger, ..)` (`@` mentions with a fuzzy pop-up), `.history`, `.grow(min, max)` rows, which key submits (Enter submits, Shift+Enter or Alt+Enter adds a line) | Cornacai's prompt | M | core (a files completer is the `files` plugin's) | — | #800 |
| 5 | **A theme system**: semantic colours (primary, secondary, accent, surface, panel, boost, error, warning, success, foreground, background) with generated shades; built-in themes taken from Textual's palettes, with credit; a "Theme" command | Cornacai's themes; every app | M | core | 7 for the command | #801 |
| 6 | **Renderables in a `Log`**: `Log::write(renderable)` (a `Table`, `Syntax`, `Pretty`, a diff), measured to the width, kept with the capacity | Cornacai's tool output | S | core | — | #802 |
| 7 | **Command palette providers and system commands**: a `Provider` trait (`discover`, `search`, run off the app's thread), matches highlighted, and Theme, Keys, Maximize, Screenshot and Quit built in | Cornacai's `/` commands; discoverability | M | core | 5, 10, 13 for their commands | #803 |
| 8 | **Tool-call cards**: `collapsible` with a title that follows state, a status slot (spinner, ✓, ✗) and a one-line summary when closed | Cornacai's tool calls | S | core | — | #804 |
| 9 | **MarkdownViewer**: a table of contents, link-following with back and forward, anchors, `#` jumps | Cornacai's docs and plans; any reader | M | core | 1 | #805 |
| 10 | **User keymaps**: binding ids, `App::keymap(..)` and `App::keymap_file(..)` to remap by id; the help, the palette and `key_hints` show the remapped keys | every app; accessibility | M | core | — | #806 |
| 11 | **Maximize the focused pane**: `.maximizable()`, `cx.maximize()` and `cx.minimize()`, Esc to restore | Cornacai's diff and shell panes | S | core | — | #807 |
| 12 | **Screen modes and screen results**: `App::mode(name, build)`, `cx.switch_mode(name)` (stacks kept apart); `cx.push_for::<T>(build, then)` and `cx.dismiss(value)` | Cornacai's sessions; wizards | S | core | — | #808 |
| 13 | **Screenshots and SVG snapshots**: `Screen::to_svg()`, `cx.screenshot(path)`, `assert_screen_svg!` with an HTML report of what differs | docs, bug reports, visual tests | M | core | — | #809 |
| 14 | **Pilot-style test helpers**: `driver.click("name")`, `double_click`, `hover`, `click_at`, `scroll_on`, `wait_for_animations` | tests | S | core | — | #810 |
| 15 | **Lists of nodes**: `list_of(len, \|i\| Node, selected)`, whose rows are any nodes (cards, rows with controls), with option separators | Cornacai's session list; settings pages | M | core | — | #811 |
| 16 | **Stylesheet coverage**: `>`, `&` nesting, `$var: value;`, structural and theme pseudo-classes, margin, offset, visibility, opacity and tint, outline, scrollbar styles | apps styled by sheets | L | core | — | #812 |
| 17 | **Small widgets**: `link`, `progress_bar` (percentage, ETA, pulse), `header` (title, subtitle, clock), `key_panel` (docked), `masked_input`, the layout `placeholder` | parity; app chrome | M (S each) | core | — | #813 |
| 18 | **Small framework gaps**: Textual's full set of easings, fixed table rows, text-area bracket matching and editor themes, `files::dir_tree` | parity | S each | core; `dir_tree` plugin (`files`) | — | #814 |
| 19 | **A log console in a second terminal**: `INTUITUIVE_CONSOLE` sends `tracing` events and `cx.log` to `rich console` | debugging full-screen apps | S | plugin (`tracing`) and the CLI | — | #815 |

Cornacai needs 1 to 9 and 11, 12 and 15. Items 1, 2, 4 and 8 are its first
milestone ("a conversation view with streaming Markdown; the prompt editor
with `@` files; permission prompts; a diff view"). Permission prompts are
already covered by `cx.confirm` and `pick`.

## Not filed

- **Message classes and `@on` handlers.** intuiTUIve uses typed closures on
  nodes, and signals, not messages dispatched by class. Copying Textual's
  message pump would give two ways to do the same thing.
- **`Welcome`.** This is a demo splash for Textual's own command.
- **Textual's helper commands** (`textual colors`, `keys`, `borders`,
  `easing`). The `rich` CLI and the docs' tapes do this job. A keys tester
  might go into `rich` later. It is not a framework gap.

## Where intuiTUIve goes beyond Textual

To be fair, the comparison runs both ways. These have no Textual
counterpart, or only a smaller one:

- **Pixels, sprites and images:** `pixels` (an RGBA buffer in half blocks,
  quadrants or Braille), `sprite::stage` with collisions, `image` in Kitty,
  Sixel or iTerm2 graphics (`art`), sprite sheets, micro assets, `fx`
  effects (fades, dissolve, sweep, glitch, rain), `big_text`, `treemap`.
- **Audio and visualisation:** PCM in, capture, playback and WAV (`pcm`,
  `audio`, `wav`); `meter`, `spectrogram`, `bar_spectrum`, `persistence`,
  `tuner` (`viz`).
- **Editing:** Vim mode (`vim`), Emacs keys, a find bar with regex, a
  Vim-style `command_line` with completion and history.
- **The loop and timing:** `lockstep` and `stepped_worker` for
  deterministic thread-fed apps, `timedemo` and frame stats, paint
  strategies, synchronized output, held keys and key releases (the kitty
  keyboard protocol), `fixed_step`.
- **Interop and hosting:** a `Driver` for a loop of your own, ratatui
  interop both ways (rs-rich-ratatui, `examples/in_ratatui.rs`), a terminal
  pane and a web view (rs-rich-embed), Python bindings (rs-rich-py).
- **Accessibility:** roles, states, announcements and a linear text mode
  for screen readers.
- **The inspector:** the node tree, rules, timings, an event trace, and a
  redraw flash.
- **Responsive rules:** `@media` in stylesheets, `hide_below`.
- **Components Textual lacks:** menu bars, drop-downs and context menus;
  drag-and-drop; draggable split panes; which-key pop-ups and key
  sequences; Miller columns (`miller_of`); a streaming fuzzy finder
  (`finder_of`); a jobs panel with cancel; calendars, time and date
  pickers; `hex_view`; a query language for filters (`query`); a state and
  frecency store; `scroll_virtual` with measured heights.
