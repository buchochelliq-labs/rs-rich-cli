# intuiTUIve gaps: Slumber

## Round 1: Slumber

[`examples/http.rs`](https://github.com/buchochelliq-labs/rs-rich-cli/blob/main/crates/rich-intuituive/examples/http.rs)
rebuilds [Slumber](https://github.com/LucasPickering/slumber), the
terminal HTTP client, on intuiTUIve with no framework change. It is 2,088
lines (after rustfmt): about 270 read Slumber's YAML collection (through
rs-rich-ext's `data` parser, `yaml` feature) with `$ref` reuse resolved by
hand, 80 are the templates, 280 build a request and send it (a plain
HTTP/1.1 client on `std::net::TcpStream`: chunked bodies, a socket a
cancel can shut, no TLS), 80 the history, 435 the state (40 of them the
focus bookkeeping described below), and 890 the panes, popups and keys.
Its tests ([`tests/http.rs`](https://github.com/buchochelliq-labs/rs-rich-cli/blob/main/crates/rich-intuituive/tests/http.rs),
674 lines, 20 tests) start a small HTTP server on 127.0.0.1 for each test
(a thread a connection, recording what it was sent), write a collection
pointing at it to the temporary directory, and drive the app headless.

Run it with
`cargo run -p rs-rich-intuituive --example http --features rich-ext/yaml,rich-ext/jsonpath,watch -- slumber.yml`.
The example and its test declare `required-features` on rs-rich-ext's
`yaml` and `jsonpath` (a dependency's features, which Cargo accepts there);
`watch` is optional and reloads the collection when its file changes.

What is there, with Slumber's layout, titles, strings and default keys:

- **The sidebar** (30 columns): the recipe tree (`Recipe [r]`, folders
  open down to the first recipe), the profiles (`Profile [p]`) or the
  history (`Request History [ctrl h]`: `HH:MM:SS / 12ms  200 OK`, newest
  first, the row picked shown below); `s` toggles it.
- **The headers**: `Profile [p]` and `Recipe [r]` boxes, the one the
  sidebar lists left out, as Slumber's `ViewState::layout`.
- **The recipe pane** (`Recipe [1]`): `URL │ Query │ Headers │ Body │
  Authentication`, the recipe id at the right. Templates preview with their
  values underlined, an unknown field reversed red. Query and headers are
  key-value tables with Slumber's `▣`/`□` checkbox column: space turns a
  row off, `e` edits a value in a box over the row (the URL in a prompt,
  the body in `$VISUAL`/`$EDITOR` through `cx.handoff_then`, read back),
  `z` resets; edits show in italics and last the session, never written
  to the file. A folder shows its recipes.
- **The exchange pane** (`Request / Response [2]`, `History [ctrl h]` on
  its edge): a status line (`200 OK` green or red, the time, the size;
  `Loading... 0.4s` ticking while in flight), then `Request │ Body │
  Headers`: the request as sent, the body (JSON pretty-printed and
  highlighted by rich's `JSON`), and the response headers. Under the body,
  the query box (`/`): a JSONPath query (`$…`, rs-rich-ext's
  `data::select`, Slumber's older query language) or a shell command fed
  the body (Slumber's current one); empty restores the body. `:` pipes the
  body to a command (its first line in the footer), `v` opens it in
  `$PAGER`.
- **Requests**: Enter sends (a `spawn`ed task), Esc asks `Cancel
  request?` and shuts the socket; build errors (`Unknown field
  `nowhere``), transport errors (`https needs TLS…`) and 4xx/5xx show in
  the pane and the history.
- **Profiles**: `p` lists them and shows the selected one's fields
  (rendered) in the bottom pane; templates follow at once.
- **Layout**: Tab/Shift+Tab and `1`/`2` move between panes, `f`
  fullscreens the selected pane (Esc leaves), `[`/`]` move the split, and
  the divider drags.
- **The actions menu** (`x`): Copy URL, Copy as cURL, Copy Body, Resend,
  Delete Request, Reload Collection. **F5** reloads; **`?`** is the
  framework's help, sectioned as Slumber's (Pane Navigation, Layout
  Control, Interaction, Collection Management, and the focused pane's
  keys). The footer shows the collection path, notices for five seconds,
  and `x actions / ? help / q quit`.

Left out: Slumber's template functions (`response()`, `command()`,
`prompt()`, `select()`, `file()`, `env()`, filters such as `jsonpath()`
and `trim()`) and the prompt form they open; persistence (the SQLite
request history and the UI state saved across runs); TLS, redirects,
cookies, compression; collection switching (F3), import, the CLI and the
Python package; streaming bodies; recipe search (`/` in the list); the
query box's command history (Ctrl+R) and its default command per MIME
type; multipart and file bodies; per-row overrides of a form body.

Nothing in the example is a bespoke widget: every part is a built-in node
(`tree_with`, `list`, `table_with` with `row_rects`, `tabs`, `vsplit`,
`scroll`, `input`, `text`/`renderable`, `switch`, panels) or a framework
popup (`prompt_with`, `pick`, `confirm`, `help`).

### New gaps and bugs

| Gap or bug | Needed for | API sketch or repro | Where |
|---|---|---|---|
| Show or hide a node by a signal | The sidebar (`s`), fullscreen (`f`), the bottom pane that is the exchange or the profile, the tab pages. `class_when` restyles only (its `display` is ignored, by design: layout comes from fixed classes), and `hide_below` follows the viewport. The example puts the whole layout in a `switch` keyed by a `Layout` value, so each layout is built anew and its node state (scroll offsets, a table's offset, the query box's caret) is per layout: fullscreen and back loses the body's scroll | `node.show_when(\|\| cond)` (hidden as by `display: none`: no room, out of the Tab order), or `class_when` honouring `display` | core |
| Focus a node a `switch` has not built yet | `1`, `2`, Tab and `h`/`l` focus the selected pane's page, which a `switch` may only build on the next frame; `cx.focus(id)` needs the id now. The example keeps a `(layout, target) → id` map in a signal, written as pages build, and a watch that focuses a wanted target once it appears (about 40 lines). `cx.after(Duration::ZERO, ..)` runs before that frame, too early | `cx.focus_within(pane_id)` (the pane's remembered or first focusable node, resolved after layout), or `node.focus_scope()` that remembers its last focused descendant; `cx.after_frame(\|cx\| ..)` for the rest | core |
| Which pane holds the focus | Slumber's Tab cycles panes, not every focusable node; nothing says which ancestor holds the focus, so the example keeps a `pane` signal that a mouse click does not update | `focus_within(node_id) -> Memo<bool>`, or `cx.focus_path()` | core |
| A panel's kind of border by focus | Slumber's selected pane is double-bordered; `.focus_border(..)` is for collapsed containers only, and a stylesheet's `:focus-within` cannot change `border` (a layout property). The example keeps the blue focus colour only | `.panel(t).focus_border(Border::Double)` on any panel | core |
| Panel titles that read signals | The recipe id on the recipe pane's right edge; titles are fixed strings, so the id sits at the end of the tab strip's row | `.title_right_with(\|\| markup)` (any `add_title` taking a closure) | core |
| A split with no divider row | Slumber's panes share their borders (`Spacing::Overlap(1)`); `vsplit` puts a rule row between them, one row lost | `split_with(..).divider(Divider::SharedBorder)`, the panels' joined edge as the drag handle | core |
| Edit a table cell in place | Slumber turns the value cell into a text box (`EditableTemplate`); the example opens `prompt_with` over the row at `row_rects`, which covers the row and its neighbours | `TableOptions::editable(col, \|row, text, cx\| ..)` or `cx.edit_cell(table, row, col, ..)` | core |
| A checkbox column on a table | Rows on and off (`▣`/`□`): the example writes the glyphs and binds Space itself; `selection_markers` is for lists | `TableOptions::checks(signal<HashSet<usize>>)`, toggled by Space and a click | core |
| A long read-only text, virtualised | A response body of thousands of lines: `scroll(renderable(..))` lays the whole text out (up to 4,000 rows, the rest cut); a read-only `text_area` is virtual but takes ←/→ and `h`/`l`, which Slumber uses for tabs | `text_view(\|\| text).language("json")`: virtual rows, ↑↓/PgUp/PgDn/Home/End only, selection and copy (Bubbles' `viewport`) | core |
| Disabled tabs | Slumber dims Body and Authentication when the recipe has none | `tabs(..).disabled(\|i\| bool)` | core |
| Help with a header | Slumber's help opens with Version, Docs, Collection, Configuration, Database and Log paths above the keys | `HelpOptions::header(node)` | core |
| `wait_for_tasks` and a task that never ends | A test of Esc-cancel cannot wait for tasks: the request hangs on purpose, and every step would wait for it, so that test runs without `wait_for_tasks` | `spawn_detached(..)`, a task a test does not wait for, or a per-task timeout under `wait_for_tasks` | core |
| An HTTP client | The whole transport: TLS, keep-alive, redirects, cookies, gzip, proxies, timeouts, progress of a large body, a cancel that stops it. The example's `send` is HTTP/1.1 over `std::net` only (about 150 lines) | `http::send(request) -> Task` with a `Signal<Progress { sent, received, total }>` and cancel, on ureq + rustls (both already in Cargo.lock through rs-rich-cli's `fetch`); the generic core under it is `jobs::<P>()` (cancellable work with progress), already there | plugin |
| Templates | Slumber's template language: functions (`response`, `command`, `file`, `env`, `prompt`, `select`), filters (`jsonpath`, `trim`, `base64`), lazy evaluation that asks the user while a request builds. The example reads `{{ field }}` only | a template engine in rs-rich-ext (or a new crate) with a `Prompter` trait; the generic core under the prompts is a modal form asking several values at once, `cx.form(fields, \|answers, cx\| ..)` | plugin (core form) |
| `$ref` references | Slumber reuses maps with `$ref: "#/.base"`; rs-rich-ext reads the YAML (anchors and aliases too) but not JSON references; the example resolves them (40 lines) | `rich_ext::data::resolve_refs(&node) -> Result<Node>` | plugin (rs-rich-ext `data`) |
| Saved history and state | Slumber keeps every exchange in SQLite and restores the selected recipe, profile, pane and tabs; the example forgets on exit | `persisted("key", signal)` in core over a `Store` trait, with a JSON-file store (and SQLite as a plugin) | core (store plugins) |

No bugs were found in the framework's existing behaviour: each row above
is something missing, not something broken.

### Friction with the existing APIs

- **Markup titles eat keys.** `panel("Recipe [r]")` shows `Recipe `: `[r]`
  is a markup tag (rich's `r`, reverse), dropped silently; `[ctrl h]`
  vanished the same way. Slumber's titles all carry a key in brackets.
  The example escapes every title. A `panel_text(..)` taking plain text,
  or a debug-build warning when markup drops a tag that styles nothing,
  would have caught it.
- **`class_when` looked like the way to hide a node.** The guide says
  stylesheets set `display: none`, and `class_when` sets a class; only the
  `class_when` doc says layout ignores it. The first build of the example
  hid nothing. A note under Stylesheets ("conditional classes restyle;
  they do not hide") would save the detour.
- **Ids of nodes inside a `switch`.** A builder can take a node's id
  before handing the node over, but a page is built later than the
  handler that wants it; the registry and its watch are the price. A
  `switch` that takes prebuilt pages (`switch_of(key, [(k, node), ..])`)
  would also do.
- **Signals are `Send`, so tasks can write state directly.** `spawn`'s
  `done` takes the whole `Copy` state struct; no proxy needed. This went
  smoothly, as did `cx.handoff_then` for `$EDITOR` and the driver's
  `take_handoff`/`handed_back`, which let a test edit the file the editor
  was given.
- **`required-features` with a dependency's feature works** for the
  example (`rich-ext/yaml`), but the test needs its own `[[test]]` entry
  with the same list, or `cargo test -p rs-rich-intuituive` builds it
  without the YAML parser.
- **`rich::Text::from_markup` returns a `Result`.** Every renderable
  closure that shows markup needs a fallback; `renderable` taking markup
  (`markup_renderable(|| String)`) or `Text::markup_lossy` would be
  shorter.

### Components that would help

What Slumber leans on, what comparable libraries offer (ratatui's
`tui-textarea` and `tui-tree-widget`, Bubble Tea's Bubbles, Textual), and
what would make the clone shorter or more faithful.

| Component | What it would replace or enable | API sketch | Where | Value |
|---|---|---|---|---|
| An HTTP client plugin | The 150-line `std::net` client; TLS and the rest of HTTP; progress of a download; a cancel that stops the transfer | `intuituive::http::send(req) -> (Task, Signal<Progress>)`, `Request`/`Response` types, over ureq + rustls; core under it: `jobs` (exists) | plugin | high |
| A key-value editor | Query, headers, form fields and profile data: a table with a checkbox column, in-place editing of the value, add and delete rows, edits shown apart from the original and reset with `z` | `kv_editor(rows: Signal<Vec<KvRow>>)` with `KvRow { key, value, enabled, edited }`; generic, built on the table's checks and cell editing above | core | high |
| A virtualised text view | Response bodies of any size, highlighted, scrolled by keys that leave ←/→ to the app (Bubbles' `viewport`, Textual's `TextArea(read_only=True)`) | `text_view(\|\| text).language("json").wrap(false)` | core | high |
| Persisted state | Slumber's `PersistentStore` (the selected recipe, profile, pane, tab, a table's row, all restored) and its request history | `persisted(key, signal)` over a `Store` trait; JSON-file store in core, SQLite as a plugin | core (store plugins) | high |
| Show/hide and focus scopes | The layout `switch` and the focus registry: `show_when` for the sidebar and fullscreen, `focus_scope` for each pane with Tab cycling scopes (Textual's `can_focus_children` containers) | `node.show_when(..)`, `node.focus_scope()`, `cx.focus_scope_next()` | core | high |
| A JSON tree viewer | Folding a large response (Textual's `Tree` over JSON, `tui-tree-widget` demos); rs-rich-ext's `data::Explorer` draws the tree but is not interactive | `data_tree(\|\| node)` on core `tree_with`, copy a path with `y`, filter by JSONPath | plugin (rs-rich-ext `data`; core `tree` under it) | medium |
| A template engine | `{{ }}` with functions and filters, and the prompt form it asks through | `rich_ext::template::render(text, &ctx)` with a `Prompter`; core `cx.form(..)` for the questions | plugin (core form) | medium |
| An input with history | The query and export boxes: ↑/↓ through earlier commands, Ctrl+R to search them, as Slumber's | `input(sig).history(signal<Vec<String>>)` | core | medium |
| A spinner node | The in-flight request (Bubbles' `spinner`); the example ticks a signal every 100 ms to redraw the elapsed time | `spinner(Spinner::Dots)`, animating on the app's clock while shown | core | low |

## Round 2: Slumber on round 6

The clone is rebuilt on round 6. It is 2,026 lines (from 2,088), and is
more faithful: the selected pane is double-bordered, the recipe's id sits
on the recipe pane's top edge, a click selects a pane, the recipe tree
keeps its selection by id across a reload, and the query and export boxes
keep a history. The tests are 710 lines, 22 tests (from 20); all pass.

### What was replaced, and by what

| Hand-built in round 1 | Now | Lines |
|---|---|---|
| The whole layout in a `switch` keyed by a `Layout` value (sidebar, fullscreen pane), each layout built anew with its own node state; `arrange` | one tree built once: `shown_when`/`hidden_when` on the sidebar, the headers and each header box, the top and bottom panes and the profile/exchange pair; `size_with` for the sidebar (30 columns, or all of it in fullscreen); `vsplit` gives the shown pane all of it | −20 |
| The focus registry: `Target`, `Registry` (`(layout, target) → id`, written as pages build), `register`, `register_all`, the `want` signal and the watch that focused a target once its page appeared (~70 lines) | `cx.focus_within(pane_id)`, which waits for a `switch`'s page to be built and laid out; three pane ids | −65 |
| The selected pane not updated by a click | `Node::on_focus` on each pane sets it (new test: a click, then Tab from there) | +3 |
| A blue border only (a panel's kind of border could not follow the focus) | `focus_border(Border::Double)` on every pane and the sidebar, as Slumber's | +2 |
| The recipe id at the end of the tab strip's row (titles were fixed) | `add_title_with(Edge::Top, Align::End, \|\| id)` on the recipe pane; the sidebar's title `panel_with(\|\| ..)` from the list it shows | ±0 |
| The recipe tree by index path (`tree_with`, `HashSet<Vec<usize>>`), a reload keeping the path, the first recipe's folders opened by path | `tree_keyed` with `TreeItem::keyed` (recipe and folder ids), `tree_reveal` to open the folders down to the first recipe; a reload keeps the selection on the same recipe where it moved (new test) | −5 |
| A 10 Hz tick always running, bumping a signal only while a request was in flight | `every` returns a `Timer`: a watch on "loading" `resume`s and `pause`s it | ±0 |
| No history in the query box or the export prompt | `input(..).history("query")`, `PromptOptions::history("export")` (Slumber's query history; Ctrl+R search is still out) | +2 |

Tests changed where the component's behaviour is Slumber's: the focused
pane's border is double (`╔═ Recipe [1] ═`, `╔═ Profile [p] ═`,
`║Other`), and the recipe's id is on the top edge (` list_fish ═╗`,
` create_fish ─╮`) rather than at the end of the tab strip.

### Still hand-built, and why

- **`cycle_pane` and the `pane` signal.** `focus_next_scope` skips hidden
  panes, so in fullscreen Tab could not move to the pane to show next;
  and the pane decides which tab signal ←/→ change. `on_focus` keeps the
  signal in step with clicks now.
- **The key-value tables' checkbox column** (`▣`/`□` written into a cell,
  Space bound by hand): `marker`/`marks_by` draw a styled cell, not a
  glyph, from a `MultiSelect`, where the rows off are per-recipe edits.
- **Editing a value over its row** (`prompt_with` at the row's rect): no
  in-place cell editing.
- **The response body** in `scroll(renderable(..))`: no virtualised
  read-only text view; it also loses its scroll offset when it grows to
  fit (fullscreen) and comes back.
- **Every title escaped** (`Recipe [r]`): markup titles still eat
  bracketed keys.
- The HTTP client, templates, `$ref`s and the history store: the plugin
  gaps of round 1.

### New gaps

| Gap | Needed for | API sketch | Where |
|---|---|---|---|
| A table column of glyphs from state, toggled by a key and a click | Slumber's `▣`/`□` rows | `TableOptions::checks(\|i\| bool, on_toggle)` drawing `checks.on`/`checks.off` glyphs (`▣`/`□`) | core |
| A scroll that keeps its offset across a resize that fits the content | the body after fullscreen and back | `scroll(..).offset(Signal<u16>)` read and set by the app, kept when the content fits | core |
| `focus_next_scope` over panes some of which are hidden by a layout the move itself changes | Tab in fullscreen moving to (and showing) the next pane | `Ctx::focus_next_scope_with(\|scopes\| ..)`, or scopes in a fixed ring including hidden ones, with an `on_focus` that shows them | core |
| Plain-text titles | every Slumber title (`Recipe [r]`, `History [ctrl h]`) | `panel_text(..)`/`title_text(..)`, or a debug warning for a tag that styles nothing (round 1) | core |

### Bugs found

None in this clone; see [tasks](tasks.md) for the focus-then-modal bug,
which an error box here would meet too.

### Components that would help

| Component | What it would replace or enable | API sketch | Where | Value |
|---|---|---|---|---|
| A key-value editor (round 1), on a checks column and in-place cell editing | `key_values`, `toggle_row`, `edit`'s rows and `reset` (~130 lines) | `kv_editor(rows)` | core | high |
| A virtualised text view (round 1) | `body_view`, `request_view` | `text_view(\|\| text).language("json")` | core | high |
| An HTTP client plugin, templates, persisted state (round 1) | the transport, `render`, the history | as in round 1 | plugin | high |
