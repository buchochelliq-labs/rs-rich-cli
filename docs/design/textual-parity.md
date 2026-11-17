# intuiTUIve against Textual: a parity audit

Status: planning note, 2026-10-27. The tables were corrected on
2026-11-17 after an audit of the code (0.0.27's widgets branch): every
row is ✓ but Textual's `Welcome` and its message classes. Each gap had
an issue, labelled `intuituive`, in the [gap list](#prioritised-gaps);
what is left is the long tail of #812.

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
| `Button` | `button`, `button_of`, `ButtonVariant` (`controls.rs`) | ✓ | Five variants are themed, as Textual's: default, primary, danger, success and warning (`button.success`, `button.warning`, in every built-in theme) |
| `Checkbox` | `checkbox`, `checkbox_tri`, `checkbox_of` (`controls.rs`) | ✓ | Has a mixed state, which Textual's lacks |
| `Collapsible` | `collapsible(title, open, child)`, `collapsible_with` (a title that follows state), `.status`, `.summary`, `.symbols`; `accordion(open, sections)`, one open at a time (`collapsible.rs`) | ✓ | |
| `ContentSwitcher` | `switch(key, build)`, `keyed`, `.keep_last(n)` (`builtin.rs`) | ✓ | |
| `DataTable` | `table`, `table_with`, `virtual_table`, `TableOptions` (`widgets.rs`) | ✓ | Sorting, a row cursor, a cell cursor (`.cells`), column highlight, frozen columns and rows (`freeze_rows`, 0.0.24), zebra rows (`row_style`), resizable columns, rows from a closure (so cell updates are signal writes) and virtual rows; row labels (`row_labels`) and cells holding renderables (`renderables`), 0.0.25. |
| `Digits` | `digits` (`digits.rs`) | ✓ | |
| `DirectoryTree` | `files::dir_tree` (`files/dir_tree.rs`), on the core's `tree_lazy`; `files::miller`, `files::file_list`, `files::pick_file` | ✓ | A plugin (`files` feature), by the core rule |
| `Footer` | `key_hints`, `key_hints_with` (`bars.rs`) | ✓ | Built from described bindings, greyed when unavailable |
| `Header` | `header()`, `.clock()`, `.tall()` (`parity.rs`): the app's title and the screen's subtitle | ✓ | |
| `HelpPanel` | `key_panel()` (`parity.rs`), docked, following the focus; `cx.help()` for the modal | ✓ | |
| `Input` | `input`, `number_input`, `.validate`, `.complete`, `.history`, `.mask`, `.max_length` (`input.rs`, `complete.rs`, `text_edit.rs`) | ✓ | Ghost-text completion is the equivalent of Textual's `Suggester` |
| `KeyPanel` | `key_panel()` (`parity.rs`), `cx.toggle_key_panel()` (0.0.24), with the help's sections; shown or hidden by the palette's *Keys* system command (#803) | ✓ | A docked panel following the focus, its bindings under the help's sections |
| `Label` | `label` (`node.rs`) | ✓ | |
| `Link` | `link(text, url)` (`parity.rs`): focusable, Enter or a click opens it; `[@click=action]` and `[link=url]` spans in any `text` or `label` (`spans.rs`: `.on_span`, `.focus_spans`) | ✓ | |
| `ListItem` | a `list_of` row, which is any node (`node_list.rs`); `ListRow` for markup rows (`list.rs`) | ✓ | |
| `ListView` | `list_of(len, \|i\| Node, selected)`, `list_of_virtual` (`node_list.rs`); `list`, `list_rows`, `virtual_list` | ✓ | Rows are any nodes, with separators |
| `LoadingIndicator` | `spinner`, `.loading_when`, `fx::placeholder` (`spinner.rs`, `fx.rs`) | ✓ | |
| `Log` | `Log::new(cap).view()` (`log.rs`) | ✓ | Adds levels, targets, filters and follow, which Textual's lacks |
| `Markdown` | `markdown(signal)`, `MarkdownStream` (`markdown.rs`): retained blocks, links that take clicks and the focus, highlighted code, streaming | ✓ | |
| `MarkdownViewer` | `markdown_viewer(load)`, `MarkdownViewer` (`go`, `back`, `forward`, `toc`, `on_link`), `files::markdown_loader(root)` (`markdown_viewer.rs`) | ✓ | The loader is the app's (the `files` plugin reads files). Anchors are GitHub's slugs of the ATX and setext headings (#805) |
| `MaskedInput` | `masked_input(value, template)` (`parity.rs`) | ✓ | Textual's template characters, `>`/`<` case, `:invalid` while partly filled |
| `OptionList` | `list`, `.multiline()`, `list_search`, `.row_style`, `.separator_after` | ✓ | Separators between options (`separator_after`, on `list` since 0.0.25, as on `list_of`) |
| `Placeholder` | `placeholder(label)`, `PlaceholderVariant` (`parity.rs`) | ✓ | Labelled, in cycling colours; a click cycles its variant |
| `Pretty` | `renderable(Pretty::new(&value))` or `Pretty::from_value` (rs-rich's port) | ✓ | Rust has no reflection, so it pretty-prints `Debug` or a `PrettyValue` |
| `ProgressBar` | `progress_bar(done, total)`, `.show_percentage`, `.show_eta` (`parity.rs`); `seg_gauge`, `jobs_panel` | ✓ | Pulses with no total |
| `RadioButton` | `radio` rows | ✓ | |
| `RadioSet` | `radio`, `radio_keyed` (`controls.rs`) | ✓ | |
| `RichLog` | `Log::write(renderable)`, `write_with` (`log.rs`) | ✓ | Any renderable, measured to the width, kept with the capacity |
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
| `TextArea` | `text_area`, `.language`, `.line_numbers`, `.soft_wrap`, `.read_only`, `.cursor_line`, `search_bar`, `.vim` (`text_area.rs`, `text_edit.rs`) | ✓ | Undo and redo, selection with the mouse, highlighting (syntect, or tree-sitter through rs-rich-lumis) with the state from above the view carried in (0.0.25), a find bar, Vim mode, bracket matching (`.match_brackets`) and editor themes (`EditorTheme`, 0.0.24) |
| `Toast` | `cx.toast`, `cx.notify(Level, ..)`, `cx.notify_with(Notification)` with buttons, `cx.dismiss_toasts`, the `notifications()` history, `App::toasts_at` (`notify.rs`) | ✓ | A click dismisses one; `Level::Success` and the history are more than Textual has |
| `ToggleButton` | the base of `checkbox`, `radio`, `switch_toggle` | ✓ | |
| `Tooltip` | `.tooltip(markup)` (`node.rs`) | ✓ | |
| `Tree` | `tree`, `tree_keyed`, `tree_lazy`, `tree_reveal`, `tree_table` (`tree.rs`, `tree_table.rs`) | ✓ | Adds rows that nest inside a table |
| `Welcome` | — | won't-do | A demo splash for Textual's own `textual` command, with no use in an app |

**Counts:** 44 widgets. 43 ✓, 1 won't-do.

## Framework features

| Area | Textual | intuiTUIve | Status | Gap |
|---|---|---|---|---|
| **TCSS selectors** | type, `#id`, `.class`, descendant, child `>`, `*`, `&` nesting | `Stylesheet` (`sheet.rs`): kind, `#name`, `.class`, `*`, descendant, child `>`, nesting with `&` (read as the flattened rule) | ✓ | Sibling combinators (`+`, `~`), which Textual lacks too (#812) |
| **Pseudo-classes** | `:focus`, `:focus-within`, `:hover`, `:disabled`, `:blur`, `:can-focus`, `:dark`, `:light`, `:first-child`, `:last-child`, `:odd`, `:even`, `:empty`, `:inline`, … | those, `:selected`, `:error` and `:enabled`; `:first-of-type`, `:last-of-type`, `:ansi`, `:nocolor` and `:inline` (0.0.27) | ✓ | States change colours and text styles, not layout (#812) |
| **Variables** | `$name: value;` declared in CSS; theme variables | `$name: value;` declared at the top level (values may use earlier ones); a `$name` not declared names a theme style | ✓ | (#812) |
| **Properties** | about 90 (width, height, margin, offset, opacity, tint, visibility, outline, scrollbar colours, hatch, keyline, layer, …) | about 45: colours, text style, border (titles, sides, collapse), text align, wrap and overflow, size and its limits, padding, margin, visibility, scrollbar colours (hover too) and size, gap, grid tracks, display, dock, flex alignment; offset, opacity, tint, outline, hatch and keyline (0.0.25) | ✓ | Textual's `layer`s and the rest of its long tail; `opacity`, `tint`, `outline` and `hatch` are set by kind, name and fixed classes, not states (#812) |
| **@media / responsive** | horizontal and vertical breakpoints that set classes (`-narrow`); no `@media` | `@media (min-width: N) and …`, `.hide_below`, `viewport()` | ✓ | More than Textual |
| **Live CSS reload** | `textual run --dev` | `App::stylesheet_file`, `App::theme_file` are read again when the file changes | ✓ | |
| **Layout: grid, dock, layers, align, fr** | all | `grid` (tracks, spans), `dock`, `layers`, `Align` and `Justify`, `Size::Flex`/`3fr`, `place` (`Origin`, `Offset`), the `Solver`, `treemap`, draggable `split` | ✓ | |
| **Screens** | push, pop, switch, `ModalScreen`, a result through `dismiss(value)` and a callback or `push_screen_wait` | `cx.push`, `pop`, `replace`, `modal`, `modal_with` (shadow, backdrop); `cx.push_for::<T>(build, then)` and `cx.dismiss(value)`; dialogs `confirm`, `prompt` and `pick` | ✓ | |
| **Modes** | `MODES`, `switch_mode`: named screen stacks | `App::mode(name, build)`, `cx.switch_mode(name)`, the stacks kept apart | ✓ | |
| **Command palette** | `Provider` classes (`startup`, `search`, `discover`), fuzzy matching with highlights, system commands (Theme, Keys, Maximize, Screenshot, Quit; Bell is its docs' example of one of your own) | `cx.palette` over the described bindings; `Provider` (`discover`, `search` into a `HitSink`, `background` for a search off the app's thread, cancelled by the next query) registered with `App::provider`; hits merged by score with the matched characters highlighted; `SystemCommands` (Theme with a `Themes` picker, Keys, Maximize/Minimize, Screenshot, Quit, and Bell, off until turned on), each can be turned off; `cx.palette_of`; `action_overlay`/`cx.action_palette` | ✓ | No `startup` hook (a provider's `new` does its setup) |
| **Bindings and keymaps** | `Binding(id=…)`, `App.set_keymap` remaps by id, `priority`, `show` | `.bind`, `.bind_in`, `.bind_when`, `.bind_unless`, sequences, which-key, `bind_scaled`, `bind_cycle`; binding ids (`bind_id`, `binding_id`) that `App::keymap` and `App::keymap_file` remap, the help, palette and footer showing the keys remapped (`app/keymap.rs`) | ✓ | |
| **Focus** | Tab chain, `can_focus`, `focus_next` | Tab, focus scopes, by direction, `autofocus`, `focus_within`, `assert_focused` | ✓ | |
| **Mouse** | click, double-click, drag, scroll, hover, enter and leave | `on_click`, `on_mouse`, drag-and-drop, hover styles, wheel, text selection with the mouse captured | ✓ | |
| **Scrolling** | `ScrollableContainer`, smooth scroll, `anchor()` | `scroll`, `scroll_x`, `scroll_both`, `scroll_virtual` (keyed, measured, stand-ins), `scrollbar`; `.follow_end()`, at the bottom until the reader scrolls up | ✓ | |
| **Reactive, watch, compute** | `reactive`, `var`, `watch_`, `compute_`, `validate_`, `data_bind`, `recompose` | `signal`, `memo`, `watch`, `on_write`; the nodes that read a signal redraw | ✓ | Finer grained than Textual |
| **Messages and events** | `Message` classes that bubble, `@on(Event, selector)` | closures on the node (`on_key`, `on_click`, `on_select`, …) and signals | won't-do | A different model by design. Signals and closures do what messages do, typed, with no dispatch by class name. |
| **Workers** | `@work` (async or a thread), `exclusive`, cancellation, `WorkerState` | `spawn`, `spawn_future`, `spawn_stream`, `latest`, `resource`, `jobs` (progress, cancel), `Proxy` | ✓ | |
| **Timers, animation, easing** | `set_interval`, `set_timer`, `animate` on any attribute, about 30 easings | `every`, `after`, `fixed_step`, `Timer` (pause, period); `cx.animate(Signal<f64>, ..)`; 34 easings, Textual's set (`Easing::ALL`); `fx` presets | ✓ | |
| **Themes** | about 20 built-in themes (textual-dark, nord, gruvbox, dracula, tokyo-night, monokai, catppuccin ×4, solarized ×2, rose-pine ×3, flexoki, atom-one ×2, ansi ×2); semantic colours (`$primary`, `$secondary`, `$accent`, `$surface`, `$panel`, `$boost`, `$error`, …) with generated shades (`$primary-lighten-2`, `$text-muted`); switched from the palette | `Theme::dark`, `light`, `mono` and Textual's palettes, 21 built in (`Theme::builtin_names`); `Palette` and `Theme::from_palette`, the semantic colours with their shades; named styles; `Ctx::set_theme`; theme files; the palette's *Theme* | ✓ | |
| **Notifications** | `notify(severity, title, timeout)` | `cx.notify(Level, title, body)`, `toast`, placement | ✓ | |
| **Tooltips** | `tooltip` | `.tooltip` | ✓ | |
| **Suspend** | `App.suspend()` | Ctrl+Z suspends; `cx.handoff(Command)`, `cx.edit_text` (`$EDITOR`) | ✓ | |
| **Maximize** | `screen.maximize(widget)`, `allow_maximize`, a system command | `.maximizable()`, `cx.maximize()`, `cx.minimize()`, Esc to restore, the palette's *Maximize* | ✓ | |
| **Clipboard** | `copy_to_clipboard` (OSC 52) | `cx.copy` (OSC 52), selection with the mouse, `take_copies` | ✓ | |
| **Devtools: console, log** | `textual console`: `log` and `print` sent to a second terminal | the `tracing` plugin feeds a `Log` inside the app; `INTUITUIVE_CONSOLE` sends its events and `cx.log` to `rich console` in a second terminal (`log/console.rs`) | ✓ | |
| **Devtools: DOM and style inspector** | none in the app (`textual console` only) | `App::inspector` / F12: tree, node details (classes, states, rules), timings, trace, pick, redraw flash | ✓ | More than Textual |
| **Testing: Pilot** | `run_test`, `pilot.press`, `click(selector, offset)`, `hover`, `double_click`, `resize_terminal`, `pause`, `wait_for_animation` | `App::driver`: `press`, `type_text`, `send(Event)`, `resize`, `advance`, `settle`, `run_named`, `assert_focused`; `click_on(name)`, `click_on_at`, `double_click`, `hover_on`, `click_text` (`app.rs`) | ✓ | |
| **Snapshot testing** | `pytest-textual-snapshot`: SVG screenshots with an HTML diff report | `assert_screen!` (plain text), `assert_screen_styled!` (markup), `assert_screen_svg!` (SVG, with an HTML report of what differs); unified diff; `UPDATE_SNAPSHOTS=1` | ✓ | |
| **Screenshots** | `save_screenshot`, `deliver_screenshot`, a system command | `cx.screenshot(path)`, the palette's *Screenshot*, `App::screenshot_dir`; `rich record` tapes from outside the app | ✓ | |
| **Web** | `textual serve`, textual-web | `rich serve` / rs-rich-web: xterm.js or a DOM renderer with ARIA, a session per tab | ✓ | |
| **Accessibility** | limited | roles, states, announcements, linear text mode, screen-reader guide (`a11y.rs`) | ✓ | More than Textual |
| **Markdown streaming** | `Markdown.append`, `Markdown.get_stream` → `MarkdownStream.write` (batches fragments that arrive faster than they can be drawn) | `markdown(signal)`, `MarkdownStream::append` (parses again only the last open block) | ✓ | |
| **TextArea** | tree-sitter highlighting, `TextAreaTheme`s, undo and redo, selection, soft wrap, read-only, line numbers, bracket matching, tab behaviour | all, bracket matching and editor themes included (0.0.24); highlighting through the console's highlighter (syntect, or tree-sitter through rs-rich-lumis), with the state from above the view carried into it (0.0.25); Vim mode | ✓ | |
| **DataTable** | sorting, cell, row, column and no cursor, fixed rows and columns, zebra stripes, `update_cell`, virtual size | all (fixed rows: `freeze_rows`, 0.0.24; row labels and renderable cells, 0.0.25) | ✓ | |
| **Inline apps** | `run(inline=True)` | `App::inline(height)`, `print_above` | ✓ | |

**Counts:** 35 areas. 34 ✓, 1 won't-do.

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

By 2026-11-17 every entry above is built, as the tables say; #812's long
tail (sibling combinators, `layer`s and the rest of Textual's properties,
states that change layout) is what is left.

### Built in 0.0.27's widgets round

Beyond the gap list, these came from the same audit, all in the core and
generic:

- clickable spans of markup (`[@click=action]`, `.on_span`,
  `.focus_spans`), `Ctx::run_action` and `Ctx::bell`; status-bar
  segments (`seg(..).on_click`) and breadcrumb parts (`.on_select`) that
  take clicks;
- a notice bar (`notice_bar`) that folds to one row, opens, has actions
  and is dismissed;
- `steps(items, status)`, a checklist or a wizard's stepper;
- toasts with buttons (`Notification`, `cx.notify_with`), dismissed by a
  click or `cx.dismiss_toasts`, their history (`notifications()`), and
  `Level::Success`;
- the `Success` and `Warning` button variants and the palette's *Bell*
  (off by default, as in Textual);
- `:first-of-type`, `:last-of-type`, `:ansi`, `:nocolor` and `:inline`;
- chips over rs-rich-ext's badges (`chip`, `chip_of`, `.on_remove`) and a
  `tag_input`;
- an `accordion` of collapsibles;
- a `pager` strip and a `range_slider`.

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
  frecency store; `scroll_virtual` with measured heights; a `dial` (a
  value on an arc, with a `<meter>`'s bands), `rating` and `rating_input`
  (stars), and a paged `carousel`.
