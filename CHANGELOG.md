# Changelog

Notable changes to the port. Crate versions are independent SemVer and do **not**
mirror the upstream release — which upstream version is tracked lives in
[`UPSTREAM.toml`](UPSTREAM.toml). Entries note which upstream release was
absorbed and what our own crates did.

The format loosely follows [Keep a Changelog](https://keepachangelog.com/).

## [Unreleased]

## 0.0.21 cohort — to be published

To be published from `main` by one `release.yml` run (tag
`release-0.0.21`): core 0.0.11 / macros 0.0.5 / plugin API 0.0.5 /
art 0.0.14 / diagram 0.0.4 / mermaid 0.0.7 / lumis 0.0.5 / ext 0.0.18 /
micro 0.0.7 / record 0.0.9 / interact 0.0.9 / ratatui 0.0.6 /
intuituive 0.0.6 / embed 0.0.4 / web 0.0.4 / data 0.0.5 / CLI 0.0.21, and
`rs-rich` 0.0.9 on PyPI (`python-v0.0.9`, same commit). See
[the release notes](docs/releases/0.0.21.md).

### Python: this cohort's Rust APIs, and rs-rich's identity on the Python site (rs-rich on PyPI)

- `rs_rich.ext.markdown_export`: GitHub-flavoured Markdown from the Python
  renderables (`to_gfm`, `GfmExporter`, `GfmDocument`, `GfmOptions`,
  `PanelStyle`, `fit_github_comment`, `truncate_markdown`,
  `GITHUB_COMMENT_LIMIT`). `Table`, `Tree`, `Panel`, `Rule`, `Text`, markup,
  `Syntax` and `Markdown` are read as the Rust crate reads them; a `Group`
  writes each item as its own block.
- `rs_rich.ext.sql`: the SQL highlighter without syntect (`SqlHighlighter`,
  with an optional fallback, `highlight`, `tokenize`, `token_style`,
  `is_sql`). `highlighter="sql"` picks it for `Syntax` and `Markdown`, and
  `rs_rich.plugins.SqlPlugin` registers it with a plugin host. The wheel
  builds rs-rich-ext with `sql`.
- Plain-text cells: `Table.add_plain_row`, `add_plain_rows`,
  `add_plain_column` and `Tree.add_plain` take data literally, never as
  markup.
- `Table.columns` and `Table.rows`, as in Rich: read-only `Column` and
  `Row` copies (`rs_rich.table.Column`, `rs_rich.table.Row`), compared with
  Rich 15.0.0.
- `rs_rich.mermaid` parses and draws sequence and ER diagrams
  (`parse_sequence`, `draw_sequence`, `parse_er`, `draw_er`, with
  `SequenceDiagram`, `EntityDiagram` and their parts); `parse` and `draw`
  pick the kind by the header, as `Mermaid` does. `parse_flowchart` given a
  sequence or ER diagram names the parser to use, and `MermaidDiagram` has
  the drawing's `notes`.
- `rs_rich.diagram`: edges' `start_label` and `end_label`, in
  `Graph.edge`, `Graph.link` and `DiagramEdge`.
- `Form.multi` and `Form.file` in `rs_rich.interact`: their answers are a
  `list` of `str` and a `pathlib.Path` (a path was a `str`).
- The Python site has rs-rich's mark, favicon and gold accent, and the hero
  banner on its home page, with pip's install line
  (`scripts/gen_rs_rich_hero.py` writes it and copies the mark into
  `docs/python/assets/`). New pages and sections for all of the above; the
  Mermaid page's "not drawn as text" example is a class diagram now.

### Docs: rs-rich's own identity, and intuiTUIve as a sub-site

- rs-rich has a mark (a styled "r" in a terminal tile, with a gold span and
  a gold cell), a favicon, wordmarks with "rich" reversed out of a gold span,
  27 line icons for the site's sections, a preview sheet and a hero banner
  (`scripts/gen_rs_rich_hero.py`), under `docs/assets/rs-rich/`. They share
  intuiTUIve's ink and shapes, with gold (`#c08600`) where intuiTUIve has
  purple, so the two read as parent and child.
- The site uses the mark as its logo and favicon, and gold as its accent;
  intuiTUIve's pages keep its purple.
- The home page and the README open with the hero; the home page links to
  intuiTUIve in a card of its own and ends with every section and its icon.
  The guide's front page lists each crate's section with its icon, and each
  crate's page shows its icon by its title.
- intuiTUIve is a top-level section of the site, after the Guide, with its
  overview as the section's page. Its pages keep their URLs.

### rs-rich-mermaid, rs-rich-diagram, rs-rich-cli: sequence and ER diagrams drawn as text

- **Sequence diagrams** (`sequenceDiagram`) are drawn as text, no longer
  shown as source: participants and actors (`as` aliases) heading their
  lifelines; every arrow (`->>`, `-->>`, `->`, `-->`, `-x`, `--x`, `-)`,
  `--)`, `<<->>`, `<<-->>`) with its line and tip; self-messages; notes left
  of, right of and over one or two participants; activation bars
  (`activate`/`deactivate` and `+`/`-`); `loop`, `alt`/`else`, `opt`,
  `par`/`and`, `critical`/`option`, `break` and `rect` frames with their
  labels; `autonumber` and `title`. Columns are sized to their text, so
  nothing overlaps; a diagram wider than the width is cropped with a note.
  `rich_mermaid::sequence` parses and draws them.
- **ER diagrams** (`erDiagram`) are drawn through the layered layout:
  entities as table boxes (type, name, `PK`/`FK`/`UK`, comment), solid
  identifying and dotted non-identifying relationships with their labels,
  and the cardinality at each end as text (`1`, `0..1`, `0..N`, `1..N`),
  symbols and word forms alike; `direction` and a front-matter title.
  `rich_mermaid::er` parses and draws them.
- `box`, `create`, `destroy` and participant types are drawn simplified, with
  a note; other syntax in either kind falls back to the source under a note
  naming it. Without Sixel graphics, the `mmdc` backend draws both as text,
  as it does flowcharts.
- **Texts at an edge's ends** in rs-rich-diagram: `Edge::start_label` and
  `end_label` (the `Graph::end_labels` builder call) are drawn where the line
  leaves and reaches a node, on the line left-right and beside it top-down,
  clear of the other lines. DOT's `taillabel` and `headlabel` map to them.
  Graphs without them draw as before.
- `rich mermaid` and ```` ```mermaid ```` fences pick both kinds up; the help
  and the CLI reference say so.
- Tests: parser tests for each construct, Unicode and ASCII snapshots,
  cropping, fallbacks, random sources (never wider than asked, never a
  panic) and bounded work on the largest sources.

### Builders are `#[must_use]` (rs-rich-ext, rs-rich-interact, rs-rich-intuituive, rs-rich-art, rs-rich-diagram, rs-rich-mermaid, rs-rich-micro, rs-rich-data, rs-rich-record, rs-rich-web, rs-rich-embed, rs-rich-ratatui, rs-rich-plugin-api)

- **A dropped builder now warns.** A builder method takes `self` and
  returns `Self`, so `node.on_key(..);` on its own line moved the node
  away and did nothing. 184 types that are only useful once used
  (intuiTUIve's `Node`, its `*Options` structs and widgets, rs-rich-ext's
  charts and views, rs-rich-interact's components, `Graph`, the art
  renderables, `RichWidget`) are `#[must_use]`, and so are 423 setters on
  other types.
- No breaking change: code that uses its builders compiles as before;
  code that drops one gets an `unused_must_use` warning.
- Setters on `Copy` handles that work through shared state
  (`Transport::tick`, `Timer::max_catch_up`, `MultiSelect::scoped`,
  `Resource::stale_after`, …) warn too, for one rule everywhere; they still
  make their change when the result is dropped, so `let _ =` keeps the old
  behaviour without the warning.
- Every crate but `rs-rich` (the mirror is unchanged) turns on
  `clippy::return_self_not_must_use`, so new builders stay covered; the
  rule is in `CONTRIBUTING.md`, "API conventions".

### rs-rich: `Syntax` and `Markdown` without syntect (feature `code`)

- **`code`**, a new feature with no dependencies: `Syntax`, the
  `CodeHighlighter` plumbing and everything `Markdown` needs. `syntax` is
  now `code` plus syntect, `markdown` implies `code` instead of `syntax`,
  and `syntax-cache` and `onig` stay on `syntax`. The default features, and
  the default output, are unchanged; the golden tests pass as they were.
- With `code` but not `syntax`, a `Syntax` or Markdown code block
  highlights with its own highlighter or the console's default (which
  `rs-rich-ext`'s registry installs), and with neither renders as plain
  code in the same layout. `SyntectHighlighter` stays behind `syntax`.
  Recorded as [Divergences #39](docs/DIVERGENCES.md).
- CI builds, lints and tests `--no-default-features --features
  code,markdown`, and checks that `rs-rich-ext` with `sql,markdown` and no
  default features carries no syntect or bincode.

### rs-rich: read-only accessors for tables, panels, rules and Markdown

- Getters that mirror upstream's public attributes and change nothing:
  `Table::columns` and `Table::rows` (the now-public `table::Column`, with
  `header`, `footer`, `justify`, `style`, `header_style` and
  `footer_style`, and `table::Row`, with `cells`, `style` and
  `end_section`), `Table::get_title`, `get_caption`, `get_show_header`,
  `get_show_footer`, `get_show_lines`, `get_box`, `get_header_style` and
  `get_footer_style`; `Panel::renderable`, `get_title`, `get_subtitle`,
  `get_title_align` and `get_subtitle_align`; `Rule::get_title` and
  `get_align`; `Markdown::markup`. A `get_` prefix where the builder
  already has the name, as `Tree::get_style`. Output is unchanged.

### rs-rich-ext: Markdown for GitHub

- **`markdown_export`** writes renderables as GitHub-flavoured Markdown, for
  pull request comments and job summaries. `GfmDocument` collects blocks;
  `ToGfm` is implemented for `Table` (a GFM table: header, an alignment row
  from each column's justify, `|` escaped, cell styles as `**`, `*`, `~~`
  and links), `Tree` (nested lists), `Panel` (a blockquote, or
  `<details><summary>` with `PanelStyle::Details`), `Rule` (`---`), `Text`
  and markup (inline Markdown, code from `code`-named styles), `Syntax` (a
  fence with its language) and groups. Anything else, `Columns` included,
  is its plain rendering in a ```` ```text ```` fence at a set width.
- Exact: tables, panels, rules and Markdown are read through core's new
  read-only accessors, not their rendering, so every table is written as
  it is (a grid, a table without edges, a cell of several lines as
  `<br>`), styles named `code` survive in table cells, and a core
  `Markdown` is written as its source. Only what exposes nothing (a
  panel's non-text body, `Columns`) is rendered and read back, or plain.
- `fit_github_comment` and `truncate_markdown` cut Markdown to GitHub's
  65 536-character comment limit at a line break, close an open fence or
  `<details>` block, and say how much was kept.
- Guide: [Markdown for GitHub](docs/guide/ext/markdown-export.md).

### rs-rich-ext: plain-text table cells

- **`table::PlainRows`**, an extension trait for the core `Table`:
  `add_plain_row`, `add_plain_rows`, `add_plain_column`,
  `add_plain_column_justify` and `plain_column_footer` take cells as plain
  text, so `[bold]x[/]`, a `[` in a file name or `:smile:` stay as written.
  The core `add_row` still reads a `&str` as markup, as upstream does.
- **`table::PlainTree::add_plain`** does the same for tree labels.
- Guide: [Plain-text cells](docs/guide/ext/tables.md#plain-text-cells).

### rs-rich-ext: a SQL highlighter without syntect

- **`sql::SqlHighlighter`** (feature `sql`, off by default, no new
  dependencies): a `CodeHighlighter` on a hand-written lexer for SQL,
  answering to `sql` and the aliases `postgresql`, `mysql`, `sqlite`,
  `bigquery`, `snowflake`, `duckdb` and others. Keywords (with common
  dialect words), types, constants, functions, names, quoted names
  (`""`, backticks, `[]`), strings (`''` escapes, `E''`, dollar quotes,
  triple quotes), numbers, nested comments, parameters, operators and Jinja
  blocks (`{{ }}`, `{% %}`, `{# #}`) for dbt, in `monokai`, `ansi_dark`
  and `ansi_light`. `with_fallback` hands other languages to another
  highlighter; `SqlPlugin` registers it as `"sql"`.
- `sql::highlight` returns highlighted SQL as a `Text`, for builds without
  `Syntax`; `sql::tokenize` returns the tokens.
- `rs-rich-ext` forwards core's new `code` feature: `code` is new,
  `syntax` is `code` plus syntect, and `markdown` now needs only `code`
  (it used to turn on `syntax`, so syntect). `sql` turns on `code`. The
  default features are unchanged.
- With `default-features = false, features = ["sql", "markdown"]` on
  `rs-rich-ext` alone, a `Syntax` and a Markdown ```` ```sql ```` fence are
  highlighted by `SqlHighlighter` from the registry with no syntect in the
  build (the `sql_without_syntect` test, which CI runs that way).
- Tests: token classes on tricky input, unterminated strings and comments,
  and 3000 random inputs that must tokenize and highlight without a panic;
  a `Syntax` block and a Markdown ```` ```sql ```` fence highlighted through
  the console. CI builds the feature on its own.
- Guide: [SQL without syntect](docs/guide/ext/code-highlighters.md#sql-without-syntect).

### rs-rich-embed: Chrome's DevTools over a pipe on Linux and macOS

- **No DevTools port on Unix.** `ChromeEngine` starts Chrome with
  `--remote-debugging-pipe` on Linux and macOS and speaks the protocol over
  its file descriptors 3 and 4 (NUL-ended JSON), so no other program or
  user on the machine can reach it. The descriptors are passed with
  `command-fds` (pinned to 0.3.3, Apache-2.0), and the crate keeps
  `unsafe_code = "deny"`. Windows keeps the `127.0.0.1` port, still a
  [known limit](docs/known-issues.md#chromes-devtools-port-on-windows-in-rs-rich-embed).
- One protocol path over either transport; a browser that exits early now
  reports its last line of standard error.
- `--remote-debugging-…` switches passed in `.args` are dropped, as sandbox
  switches are: the engine chooses where DevTools goes.
- Tests: on Linux, the browser and every process it started listen on no
  TCP socket while a view is open; frames arrive over the pipe and over the
  port.

### intuiTUIve: a logo and topic icons (docs)

- intuiTUIve has a mark (a node tree drawn with box-drawing guides, one
  cell lit: the one a signal redraws), a favicon-sized mark, a wordmark
  with TUI reversed out like a selection, and a line icon for each guide
  page, under `docs/assets/intuituive/`. The overview page shows the
  wordmark and an icon beside each page; the crate README shows the mark.
- A widget index (`guide/intuituive/widget-index.md`): every widget and
  component, 101 in eight groups, each with an icon, its constructor, a line
  on what it is and a link to the section that documents it.
- A mascot: Memo, a purple elephant with block-cursor eyes who holds up the
  mark's lit cell, in five poses, at 32 and 16 px, and as a 32x32 pixel
  sprite for half blocks; and her calf Tui, alone and holding Memo's tail
  (`docs/assets/intuituive/mascot/`). The overview page shows Memo waving.

### Versions: the 0.0.21 versions are open

- Every package moves one patch past the 0.0.20 cohort, as the
  [0.0.21 plan](docs/plans/0.0.21.md) lists: core 0.0.11, macros 0.0.5,
  plugin API 0.0.5, art 0.0.14, diagram 0.0.4, Mermaid 0.0.7, lumis 0.0.5,
  ext 0.0.18, micro 0.0.7, record 0.0.9, interact 0.0.9, ratatui 0.0.6,
  intuituive 0.0.6, embed 0.0.4, web 0.0.4, data 0.0.5, the CLI 0.0.21 and
  `rs-rich` 0.0.9 on PyPI. Parity work changes core, and Cargo's 0.0.x
  requirements are exact, so every crate that depends on it moves too.
  `RELEASES.toml`, both lockfiles, the version tables and the CLI reference
  follow.

### intuiTUIve: stock form controls (rs-rich-intuituive, rs-rich-web, rs-rich on PyPI)

- `button(label, on_press)`: Enter, Space or a click (a press let go over
  it) presses it; `.variant(ButtonVariant::Primary | Danger)` picks its
  face from the theme's `button`, `button.primary` or `button.danger`
  (the dark, light and mono themes name all three); it looks picked under
  the focus or the pointer, pressed while the mouse holds it, greyed out
  with `disabled_when`, and three rows tall draws lit and shaded edges in
  colours a theme names.
- `checkbox(label, Signal<bool>)`, `switch_toggle(label, Signal<bool>)`,
  `radio(options, Signal<usize>)` (the arrows move the choice within the
  group), `slider(Signal<f64>, range, step)` (the arrows, PageUp/PageDown,
  Home/End, a click, a drag and the wheel), `select_field(options,
  Signal<usize>)` (a menu under it on `open_menu`'s list, opened at the
  choice; letters jump to the option they start, closed or open) and
  `collapsible(title, Signal<bool>, child)`.
- `spinner(name)`: rich's named spinners as a node, turned by an `after`
  timer only while it spins (`.spinning_when(..)`), hidden from screen
  readers. `.loading_when(..)` dims any node under a spinner and marks it
  busy.
- Each has a role and states for assistive technology: new
  `Role::Slider`, `Role::RadioGroup` and `Role::ComboBox`;
  `Widget::access_value` for a value the cells do not say (a slider's
  number, sent to a browser as `aria-valuenow`); `DrawCx::disabled`. The
  DOM renderer gives a radio group's chosen item the `radio` role, and
  `rs_rich.tui.Role` names the three new roles.
- Each draws in theme styles with fallbacks (`checkbox.on`, `switch.on`,
  `slider.filled`, `control.focus`, …), so a theme file or
  `.theme_styles(..)` restyles it. A select field's menu, and only its,
  takes letters rather than j and k.
- `examples/controls.rs` is a settings form of all of them, and the guide
  has a page for them (Buttons and form controls). The `access` example's
  check box, switch, buttons and spinner, the widget gallery's three
  hand-drawn buttons and `checked_when`'s doc example use them now.

### Logs: the app's own `tracing` events in an intuiTUIve pane (rs-rich-ext, rs-rich-interact, rs-rich-intuituive)

- rs-rich-ext: `Record::from_event` reads a `StructuredEvent` (from
  `EventLayer` or `LogAdapter`) as a `log_stream::Record`, the reverse of
  `to_event`. `ChannelSink` is an `EventSink` over a bounded channel: a
  full channel drops and counts the record, so the thread that logs never
  waits, and `notify` wakes the reader. `adapters` (`EventSink`) is now in
  every build; `LogAdapter` and `EventLayer` still need their features.
- rs-rich-interact: `LogEvent: From<Record>`, so a
  `ChannelSink::<LogEvent>` feeds `LogView::source`.
- intuiTUIve: `Log` keeps a scrollback (`set_capacity`) and follows the
  newest line until it is scrolled back (keys while focused, the wheel,
  the scrollbar); End follows again, and `unseen()` counts what arrived
  below. `LogLine` gives a line a level and a target, drawn in rich's
  `logging.level.*` styles and `log.target`. `set_level`,
  `set_target_level` (a target covers the ones under it) and `set_filter`
  (a query: words, `level:`, `target:`) filter what shows;
  `targets_view()` is a tui-logger-style panel of targets and their
  levels. A log reads as its newest line in view to assistive technology.
  Its existing API is unchanged, and a following, unfiltered log still
  draws only what arrived.
- intuiTUIve: the off-by-default `tracing` plugin, `log::tracing_layer`,
  `install_tracing` and `TracingFeed`, feeds a `Log` from `tracing`
  through `EventLayer` and a `ChannelSink`, with one line for what a full
  channel dropped. The new `tracing` example rebuilds ratatui's, with the
  log in a pane, a targets panel, a filter box and a status line.

### Charts: pies, stacked bars and areas, QR codes and gradient text (rs-rich-ext, rs-rich-cli)

- `chart::PieChart`: shares of a whole as slices of a disc, clockwise from
  the top, with a legend of labels, percentages and (optionally) values;
  `.donut(hole)` makes a ring with text in the hole. Half blocks with
  colour, a fill per slice without it, Braille with the edges between
  slices left out, and an ASCII form; accessible text names each share.
- `BarGroup::stacked()`: a group drawn as one bar of parts, across or up,
  parts below zero stacking down, with the total written and a legend
  (`BarChart::legend`). Without colour each part has its own fill.
- `Chart::stacked(true)`: area datasets drawn on top of the ones before,
  the axes fitting the totals.
- `qr::QrCode`, behind the new off-by-default `qr` feature: QR codes drawn
  with half blocks inside a quiet zone, at an `ErrorCorrection` level, in
  exact black and white with colour and in the terminal's own colours
  without (`.invert` for light terminals); two cells a module in ASCII. The
  quiet zone narrows to fit, and a code that cannot fit says how many
  columns it needs. New dependency: `qrcodegen` 1.8 (MIT, no dependencies).
- `gradient::GradientText`: a `Text` coloured along two or more colours
  through `palette::gradient`, across, down or corner to corner, its
  colours the nearest each colour system has.
- `rich chart --kind pie` (one series as slices) and `--kind stacked` (a
  bar per row, each series a part).
- The guide's [Charts](docs/guide/ext/charts.md) page covers them, from the
  new `guide_charts_extra` example.

### intuiTUIve: effect presets (rs-rich-intuituive)

Ready-made effects over any node, in a new generic core module `fx`,
built only on `effect`, the app's clock and `Easing` (tachyonfx is the
reference design; it is not a dependency, as it would bring ratatui into
the workspace):

- **Presets:** `fade_in` / `fade_out` (each cell to and from the
  background colour), `dissolve` / `coalesce`, `sweep` (a soft edge
  moving in a `Direction`), `slide_in`, `hsl_shift`, `glitch`, `rain`
  (digital rain), `shimmer` (a band of light across text) and `skeleton`
  (what a node shows as pulsing bars, for loading); `placeholder(widths)`
  is bars of their own, read as "Loading". `Fx::new` and `Fx::endless`
  make your own.
- **Composition:** `fx::sequence`, `fx::parallel`, `fx::wait`, and
  `delay`, `hold`, `repeat`, `forever`, `ping_pong`, `reversed`, `limit`
  and `easing` on an `Fx`.
- **Playing:** `fx::play` (once, when the node first draws), `play_when`
  (each time a condition turns true), `play_while` (while one holds: a
  skeleton while loading) and `transition` (forwards as a node shows,
  backwards as it goes, hidden at the end); `fx::splash(cover, hold, app)`
  fades a cover (an `image`, big text) into the app.
- A running effect asks for a frame every `fx::FRAME`; once over it asks
  for none, and its end stays. In accessible and linear mode an effect
  goes straight to its end, as `Ctx::animate` does.
- Colours from the theme: the `fx` style (the colour faded to), and
  `fx.shimmer`, `fx.rain` and `fx.skeleton`. The `fx` example is a
  gallery of them all.

### Text editing: rebindable keys, Emacs and Vim presets, search, word wrap (rs-rich-interact, rs-rich-intuituive)

- `edit::Buffer` gains a kill ring (`kill_to_end`, `kill_word`, `yank`,
  `yank_pop`, joined consecutive kills), a mark (`set_mark`), paragraph
  moves, `delete_next_word`, search (`Search::plain`, `Search::regex` with
  fancy-regex, `ignore_case`; `match_count`, `select_match`), an undo limit
  (`set_history_limit`, 1000 by default), tab width and hard tabs, and
  `wrap_line` with `WrapMode::Word`, `Grapheme` and `None`.
- New `edit_keys`: the editors' keys as a keymap of named actions
  (`editor_keymap`, `emacs_keymap`, `resolve`, `perform`) and the `KeyMode`
  hook for modal presets. New `caret::CaretShape` for DECSCUSR.
- New off-by-default `vim` feature (rs-rich-interact and rs-rich-intuituive):
  `vim::Vim`, with normal, insert, visual and visual line modes, motions,
  operators, counts and `.`.
- rs-rich-interact's `TextArea` runs the buffer's new actions and gains
  `emacs_keys()`.
- intuiTUIve's `text_area` and `input` gain `rebind_key`, `edit_keymap`,
  `emacs_keys`, `key_mode`, `vim` and `vim_inserting`, `mode_status`,
  `search(TextSearch)` with match highlighting and the `search_bar` find
  bar, `wrap_mode`, `cursor_line`, `caret_shape` (the app sends DECSCUSR
  and puts the user's shape back) and `draw_caret`, `tab_width` and
  `hard_tabs`, `undo_limit`, and `validate` (a message shown and given to
  assistive technology). `Node::error_message` and `AccessNode::error` carry
  a message for any node. Editors without these options behave as before.
- A `textarea` example (ratatui-textarea's examples as a gallery) and the
  guide pages [Editing text](docs/guide/intuituive/text-editing.md) and
  [The text buffer and editor keys](docs/guide/interact/editing.md).

### Recording: a held key repeats only while it is held (rs-rich-record)

- `Hold` (and a `KeyDown` held through a `Sleep`) sends the repeats due
  before the key's release and none after: a late wake-up of the recorder
  sent the ones due in its sleep, so a slow machine (macOS CI) recorded
  more repeats than the tape asked for. The count is now exact on every
  machine, and the test asserts it.

### Docs: a playground in the docs (tier 1)

- Guide and tutorial blocks whose input is text (console markup, CSV,
  JSON, Markdown, code, chart values, diffs, data documents) have a
  **Try it** button: rs-rich, compiled to wasm, renders the edited input in
  the reader's tab, with the Rust that renders it beside the block. A new
  [Playground](docs/playground.md) page has every kind at full size. Nothing
  loads until the first click; the lean module (no syntect) serves most
  blocks, and the full one loads only for `syntax` and `markdown`. Output is
  rs-rich's own HTML export in a sandboxed frame, or xterm.js on request.
- **Copy link** puts a block's input and options in the link's fragment,
  compressed and size-capped; opening it restores them as text.
- The modules come from `playground/` (not published, outside the
  workspace). The `docs` workflow builds main's and renders every playground
  block with them under Node, so a block that stops rendering fails the docs
  build; a browser check covers the page. The release workflow attaches
  `playground-X.Y.Z.tar.gz` to the release, and the versioned docs download
  it; a release without one keeps plain code blocks.

### Docs: the CLI in the playground (tier 2; rs-rich-cli)

- CLI examples in the docs have a **Try it** button: it opens a terminal
  (xterm.js) on the page with the block's commands filled in at the
  prompt, run by `rich` compiled for WASI (`wasm32-wasip1`, about 2.5 MB
  gzipped, loaded on the first click). The prompt is a small line runner:
  pipes, `;`, `&&`, `||`, redirects, variables, globs and a few builtins
  (`echo`, `cat`, `ls`, `seq`, …) over sample files. **Stop** ends a
  runaway command; **Save as tape** downloads the session as a tape that
  `rich record` replays.
- 127 CLI examples on 16 pages run; the rest (URLs, `--watch`, `capture`,
  `record`, `rich deps`, `git` and `jq`) stay plain code. The docs check
  runs every marked block under Node and fails on an example that runs but
  is not marked.
- rs-rich-cli builds for `wasm32-wasip1` with `art,mermaid`: host-only
  dependencies are gated; under WASI `--demo` and `rich spin` exit 2 as not
  available and `rich log` reads stdin without a thread. Native builds are
  unchanged. CI checks the WASI build.

### Docs: apps in the playground (tier 3; rs-rich-interact, rs-rich-intuituive, rs-rich-web)

- The [Playground](docs/playground.md) page runs `rich choose` (and
  `--multi`), `rich filter`, `rich pager` and `rich explore`, and
  intuiTUIve's to-do, form, meters and sprites examples, in xterm.js with
  the keyboard and the mouse; the intuiTUIve tutorial runs its to-do app
  beside the Rust. A third module (`app`, no syntect, about 1 MB gzipped)
  loads only on **Run it**, in a Web Worker that **Stop** ends. The docs
  checks start every app block under Node, and drive them with keys and
  clicks in Chromium.
- rs-rich-interact: `EventLoop::step`, `pump`, `timeout`, `is_running`
  and `finish`, a push API beside `run` for a host that cannot block (a
  browser tab, another program's loop), with the same outcomes and bytes
  as `run` on the same input; `PushBackend` and `PushPort` are a backend
  for it whose clock and screen the host owns.
- rs-rich-interact and rs-rich-intuituive: a `syntax` feature, on by
  default, so nothing changes for an existing build; off, syntect is left
  out. Both build for `wasm32-unknown-unknown`, checked in CI. On wasm32,
  intuiTUIve keeps short spans (double clicks, throttles, the inspector's
  timings) on the app's clock, runs `spawn`, `spawn_stream` and the
  `files` plugin's work on the app's thread, and keeps `latest_every` to
  an `every` timer; `latest` and a meter's peak hold are not available
  there.
- rs-rich-web: a `server` feature, on by default with `syntax`, around
  everything that serves (`serve`, `serve_command`, `Server`, the page,
  the session processes). Without it the crate is `input` (the page's key
  and mouse decoder) and `dom`, with no networking or PTYs, for wasm32.

### Docs: one site per release, with a version selector

- The documentation site keeps a copy of the docs for each release, picked
  from a drop-down in the header: `/X.Y.Z/` is built from the last commit on
  main that still ships that release (its `rs-rich-cli-vX.Y.Z` tag, or a later
  one up to the commit before `RELEASES.toml` first moves a crate past it, so
  docs fixed after a release show under it), `/dev/` from main, and the
  site's root and
  `/latest/` go to the newest release. Every other version shows a banner
  linking to the newest release's docs, and an old unversioned link is sent
  to the same page under the newest release.
  `scripts/build_versioned_docs.py` builds it in the `docs` workflow, keeping
  the newest releases that fit under GitHub Pages' 1 GB; a release tag's push
  re-runs the workflow on main, so a release's docs go up when it is tagged.
  `docs-versions.toml` pins a release's docs to a commit: the commit that
  opens the next versions records the last commit of the release's notes
  there (`docs/BRANCHING.md`).

### intuiTUIve: round 7's four bugs (rs-rich-intuituive)

- **The focus after a modal.** A `cx.focus(x)` made just before
  `cx.modal(..)` (or a push or pop-up) moves the focus on the screen
  below, so closing the modal gives it back to `x`, not to the node
  focused before. A handler's focus moves and screens now take effect in
  the order it asked for them. A node focused before it was laid out (by a
  watch as the app starts, inside a `switch`'s page) has its focus path
  once it is laid out, so a command line closing gives the focus back to
  it (the tasks clone).
- **A styled `layers` node covers its whole rectangle**, the gaps its
  inset children leave included: the layers below no longer show through
  (the blocks clone).
- **`layout::squarify` keeps the largest item's tile** when the rest's
  tile would be a strip too thin for the minimum: the rest grows until it
  fits, where every item used to go into it (the disk clone).
- **A focused text box keeps its keys from `previews_keys` containers.**
  An input, text area or open `command_line` takes the keys it uses that
  have no Ctrl or Alt (letters, Enter, the arrows) before a previewing
  container round it, so a command line can sit inside the app's keymap;
  Ctrl and Alt keys still go to the container first (the csv clone).
- The tasks, blocks, disk and csv clones lose their workarounds; csv's
  `<n>G` ends the number in the goto line's `on_change`.

### intuiTUIve, rs-rich-ext: csv's million-row filter is fast again (rs-rich-intuituive, rs-rich-ext)

- **`TableSource::rows(range, each)`**: a run of rows in order, by
  default a `row` each; `view_of`'s passes read through it, a thousand
  rows at a time, so a source can read in bulk.
- **`rich_ext::transform::Pattern`**: a regular expression (fancy-regex,
  as `KeepLines`) compiled once and run on a `&str` (`is_match`,
  `try_is_match`), no `Text` built.
- The csv clone's filter over a million rows takes 3.8 s in the
  unoptimised build, from 15.4 s (0.27 s in release, from 2.3 s), with a
  release benchmark bounding it at a second
  (`a_million_row_filter_is_fast`, ignored by default).

### Marketplace: the listing format, and the host's versions (plugin-api, ext, scripts)

- **The listing format** ([design note](docs/design/marketplace.md)): one
  JSON file per package with a `schema_version`, its kind (`crate`,
  `wasm-plugin`, `micro-pack`, `theme`), tier, repository and every version's
  artifact URL, SHA-256 and size, `requires`, capabilities, permissions and
  yanking. One minisign-signed index pins every listing by hash, with a
  serial and an expiry; a backup key only rotates keys. Only WASM plugins and
  asset packs install; crates are listed only.
- **Schemas and a validator.** `schemas/marketplace/listing.schema.json` and
  `index.schema.json`, valid examples of every kind and invalid ones (a bad
  hash, an unknown kind, a missing ABI and ten more).
  `scripts/validate_marketplace.py` checks listings and indexes with the
  standard library, and with `jsonschema` too when it is installed (the two
  must agree); CI runs it and its tests.
- **`rs-rich-plugin-api`:** `is_supported_api_version` and
  `abi::is_supported` are the checks the registry and both loaders make
  against `PLUGIN_API_VERSION`, `ABI_MAJOR` and `ABI_MINOR`, so a tool that
  resolves `requires` decides as the host loads. The constants are
  unchanged (API 1, ABI 1.0) and documented for the marketplace; tests pin
  the WASM loader, the native descriptor reader and `add_plugin` to them.

### Interact: Huh-style forms and a paginator (rs-rich-interact)

- `Form` gains the rest of Huh's fields: `multi` (several options, with
  `chosen` and `limit`), `note` (read-only text that focus passes over),
  `file` (a path picked with a `FilePicker` opened under the field), and
  `dynamic_choice`, whose options come from the answers above it and follow
  them as they change. `describe` puts a line of help under any field.
- `Form::group` splits a form into pages shown one at a time over a
  paginator: Enter on a page's last field or Page Down checks the page
  first, Page Up goes back, and a submit that fails shows the page of the
  field that failed. Each new field has a line-by-line form without a
  terminal.
- `Value` gains `List` and `Path` (a breaking change for an exhaustive
  `match`), and `Answers` gains `list` and `path`.
- New `Paginator`: dots (`● ○ ○`) or a fraction (`2/3`), moved with the
  arrows, Page Up and Down, Home and End, the wheel or a click on a dot.
  It is state to embed (`bounds`, `handle_key`, `indicator`) and a
  component of its own over a list of items. New `FilePicker::reopen`, and
  a `huh_form` example.

### CLI: `rich spin` (rs-rich-cli)

- New `rich spin [--title TEXT] [--spinner NAME] -- COMMAND [ARG]...`, after
  `gum spin`: runs the command with core's `Status` spinner and the title on
  stderr until it exits, and exits with its status. `--spinner` takes any of
  core's spinner names. On a terminal the command's output is held back;
  `--show-output` prints it after, and `--show-error` only on a failure.
  Without a terminal there is no spinner: the command just runs. Reduced
  motion draws the title once.

## 0.0.20 cohort — published 2026-10-19

Published on 2026-10-19 from `56c6c97` by one `release.yml` run (tag
`release-0.0.20`), the first release made with the one-run pipeline:
core 0.0.10 / macros 0.0.4 / plugin API 0.0.4 / art 0.0.13 / diagram 0.0.3 /
mermaid 0.0.6 / lumis 0.0.4 / ext 0.0.17 / micro 0.0.6 / record 0.0.8 /
interact 0.0.8 / ratatui 0.0.5 / intuituive 0.0.5 / embed 0.0.3 / web 0.0.3 /
data 0.0.4 / CLI 0.0.20. `rs-rich` 0.0.8 on PyPI (`python-v0.0.8`, same
commit) followed once the maintainer approved it, early on 2026-10-20 (UTC). See
[the release notes](docs/releases/0.0.20.md).

### Releases: one run for every crate (scripts, release.yml)

- **A release tag publishes the whole release.** `release-<name>` (the
  release's name, rs-rich-cli's version: `release-0.0.20`), annotated, on
  the release commit, with `Release:` and `Commit:` lines, starts one
  `release.yml` run. It checks the release once, runs CI once and publishes
  every crate of its `RELEASES.toml` not yet on crates.io in one job, where
  each crate tag used to run all of it again. `release.yml` starts only on
  `release-*` pushes now; the crates' `<crate>-vX.Y.Z` tags are records, and
  dispatching one by hand still publishes that crate alone. PyPI is
  unchanged (`python-v*`).
- **The gate computes the selection.** `release_cohort.py unreleased` of a
  release tag selects, in `RELEASES.toml` order, every crate not yet on
  crates.io, and refuses when none is due or a selected crate's tag is
  missing or on another commit; `same-commit` checks the release tag (and
  requires its `Release:`/`Commit:` lines); `release.py plan` checks every
  selected manifest and internal requirement. `release_cohort.py published`
  selects what a `verify_only` run checks.
- **One multi-package publish.** `release.py publish` runs one
  `cargo publish -p a -p b … --dry-run` for the selection, then uploads with
  `-p … -p …`. New crates may share a release with existing ones: they
  upload with the environment's `CARGO_REGISTRY_TOKEN` and the rest with
  Trusted Publishing, in separate runs in dependency order
  (`publish_runs`); a release used to refuse the mix. Verification runs
  once for the whole selection.
- **`release_cohort.py tag` pushes once and waits once.** It makes the
  record tag of every crate due and pushes them in one atomic push, then the
  release tag and `python-v…` (when PyPI is due, unless `--registry
  crates.io`) in a second (GitHub starts no workflow for a push of more than
  three tags), then waits for the release run and the PyPI run, where it
  pushed and waited tag by tag. `--from` is gone: a release run publishes
  everything due.
- **Re-running resumes.** Run `tag` again after a failed run: a release
  tag whose run ended without success is deleted and pushed again (on the
  release commit when some crates are out; moved, with its records, to
  `HEAD` when nothing was), and the new run publishes only what is still
  due. `status` shows the release tag. `tag` refuses to push a release tag
  on a commit whose `release.yml` predates them (release 0.0.19's), which
  would start nothing.

### Versions: core and the crates published since the 0.0.19 cohort

- rs-rich moves to 0.0.10: the `Text.markup` port changes `src/text.rs`, and
  0.0.9 is on crates.io. Cargo's 0.0.x requirements are exact, so the
  published crates that depend on it move with it: plugin API 0.0.4, macros
  0.0.4, art 0.0.13, diagram 0.0.3, Mermaid 0.0.6, lumis 0.0.4. Data 0.0.4,
  web 0.0.3 and the CLI 0.0.20 changed since the 0.0.19 cohort was cut
  (2fc9772) and are published at their old numbers. The other crates already
  carry unpublished versions. `RELEASES.toml`, the version tables and both
  `Cargo.lock` files follow.
- The Python package `rs-rich` moves to 0.0.8 (PyPI): it builds those crates,
  so its `Cargo.lock` changed with them, and 0.0.7 is on PyPI. Without the
  bump the cohort's tagging refused main as shipping a different 0.0.7.

### Repo: the app template no longer trips git dependencies

- `templates/intuituive-app/Cargo.toml` is now `Cargo.toml.liquid`. A crate
  that depends on one of this repo's crates by `git` made cargo read every
  `Cargo.toml` in the checkout, the template's among them, and its
  `{{project-name}}` placeholder printed an "invalid character in package name"
  error on every build. cargo-generate drops the `.liquid` suffix, so
  `cargo generate` still writes a `Cargo.toml`; the template now asks for
  cargo-generate 0.23.4 or later, the release that fixed that suffix for git
  templates. Checked with cargo-generate 0.25.0: the generated app builds and
  passes its test against a `git` dependency, with no error.

### Core: `Text.markup` (rs-rich)

- **`Text::markup()`**, a port of upstream's `Text.markup` property: the console
  markup that renders a `Text`, with every tag-opening `[` escaped and a tag
  around the base style and each span, in upstream's order (opening tags before
  closing ones at a shared offset; a null style or empty name adds none).
  `Text::from_markup(&text.markup())` gives back the same plain text and spans.
  Byte-checked against rich 15.0.0 in `golden/functions.tsv` (`text_markup`,
  `text_markup_from_markup`). An `[@handler]` tag still carries no meta
  (DIVERGENCES §3a), so its text gets no `[none]` tag.

### intuiTUIve: tables, trees and lists that follow state (rs-rich-intuituive)

The round-5 clones rebuilt tables under a `switch` for every change of
layout, wrote marks into cells, found their selection again by hand after
each re-sort, and walked trees for index paths to reopen
([the demo gaps](docs/design/intuituive-demo-gaps.md), "Round 5", shared
gaps; [csv](docs/design/gaps/csv.md), [tasks](docs/design/gaps/tasks.md),
[music](docs/design/gaps/music.md), [top](docs/design/gaps/top.md),
[git](docs/design/gaps/git.md)). All of it is now generic core:

- **Tables whose options follow state** (`TableOptions`):
  `columns_with(|| columns)` and `columns_shown(|| indices)` change the
  columns, their titles, which show and their order in the same table, so
  the scroll across, the cursor and the widths stay;
  `freeze_columns_with`, `multiline_when`, `cells_when(|| on, column)`
  and `sort_indicator(|title, order| ..)` read state at each draw;
  `widths(Signal<Vec<Option<u16>>>)` is shared with the mouse's drags;
  `offset_x(signal)` reads and sets the scroll across;
  `layout(&TableLayout)` records `width_of(column)`, `columns_in_view()`,
  `first_row()` and `rows_in_view()`.
- **Columns:** `Column::min_percent` / `max_percent` (of the table's
  width, scrolled across too) and `Column::priority(n)`: columns dropped
  whole, lowest first, when they do not fit, so a flexible column keeps
  its minimum.
- **Marks and the cursor on tables:** `TableOptions::marker(|i| style)`
  (a marker column outside the highlight), `Node::selection_markers` and
  `Node::marks_by` on tables, `Node::cursor_by` on tables,
  `TableOptions::symbol_for(width, |i, selected| markup)` (a symbol per
  row) and `highlight_layer(HighlightLayer::Under)` (also through
  `Node::highlight_layer`).
- **Row heights:** `TableOptions::row_height_for(|i| lines)`; a
  `virtual_table` with `multiline` lays out only the rows near the view.
- **Rows from a source** (the new `source` module, re-exported from
  `widgets`): the `TableSource` trait (`len() -> RowCount`, `Exact` or
  `AtLeast`, and `row(i)`), `source_table(source, columns, selected,
  options)`, and `view_of(source, || ViewQuery)`, which filters, sorts
  and finds over a `Send + Sync` source on a worker thread, one read a row
  a pass, a new query dropping the old; the `View` is a `TableSource`
  with `source_row`, `found`, `next_found`, `previous_found`, `place` and
  `is_busy`.
- **Keyed trees:** `tree_keyed(items, selected, expanded)` keeps the
  selection and the open items by `TreeItem::key` (an item that goes
  leaves the cursor at its row); `tree_reveal(&items, key, selected,
  expanded)`; `TreeItem::keyed`, `TreeItem::prefix(markup)` (drawn before
  the indent); `Node::symbol` on trees; `Node::tree_keys(TreeKeys)` to
  leave Enter/Space, the letters or ←/→ to the ancestors; `LazyTree` and
  `tree_lazy_with` to reload a lazy tree's level. `TreeItem` has two new
  public fields (`key`, `prefix`): one built with a struct literal needs
  `..Default::default()`.
- **Ranges on lists:** `list(..).range(anchor)`: Shift with the moves
  extends a range from the anchor, a plain move ends it, the rows between
  in the theme's `selected.range`; a list without one leaves Shift to its
  ancestors.

### intuiTUIve: completion, a command line, queries and dates (rs-rich-intuituive, rs-rich-interact)

What the taskwarrior-tui and slumber clones built by hand for their
command bars, filters and due dates ([the demo gaps](docs/design/intuituive-demo-gaps.md),
"Round 5", shared gaps; [tasks](docs/design/gaps/tasks.md),
[http](docs/design/gaps/http.md)). All generic: the syntaxes and
candidates are the app's.

- **Input completion:** `input(..).complete(|text, caret| Vec<Completion>)`
  and `complete_with(.., CompleteOptions)` (as you type, ghost text,
  rows): a pop-up under the input (Tab, Shift+Tab, the arrows, Enter
  takes, Esc closes) drawn over its neighbours without taking the keys,
  and the top candidate's rest as ghost text (→ or End takes it).
  `Completion::new(range, text).detail(..)`, `complete_words(list)`,
  `complete::word_before`. Floats: a widget's drawing over its
  neighbours, painted after its screen, everything drawn again when one
  shrinks or goes (crate-internal for now).
- **Input history:** `input(..).history(h)` (↑/↓ through the entries that
  start with what was typed; Enter adds one) with `InputHistory`: a name
  for the session's, `history_session()`, `history_in(app, name)` in the
  app's state, `history_file(path)`; `status()` for `[3 / 7]`.
  `PromptOptions::complete` and `::history` do the same for `cx.prompt`.
- **A command line:** `command_line(CommandLine)` and
  `command_line_with(.., CommandLineOptions::new().two_rows())`;
  `open(cx, Mode::new(":"))` with `title`, `initial`, `complete`,
  `history`, `on_submit` (`Err` keeps it open, in red), `on_change`,
  `on_cancel`; `confirm`/`confirm_keys` asked in place; `error`,
  `message`, `close`; the focus goes back where it was.
- **Queries:** `query::parse(text, &Grammar) -> Query` and
  `query.matches(&record)` over a `Record` trait (`field`, `text`,
  `tags`/`has_tag`): words, quoted words, `field:value`,
  `field.before:/after:/above:/below:/is:/not:/has:/hasnt:/startswith:/
  endswith:/none:/any:`, `field > value` and the other comparisons,
  `+tag`/`-tag`, `and`/`or`/`not`/`!` and parentheses; a `Grammar` of
  fields, aliases, `Kind`s, `TextMatch`, value readers, date reading and
  word matchers; `QueryError` with a byte range.
- **Calendars:** `CalendarOptions::title(MonthTitle::Name)` and
  `month_style_with(|first_of_month| ..)`; `month_with` and
  `calendar_options_with` read their options from signals; `year_view`
  and `year_view_with` (`YearOptions`: months across as fit, `marks`,
  `today`, `across`, `gap`, `heading`); `agenda(from, days, items)`;
  `Weekday::name`.
- **rs-rich-interact dates and zones:** `dates::parse` and `parse_with`
  (`now`, `today`, `tomorrow`, `eod`, weekday and month names,
  `sow`/`eow`/`som`/`eom`/`soq`/`eoq`/`soy`/`eoy` and the current ones,
  ISO dates and times, `3d`, `2w`, `1mo`, `-2d`), `DateOptions`,
  `dates::NAMES`; `datetime::Zone` (UTC, `Local` through `localtime_r` on
  Unix and `SystemTimeToTzSpecificLocalTime` on Windows, `Fixed`,
  `posix(rule)`, `tzif(bytes)`, `named(name)`), `DateTime::from_unix`,
  `to_unix`, `now_local`, `now_in`, `add_seconds`, `Date::today_local`,
  `today_in`, `Time::now_local`. Tested across a change of summer time.
  intuiTUIve re-exports `dates`.

### Rendering: styles that don't pile up, fast pixel cells (rs-rich-ext, rs-rich-intuituive)

A full screen of truecolour pixels added thousands of styles a frame that
were never dropped, the painter copied the whole style table every paint,
and interning hashed with SipHash: the raycaster's, the sprite stage's and
real Doom's finding ([the demo gaps](docs/design/intuituive-demo-gaps.md),
"Round 5" and "Real Doom"; [raycast](docs/design/gaps/raycast.md)). A
160x50 frame of random half blocks now takes 8 ms in release instead of
224 ms (80x24: 1.5 ms instead of 48), with the same bytes; a dashboard
tick is unchanged (about 0.02 ms, 18 bytes).

- **rs-rich-ext `frame::StyleTable`:** interns with a local Fx hash and a
  chain of ids per key, not SipHash and a `Vec` per bucket;
  `intern_rgb(fg, bg)` interns a colour pair, building the `Style` only
  the first time, and `rgb(id)` reads it back; `retain(&keep)` drops what
  is unused and returns a `StyleRemap` (`get`, `get_or_gone`); `piled_up`
  says when that is worth it. `StyleId::GONE` is no style's id.
- **rs-rich-ext `surface::Screen`:** `put_rgb(x, y, ch, fg, bg)` and
  `put(x, y, ch, style_id)` set one cell without a segment or a style;
  `compact_styles()` drops the styles no cell uses, `styles_piled_up()`
  and `style_count()` say how many there are; `underlay`/`restyle`
  combine each pair of styles once, and `blit` translates each style once.
- **rs-rich-ext `surface::Painter`:** keeps only the cells it sent, not a
  copy of the style table each paint; writes true-colour pairs' SGR
  straight from the colours; `remap_styles(&remap)` follows a compaction
  and `compact(&mut screen)` does one when the table has piled up.
  `Surface` compacts after each frame.
- **intuiTUIve:** `Canvas::put_rgb` and `DrawCx::rect()`; the `pixels`
  node draws its cells with `put_rgb` (same output); the app compacts its
  screen after each paint, and a viewport its content.

### Input and timing: every key, held keys, timers you can steer (rs-rich-interact, rs-rich-intuituive)

The round-5 input and timing gaps of the falling-block, raycaster and
system-monitor clones and of real Doom ([the demo
gaps](docs/design/intuituive-demo-gaps.md), "Round 5"):

- **Fixed:** the app loop handled one input event a frame, so keys queued
  during a slow frame spread out over the frames after; it now handles
  every event waiting (64 at most) before drawing (`Headless::queued`
  scripts it). Keys typed while the terminal did not answer the start-up
  query were lost for up to 3 s; they are kept and delivered, and the
  query waits 1 s at most, 250 ms once keys come. Tables took Shift+↑/↓
  as ↑/↓, so an ancestor's binding never fired; they leave modified keys
  to their ancestors. `Driver::send(event)` settles after an event as
  `press` does (`Driver::event` still does not draw, for loops of your
  own).
- **Key state:** `held_keys()`, `Ctx::is_held`, `Ctx::held_keys`,
  `plain_key`; release bindings `Node::on_key_up` and `bind_release`;
  `Ctx::key()`, the key that fired a binding; `KeyCode::Modifier`
  (`ModifierKey`) for a modifier on its own; `SessionOptions::all_keys`
  and `App::all_keys` (kitty flags 1+2+4+8); `Releases`,
  `Session::releases_reported`, `Backend::releases_reported`,
  `Ctx::releases_reported`, `Driver::set_releases`; releases made up with
  `App::synthesize_releases(hold)`.
- **Key repeat:** `Node::repeat(delay, rate)` fires a held binding at the
  app's own delay and rate (DAS/ARR), passing the terminal's repeats over.
- **Timers:** `every`, `after`, `Ctx::after` return a `Timer` (`pause`,
  `resume`, `set_period`, `reset`, `cancel`); `Ctx::every`,
  `Ctx::fixed_step` and `fixed_step(period, |cx, steps| ..)` with
  catch-up and `Timer::max_catch_up`, exact under `Driver::advance`;
  `Ctx::missed_ticks`.
- **Frame pacing:** `App::max_fps(n)`, `FrameStats::skipped`,
  `App::stats_handle()` (`StatsHandle`) for the stats after `run`.

### intuiTUIve: state-driven layout, focus scopes and keyed nodes (rs-rich-intuituive)

Round 6's layout, focus and help pieces, from the shared gaps and bugs of
the round-5 clones ([the demo gaps](docs/design/intuituive-demo-gaps.md),
"Round 5"; the git, top, http, music, disk, csv, tasks and blocks
reports), all generic core:

- **Fixed:** an `.auto()` `switch` takes its page's own `.fixed(n)`,
  `.min_size(n)` and `.max_size(n)` (bottom's sort menu came out 9 wide
  for 11); `layers` composite, so a layer shows the ones below where it
  draws nothing (a backdrop is one `fill`; a panel still covers its inside,
  a node with a background its rectangle), drawing each layer once when
  any of them changes; a `switch` can drop the pages
  it no longer shows (`.keep_last(n)`), where it kept every page it built.
- **Layout from state:** `Node::size_with(|| Size)`, `Node::shown_when(||
  bool)` and `hidden_when` (today's private `shown_if`, public);
  `keyed(|| key, |key| node)`, one child rebuilt when its key changes, the
  old one dropped and the focus passed on.
- **Panels:** `panel_with(|| markup)`, `title_with`, `add_title_with`;
  `border_with(|| Border)`, and `focus_border` now works on a panel.
- **Focus:** `Ctx::focus_within(id)` (once the container is laid out);
  `Node::focus_scope` with `Ctx::focus_next_scope`/`focus_previous_scope`;
  `Ctx::focus_direction(Direction)` and `Node::bind_directional_focus`, on
  the pure `focus::neighbour`; `focus::focused()` and `focus::has_focus(id)`
  as reactive reads; `Node::on_focus` and `on_blur`.
- **Treemaps:** `layout::squarify(rect, weights, min, aspect)`
  (`Squarified`), and `treemap`/`treemap_with(.., TreemapOptions)`.
- **Text:** `fit_text(|| forms)`, the longest that fits;
  `Node::truncate_middle`.
- **Screens and help:** `App::min_size(w, h, message)`;
  `widgets::key_hints_with(KeyHintsOptions)` (a format per hint, key
  symbols or vim notation, disabled bindings greyed, a separator, a "more"
  expander); `HelpOptions::all_panes`.

### Recording: pixels, every frame and held keys (rs-rich-record, rs-rich-cli)

`rich record` can now record a game as it plays:

- **Terminal graphics:** the emulator follows Kitty graphics (direct
  placements and Unicode placeholders, PNG and raw pixels, zlib, chunks,
  deletes), Sixel and iTerm2 inline images on the cell grid
  (`rich_record::graphics`, `Snapshot::images`, `Tile`); PNG, GIF, APNG and
  video frames draw them over their cells.
- **Every frame:** each settled screen update (a read that drained the PTY,
  no synchronized update open) is a frame; `Set Framerate N` / `--fps N`
  (1–60, default 12) sets the video's rate, merging identical frames and
  keeping only the last update within a frame. Frames are kept packed
  (`screen::Packed`: 12 bytes a cell, rows shared between frames; a breaking
  change to `Timeline::frames`), and images compressed.
- **Held keys:** `KeyDown KEY`, `KeyUp KEY`, `Hold KEY DURATION`, with
  autorepeat (`Set KeyRepeat`); `Set KittyKeyboard on` reports the kitty
  keyboard protocol, whose flags the emulator follows per screen (`CSI > u`,
  `CSI < u`, `CSI = u`), and presses, repeats and releases are then sent as
  its escapes (`rich_record::keyboard`).
- **APNG:** `Output apng` (and `--format apng`) writes an animated PNG in full
  colour beside the GIF; `Set FontSize N` sizes video text.
- The raycaster's tape is now a 35 fps walk-through (`docs/tapes/raycast.tape`,
  GIF 7.8 MB, APNG 4.7 MB) and `raycast-iterm.tape` records it in real pixels.

### intuiTUIve: sprites, stages and pixel buffers (rs-rich-intuituive)

Cell art that moves over a backdrop, and a framebuffer drawn in cells: the
round-5 gaps of the falling-block and raycaster clones ([the demo
gaps](docs/design/intuituive-demo-gaps.md), "Round 5"), generic in core,
with two plugins:

- **`sprite`:** `Sprite`, frames of cell art with transparent cells, from
  markup lines with a transparent character, from `SpriteCell`s or from
  pixel art in half blocks; frame times on the app's clock, looping or
  `once`; an anchor; flips that mirror brackets, slashes and half blocks.
  `stage(|| Vec<Placed>)` and `stage_with(.., StageOptions)` draw placed
  sprites (id, position in cells, z order, flip, fixed frame, start time,
  `Tween`) clipped and in z order over a backdrop node or the stage's
  background, a cell with no background taking the one beneath;
  `on_sprite_click` and `on_sprite_hover` find sprites by their opaque
  cells; `half_cells` moves pixel art by half cells. `overlaps`,
  `overlaps_at` and `bounds_overlap` test collisions; `tween(..)`,
  `Sprite::frame_at` and `sprite::now()` work on the app's clock.
- **`pixels`:** `pixels(w, h, |buf| ..)` and `pixels_fit(|buf| ..)` show an
  RGBA `PixelBuf` in half blocks, or quadrants or Braille
  (`Node::pixel_mode`); `PixelBuf::blit` (alpha, scale, flip),
  `fill_rect`, `line`, `plot`; `PixelSprite` frames from RGBA or
  character art.
- **Plugins:** `sprite_sheet` cuts an image into `PixelSprite` tiles and
  half-block sprites (`art`); a new off-by-default `micro` feature makes an
  rs-rich-micro `MicroAsset` a `Sprite` (`micro_sprite`, `micro_pixels`).
- `examples/sprites.rs`: animated sprites over a backdrop, selected by a
  click, steered with the arrows, beside a bouncing ball in a pixel
  buffer.

Measured in a release build (`tests/sprites.rs`): 300 moving sprites over
a full backdrop at 200x60 take about 6 ms a frame, painting included; a
200x120 pixel buffer whose every pixel changes, 12 to 17 ms, most of it
writing 12,000 differently coloured cells into the screen.

### intuiTUIve: round 5 plugins (rs-rich-intuituive, rs-rich-ext, rs-rich-art)

The plugin pieces of the round-4 gaps of the Yazi and scope-tui clones
([the demo gaps](docs/design/intuituive-demo-gaps.md), "Round 4: files
(Yazi)" and "Round 4: scope-tui"), built on round 5's core, with the few
generic core pieces they needed:

- **Core:** `transport(length)`, a play head on the app's clock (`play`,
  `pause`, `seek`, `seek_by`, `set_rate`, `position`, `progress`, an
  `on_end` hook met exactly on the app's clock); `Feed<T>` (a feed's value
  and `FeedState` as one value), `latest_feed(stall, |tx, state| ..)`
  with a `FeedTeller`, and `Play::feed()`; `TabSet::scoped(|| value)` (a
  value per tab, `Scoped`), tab identities (`active_id`, `ids`) and
  `MultiSelect::scoped(&tabs)` (one handle, a selection and visual mode
  per tab); `Miller::update_overlay(items)`, and ending an overlay runs
  the source's `arrive`; `digits` draws `G` and `#`.
- **Files plugin, rebuilt on the core:** `bulk_rename` on
  `Ctx::edit_text`; `frecency` on `FrecencyStore` (its API kept; two
  sessions' visits add up); `files::state_dir` is the core's
  `state_dir("intuituive")`; `Miller::show_search` is an overlay at the
  root, so `h` from a search's results goes back to the directory
  searched, on the entry hovered before, as Yazi; the parent column
  marks the selection by path with `marks_by` (the theme's
  `marker.selected`).
- **Files plugin, new:** `files::Miller::navigation_with` and `go`, and
  `files::Move` (Yazi's wording without binding each key); a selection
  and visual mode per tab with `tabs` (`FileTabs::selection`);
  `Yank::link(into, kind)` on rs-rich-ext's new **`Op::Link { from,
  into, kind, conflict }`** (`LinkKind::Symbolic { relative }` and
  `Hard`, directories hard linked file by file) and
  `file_ops::relative_to`; `files::pick_file(cx, start, filter, |path,
  cx| ..)`, a file picker modal on Miller's columns;
  `Previewer::command(mime_glob, argv)`, an external previewer (`%s`,
  `%o`, `%w`) whose image output is shown as an image.
- **Sound:** `Capture::generated(frames, period)` and
  `Capture::from_pcm(pcm, rate)` (Ended and then silence, or `Failed`
  with the read error), and `Capture::feed()`, so every source is one
  type; `Pcm::error()` and `Pcm::try_next()` keep the read error that
  ended a reader; `AudioOptions::stall_after` and `Capture::stall_after`
  take an `impl Into<Option<Duration>>` (`None`: never stalled).
  **`audio::output(OutputOptions)`** plays through cpal (behind `audio`;
  `output_devices`, an `Output` with `write`, `write_matrix`, `clear`,
  `set_paused`, `set_volume`); **`audio::player(PlayerOptions)`** plays
  tracks on a `transport` with a volume, a speed and gapless playback,
  over the `Source` trait (`Wav`, now seekable with `Wav::seek`;
  `audio::clip` in memory; `open_source` by extension; an opener of your
  own). Other codecs are later work (symphonia is not a dependency).
- **rs-rich-ext's dsp:** `Tuner` and `Reading`: the loudest peak's
  nearest note, its cents and frequency, smoothed within a note.
- **The `viz` plugin:** `tuner(|| reading)` (the note in `digits`, a
  needle, the cents in colour) and `persistence(|| frames)` /
  `persistence_with` over `chart::Persistence`.
- **The `art` plugin:** `image_bytes` and `image_bytes_of` (an image in
  memory, an embedded cover); `ImageView::pixels(Pixels::Kitty | Sixel |
  Iterm)` and `Pixels::detect()` with `image_graphics()` for
  `App::graphics`: Sixel and iTerm2 pictures drawn over the node's half
  blocks after each frame, Kitty's through Unicode placeholders, uploaded
  once and freed at the end; half blocks without the source. The feature
  now turns on rs-rich-art's `kitty`, `sixel` and `iterm` (no change to
  rs-rich-art itself: it already decodes from bytes).

### intuiTUIve: round 5 core (rs-rich-intuituive, rs-rich-interact)

The generic core pieces of the round-4 gaps of the Yazi and scope-tui
clones ([the demo gaps](docs/design/intuituive-demo-gaps.md), "Round 4:
files (Yazi)" and "Round 4: scope-tui"), none of them about files, sound
or one app:

- **Fixed: `.autofocus()` on a wrapper focused the wrapper.** On a node
  that is not focusable of its own (a `switch`, a panel, padding), the
  focus now passes to the first focusable node inside it, now or once it
  is built, and the wrapper leaves the Tab order: a list in a `switch`
  gets its own keys, and the menu key reaches its `on_context`.
- **Fixed: the help cut a long key list.** Wrapping rows wrap their keys
  too, between keys (within one only where it must), the key column
  taking at most half the width. `HelpOptions::filter_key("/")`: letters
  move (`j`, `k`, `g`, `G`) until the key starts the search, as Yazi's
  help (rs-rich-interact: `overlay::Help::filter_key`).
- **Bindings:** `bind_step_scaled(keys, variants, desc, signal, field,
  step, range)`, a clamped step scaled by the modifiers held; after a
  binding, `.any_modifiers()` (its keys with any modifiers; an exact
  binding still wins), `.fall_through()` (the key goes on to the next
  handler) and `.unlisted()` (left out of the help, the which-key pop-up
  and `key_hints`, still in the palette by its name);
  `bind_hidden(keys, name, f)`; `Ctx::pass_key()` passes a key on from
  inside a handler; `Driver::run_named(name)` runs a binding by its name.
- **Feeds:** `latest_iter(..).then_at(pace, iter)`, a tail at its own
  pace, and `.with_state()` (`Ended` once the values run out, while a
  tail plays on; `.stall_after(..)`); `Latest::wait_for(ready, timeout)`
  woken by a `Notifier` (`Latest::notifier`), and `Latest::blocking(..)`,
  which count a waiting feed thread as asleep for the headless clock
  under `wait_for_tasks`; `recent_of(signal, n, |v| part)` (a part of
  each write, nothing else copied) and `recent_timed(signal, n)` (each
  with its time on the app's clock, `Ctx::now()`); `falling(signal,
  gravity)`, a level that rises at once and falls with gravity.
  `Resource::stale_after` and `PreviewOfOptions::clear_after` take an
  `impl Into<Option<Duration>>`, as `latest_with_state` does.
- **Lists and Miller columns:** `Miller::overlay(at, items)` lists other
  items at a level (each opening a level of its own) until the columns
  leave it, `leave`/`end_overlay` going back to the level's own items
  (`overlay_at()`); `Node::marks_by(selection, |i| key)` marks a list's
  items in a `multi_select` it is not over; `ListSearch::next_from(here)`
  and `previous_from(here)` stay on a hovered match; `tabs_for_indexed`;
  `StatusSegment::when(|| cond)` hides a segment and its caps;
  `facts(..).value_style(..)` (theme `facts.value`) and
  `.value_style_for(|i| style)`.
- **App plumbing:** `Ctx::edit_text(text, |edited, cx| ..)` and
  `edit_text_as(text, ext, ..)` (a temporary file handed to `$VISUAL` or
  `$EDITOR`); the `state` module: `state_dir(app)`, `state_file`,
  `read_state`, `write_state` (written whole and renamed), `update_state`
  and `update_file` (read, change and write under a lock), `write_atomic`;
  `FrecencyStore` (`frecency_store(file)`, `frecency_in(app, name)`,
  `frecency_session()`) ranking string keys as zoxide does, merged on
  write so two sessions add up, with `keep` and `max_age`;
  `Jobs::run_process(title, command, done)` and `run_process_with(..,
  |line| progress, ..)` (`ProcessOutcome`, `ProcessOutput`: the last line
  of stderr as the error, killed on cancel); `Job::errors()`.

### intuiTUIve: round 4 sound, charts and the viz plugin (rs-rich-intuituive, rs-rich-ext)

The round-3 scope-tui gaps' sound and chart pieces, built on round 4's
runtime ([the demo gaps](docs/design/intuituive-demo-gaps.md)):

- **Fixed: grid lines shared one layer.** Under `Layering::PerDataset`
  each of an axis's grid lines is now a layer of its own, the y axis's
  first, as scope-tui's reference datasets draw: where an x and a y line
  cross, the x line is drawn over the y line (`⢸`), not merged with it
  (`⣸`) (rs-rich-ext).
- **Fixed: a grid with no datasets was drawn in dots.** The grid's
  marker is now the axis's `Axis::grid_marker(..)`, else the chart's new
  `Chart::marker(..)`, else the first dataset's, else Braille, so a
  chart's zero line looks the same before its first data as after
  (rs-rich-ext).
- **`Axis::grid_style(..)`** styles the grid lines apart from the axis
  line and labels (`Axis::style`), and **`Chart::legend_unstyled()`**
  names what `Chart::style("none")` does to the legend's box (the
  terminal's own colours); `"none"` still works (rs-rich-ext).
- **`chart::BarSpectrum`**: a column per band rising from the bottom in
  eighths of a cell, as cava's bars; `.mirror(true)`, the low bands in the
  middle, and `BarSpectrum::stereo(left, right)`; `.range`, `.bar_width`,
  `.gap`, `.style`, `.charset`, `.height` (rs-rich-ext).
- **`chart::Persistence`**: a trace's last frames, oldest first (a
  `recent` history), as a `Chart`'s datasets, each older one's colour
  faded towards `.background(..)` down to `.fade(floor)`, as a scope's
  phosphor (rs-rich-ext).
- **`dsp`**: `bands(spectrum, n, lo_hz, hi_hz)` (log-spaced bands),
  `Falloff::gravity(g)` (levels falling faster and faster, cava's),
  `peak_hz(spectrum)` (the loudest bin, refined between bins) and
  `NoteTable` (`Equal`, and `ScopeTui`: scope-tui's octave-0 table
  rounded to the hundredth and doubled, octave 0 by default, so a clone's
  buffer sizes match scope-tui's sample for sample). `nearest_note(hz)`
  now returns the name, octave and cents apart (`("A", 4, 0.0)`), not
  the name with its octave (rs-rich-ext).
- **`audio::Capture` on the core's `latest_with_state`:** its `status()`
  is a `FeedState` (`Running`, `Stalled(since)` on the app's clock,
  `Failed(why)`, `Ended` once `stop()`ped); `AudioStatus` is gone. The
  capture's app side is now under `pcm`, so
  **`Capture::simulated(frames, period)`** stands in for a sound card in
  tests, with `stall()`, `resume()`, `fail(why)` and `stall_after(d)`.
- **`audio::play(pcm, rate)`** plays a reader's buffers at the sample
  rate, then silence, on `latest_iter` (`pcm`); `Pcm::channels` and
  `Pcm::buffer`.
- **The `viz` plugin feature**, off by default: `meter`, `meter_with`,
  `sparkline`, `sparkline_with` and `Node::hold` (the peak hold) have
  **moved behind `viz`**, out of the always-compiled core, as views of
  one domain's data; an app that uses them turns `viz` on. With them,
  **`spectrogram(|| rows)`** and **`bar_spectrum(|| bands)`** (and their
  `_with` forms), nodes over rs-rich-ext's `Spectrogram` and
  `BarSpectrum`. CI builds the tapes' examples and docs.rs the docs with
  it.

### intuiTUIve: round 4 runtime (rs-rich-intuituive)

Runtime fixes and generic pieces the round-3 clones asked for
([the demo gaps](docs/design/intuituive-demo-gaps.md)):

- **Fixed: values from other threads were picked up at most every 50 ms.**
  A `Proxy` send (a `latest` value, a task's result, `Proxy::run`) now
  wakes the run loop's wait on the terminal through the backend's new
  `Waker`, so a feed faster than 20 a second is seen at its rate on every
  backend. Headless, `Driver::timeout` (and so `Driver::advance` and a
  script's `wait`) steps to the next `latest` thread asleep on the app's
  clock: a `rate` of a 10 ms `latest_every` reads 100, not 20.
  `Driver::set_waker` wires a loop of one's own.
- **Fixed: work a layout set off was not settled before the next event.**
  `Driver::render` runs the watches its new rects woke (a `RectHandle`'s
  readers) and, under `wait_for_tasks`, their tasks, before it returns.
- **Bindings in a state:** `Node::bind_when(cond, keys, desc, f)` and the
  `when(cond, |node| …)` scope (any kind of binding, sequences too): a
  binding applies, and is listed in the help, the palette, the which-key
  pop-up and `key_hints`, only while its condition holds; two may share a
  key. `in_mode(mode, value, section, |node| …)` and `modes(mode, &[…])`
  scope a mode's bindings and help section to the mode a signal holds.
- **Sections for every kind of binding:** `Node::section(name, |node| …)`.
- **`bind_toggle`** (a `bool` field of a signal's value) and **`bind_step`**
  (a number field moved by a step, clamped to a range; `Stepped`).
- **`bind_scaled` labels:** `("alt", 0.2, "⅕")` is listed `alt ⅕`
  (`Scaled`); the old two-item form still works.
- **The help wraps long rows** instead of cutting them, and
  **`Ctx::help_with(HelpOptions)`** sets its title, its key notation
  (`KeyFormat::Vim`: `<C-u>`, `gg`) and a filter-first layout (Yazi's).
- **Feeds and signals over time:** `latest_with_state(stall_after, run)`
  (`FeedState::{Running, Stalled(since), Ended, Failed(why)}`),
  `latest_iter(iter, Pace::Every(d) | Pace::Free)` with `.then(iter)` /
  `.then_with(f)` / `.signal()`, `history(signal)` (`back`, `forward`,
  `can_back`, `can_forward`, `limit`), `recent(signal, n)` (`n` a number
  or a signal: `Count`), `smoothed(signal, attack, release)` and
  `on_write_ref(signal, |&T, cx| …)`.

rs-rich-interact, for the above: `Backend::waker` and `Waker` (a self-pipe
polled with the terminal on the crossterm, termion and termwiz backends on
Unix; short waits elsewhere); `Key::parse` names the punctuation keys
(`equals`, `minus`, `plus`, `underscore`, `slash`, `backslash`,
`period`/`dot`, `semicolon`, `colon`, `quote`, `backtick`, `tilde`,
`bracketleft`/`bracketright`, `lt`, `gt`, `question`, …;
`event::punctuation`), shown as the character; `overlay::Help::wrap`,
`::flat` and `::key_format`, off by default.

### intuiTUIve: round 4 widgets (rs-rich-intuituive)

Generic pieces for the round-3 gaps of the Yazi clone, none of them about
files (the `files` plugin builds on them later):

- **Fix: list options on a wrapper are no longer dropped.** `.symbol`,
  `.highlight_layer`, `.wheel_step`, `.selected_style_for`, `.marker` and
  every other list option reach the list a node wraps: a panel, padding
  (any node with one child) and a `switch`, whose lists get them as they
  are built (a list with a "No items" text in its place). On a node that
  wraps no list a debug build says so on stderr, once an option.
- **Tab sets:** `tab_set(|| state)` keeps a state per tab with one active
  (`open`, `open_with`, `close`, `close_active`, `switch`, `cycle`,
  `swap`, `with_active`, `update_active`, `titles`); `tabs_for(&set,
  |state| title)` is a `tabs` strip over it.
- **Fitted tabs:** `tabs(..).fit()` cuts the titles with `…` to share the
  width; unselected titles take the theme's `tabs.unselected` style.
- **Multi-select:** `multi_select::<K>()` (`MultiSelect`, `Mark`), a
  selection by key over a list's items (`over`) with `toggle`,
  `select_all`, `invert`, `clear`, a visual range from the cursor
  (`range_from`, `visual(unset)`, `end_visual`, `cancel_visual`),
  `selected()` and `targets(hovered)`; `list.selection_markers(sel, |i|
  key)` draws the marks (`marker.selected`, `marker.visual`,
  `marker.unset`).
- **Find in a list:** `list_search(query, items, cursor)` (`ListSearch`)
  with `next`, `previous`, `place() -> (n, of)`, smart case.
- **Lists:** `.cursor_by(|i| key)` keeps the cursor on its item when the
  items change; `.on_context(|i, cx| ..)` runs on a right press or
  Shift+F10.
- **Status bar:** `seg_gauge(fraction, label)`, a segment filled to a
  share (`.width(cells)`; `status_bar.gauge`, `status_bar.gauge.rest`).
- **Description lists:** `facts([("Name", value), ..])` and
  `facts_of(|| rows)`, with `.key_style(..)` (theme `facts.key`).
- **A streaming fuzzy list over any source:** `fuzzy_list(source,
  display, on_pick)` and `fuzzy_list_with(.., FuzzyOptions)` (a title, a
  border, a prompt, `on_cancel`), over `FuzzySource::iter(items)` or
  `FuzzySource::feed(|feed| ..)`; Esc closes its screen. `finder_of` is
  now one over an iterator, its API unchanged (it gains Esc).
- **Digits:** `digits(markup)`, big text three rows tall in box-drawing
  glyphs with no FIGlet font, in every build (Textual's `Digits`).
- **Miller columns:** `Move` and `Miller::go`, `navigation_with(node,
  &[(Move, keys, description)])` for a clone's own keys and wording
  (`navigation` is it with the defaults), `view_with(MillerOptions)` with
  `current`/`parent`/`preview` hooks on the columns' lists, a method for
  each signal (`at()`, `cursor()`, `hovered()`, `items()`, `first()`,
  `parent_cursor()`, `preview_cursor()`, `remembered_all()`), and
  `visited()`, the levels shown.

### intuiTUIve: round 4 files plugin (rs-rich-intuituive, rs-rich-ext)

The `files` plugin's part of round 4, built on the round-4 runtime and
widgets above ([the demo gaps](docs/design/intuituive-demo-gaps.md),
"Round 3: files"):

- **Fixed: the first frame's preview.** `preview_of_with` (and so
  `files::preview`) is now a `resource_for` keyed by the pane's width and
  the key, with `clear_after` as its `stale_after` (on the app's clock)
  and no generation counter or sleeping thread of its own: with the
  runtime's layout fix, the first frame shows the hovered entry's
  preview, with no wait.
- **The hovered row's ends:** `FileListOptions::hovered_caps(open,
  close)` and `MillerOptions::hovered_caps`: Yazi's Nerd Font half
  circles in the row's type colour, on the list's `highlight_layer(Under)`.
- **`file` classification:** `Previewer::file_type()` (`file -bL`, else a
  description of the MIME type), chosen for binary files by
  `PreviewOptions::binary(BinaryPreview::FileType)` (or
  `MillerOptions::binary`). **Breaking:** `Previewer` is a struct
  (`Previewer::new(f)`, `preview(..)`), no longer an `Arc` alias;
  `PreviewSettings` has a `binary` field; `previewer_of(..)` adds one.
- **Tabs:** `files::tabs(miller)` (`FileTabs`, `FileTab`): a directory
  and a back/forward history per tab on the core's `tab_set` and
  `history`, with `open`, `open_at`, `close_active`, `switch`, `cycle`,
  `swap`, `back`, `forward`, `rename` and `strip()`.
- **Selection and yank:** `files::selection(miller)`, the core's
  `multi_select` over the current column by path, marked in the columns;
  `Miller::targets()`, the selection or the hovered file; `files::Yank`
  (`copy`, `cut`, `marker`, `markers(signal)`, `paste(into, conflict) ->
  Op`).
- **Miller:** `find_next(previous)` on `list_search`; the cursor kept by
  path through a re-sort, a filter, hidden files and `refresh` (the
  list's `cursor_by`); `MillerOptions::on_context(|entry, cx| ..)` and
  the core's `current`, `parent` and `preview` hooks; `visited()`;
  `show_search(search)`, `searching()` and `end_search()`.
- **Tasks:** `Tasks::summary() -> (share, running, failed)` for a
  `seg_gauge`; `Tasks::on_fail(|job, cx| ..)` for every job's failure;
  `tasks_panel_with(jobs, JobsPanelOptions::new().failed_title(false))`
  so a failure is told once; `open_with_tasks(cx, opener, paths, jobs,
  done)` runs extraction as jobs.
- **Frecency:** `files::frecency()` (or `frecency_at(file)`): `visit`,
  `ranked`, `remove`, `follow(miller)`, zoxide's ranking, kept in
  `state_dir()` (`$XDG_STATE_HOME/intuituive`, Application Support,
  `%LOCALAPPDATA%`), written whole and renamed into place.
- **Finder:** `files::finder_with(walk, on_pick, FuzzyOptions)` on the
  core's `fuzzy_list`: a title, a border, Esc closing its modal or
  `on_cancel`.
- **Spotter, bulk rename, shell, search:** `files::spot(path)` (and
  `spot_facts`) on `facts`; `bulk_rename(cx, paths, done)` through
  `$EDITOR` (names may swap; conflicts reported); `shell(cx, paths,
  ShellOptions)`, Yazi's `;` and `:` with `$0`/`"$@"` (`shell_command`);
  `search(root, query, SearchKind::Name | Grep)` through `fd`/`rg` or a
  walk of its own, streaming.

Core, for the above: `jobs_panel_with` and `JobsPanelOptions`;
`Ctx::is_modal()`, and a `fuzzy_list` on the first screen leaves Esc to
the app; a list's `on_context` answers the menu key. rs-rich-interact:
`KeyCode::Menu`, parsed as `menu` (crossterm's `Menu`, termwiz's
`Applications`), which rs-rich-embed sends on. rs-rich-ext:
`trash::send`, the platform's trash (FreeDesktop; the Finder's through
`osascript`; the Recycle Bin through `SHFileOperationW`, with
`windows-sys`, already in the tree), which `file_ops`' `Op::Trash` now
uses.

### intuiTUIve: core components are generic (rs-rich-intuituive)

The rule for intuiTUIve is now that its always-compiled core holds only
generic components, reusable by other components; anything only one
domain could use (files, sound, one app) is a plugin, built on a generic
core piece where there is one. [The demo gaps](docs/design/intuituive-demo-gaps.md)
and `AGENTS.md` make it a gate for new proposals. An audit of the core
moved or split what did not pass:

- **Miller columns over any tree:** `miller_of(source, start)` (and
  `miller_in(source, at)`) over a `MillerSource` (a level's items, the
  level an item opens, the level above, an `arrive` hook): the parent,
  current and preview columns with remembered cursors per level, clicks,
  the wheel, draggable rails and the movement keys (`intuituive::miller`).
  `files::miller` is now a thin layer over it, its API unchanged.
- **Jobs with progress:** `jobs::<P>()`, `Jobs::run(title, work, done)`
  and `jobs_panel(list)`, generic over the progress (`JobProgress`; `f64`
  is one) and the outcome (`JobOutcome`; `()`, `Result<T, String>`).
  `files::tasks` runs file operations on it, its API unchanged;
  `files::Job` is now `Job<file_ops::Progress>`.
- **A streaming fuzzy finder over any items:** `finder_of(items, display,
  on_pick)`, read on its own thread. `files::finder` is it over a
  directory walk; `files::Finder` is now `Finder<String>`, its `walking`
  field renamed `streaming`, and its status says `loading…`.
- **A keyed preview pane:** `preview_of(key, load, render)` and
  `preview_of_with(.., PreviewOfOptions)` load a value off the app's
  thread for the current key and drop results for keys left behind.
  `files::preview` is built on it, its API unchanged.
- **Breaking: `breadcrumb` takes parts, not a path.**
  `widgets::breadcrumb(|| Vec<String>)` joins any trail with ` › ` (or
  `.crumb_separator(..)`); `.home_tilde()` and `.home_tilde_at(..)` are
  gone from the core. The path form is the `files` plugin's
  `path_crumb(|| path)` (home as `~`), `path_crumb_at(.., home)` and
  `path_parts(path, home)`.
- **Breaking: the `audio` module needs a feature.** `audio::pcm`,
  `Format`, `deinterleave`, `Frame` and `Matrix` were compiled in every
  build; they are now behind the new off-by-default `pcm` feature (no
  dependency), which `audio` and `wav` turn on. The scope example needs
  it (`--example scope --features pcm`; `required-features`), and CI's
  tapes build turns it on.
- **`hex_view` moves to the core** (`widgets::hex_view`): any bytes are
  generic. `files::hex_view` re-exports it.

Kept in the core after the audit, as generic: `meter` and `sparkline`
(any level, any series; decibels are a scale option), `big_text` (`art`),
`key_hints`, `status_bar`, `cells`, `live`, tabs, the prompt, pick and
confirm pop-ups, which-key, `latest`/`latest_every`, `rate` and the
`bind_*` family. The image nodes stay behind `art`, `watch_path` behind
`watch`.

### The Yazi clone rebuilt on round 3 (rs-rich-intuituive)

- `examples/files.rs` is rebuilt on the `files` feature's Miller columns,
  previews, openers, tasks and finder and the core's `status_bar`,
  `breadcrumb`, `tabs(..).auto_hide().on_select(..)`, `cx.prompt_with`,
  `cx.pick_with` and `cx.confirm_with`: 2,756 lines down to 1,450. Newly as
  Yazi: the system's trash, file jobs in the background with a task
  manager (`w`) and a quit confirm while they run, git signs, sniffed type
  colours, archive previews and extraction, the streaming finder (`z`),
  50-wide prompts, 70 x 20 confirms, and a click in the parent column that
  goes into the directory. It now needs the crate's `files` feature
  (`required-features`); CI's tapes job builds it with `files` and
  `archive`. Round 3's findings are in
  [the demo gaps](docs/design/intuituive-demo-gaps.md).

### intuiTUIve: widgets, pop-ups and runtime from round 2 (rs-rich-intuituive)

The rest of what the round-2 rebuilds of scope-tui and Yazi found
missing (see [the demo gaps](docs/design/intuituive-demo-gaps.md)), and
what the plugin work on files, audio and charts asked of the core.

- **Lists.** `.highlight_layer(HighlightLayer::Under)` puts a list's
  highlight under the item's markup, so Yazi's rounded ends keep their
  colour on the hovered row. **`list_rows(len, |i| ListRow, selected)`**
  makes each row once a draw, with its trailing text, marker, row style
  and highlight style (`.marker_column()` keeps the marker column);
  `list_lazy` is unchanged. `.wheel_step(n)` sets how far the wheel moves
  the selection, and `0` leaves the wheel to the list's ancestors.
  `selected_style_for`'s docs no longer suggest it where `row_style` and a
  reversed highlight do.
- **`Node::truncate_start()`**: a text too long loses its start, with
  `…`, rather than its end.
- **Pop-ups.** `cx.confirm_with(.., ConfirmOptions::new().size(w, h)
  .title_style(..), ..)` sizes a confirm and styles its title; with no
  body, the items sit under the title with no empty row or rule (fixed).
  **`cx.prompt(title, initial, |text, cx| ..)`** and `prompt_with(..,
  PromptOptions)` (placement, caret, title style, change and cancel
  handlers) are Yazi's input; **`cx.pick(title, origin, items, |i, cx|
  ..)`** and `pick_with` its "Open with:" menu. A pop-up's panel's
  `.title_style` already overrode `popup.title` per pop-up.
- **Fixed: a rule inside padding did not join its panel.** A rule at the
  inside edge of a padded column, or of a panel a stylesheet pads, runs
  across the padding to the border.
- **Fixed: a pop-up below a row slid away from it.** `Origin::At` makes a
  box too wide for the room right of the rectangle narrower (down to the
  rectangle's width) before moving it.
- **`App::notify_width(size)`**: how wide a notification may grow, 48
  cells by default.
- **Bars.** `status_bar(left, right)` of `seg(..)` segments with
  `.caps(Caps::Powerline | Caps::Rounded)`; `breadcrumb(parts)` with
  `.crumb_separator(..)` and `.crumb_suffix(..)` (a path's is
  `files::path_crumb`, below); `key_hints()`, a footer of the
  keys that work now; `cells([(35, a), ..])`, ratatui's one-row table;
  `live(fps_cap, || renderable)`, which draws at most so many times a
  second of the app's clock.
- **Tabs.** `.auto_hide()` hides a strip of fewer than two titles;
  `.on_select(|to, cx| ..)` hears the title picked instead of the strip
  setting its signal.
- **Clock.** `after(wait, |cx| ..)` (and `cx.after`) runs once on the
  app's clock; `Resource::stale_after` waits on it. `Driver::advance(d)`
  steps the clock to each thing due and draws. `DrawCx::now()` gives a
  widget the app's clock.
- **Keys.** `.bind_sequence_in(section, ..)`; `.bind_cycle_back(..)`
  cycles a mode backwards.
- **Tasks.** `spawn_then(work, done)` keeps a non-`Send` `done` on the
  app's thread; `spawn_stream(|stream| ..)` is a thread with a proxy
  counted as a task until it returns, told of a cancel.
- **`Node::on_unmount(|| ..)`** runs when a node leaves the tree.
- **`Widget::access_text`**: what a widget says to assistive technology,
  read as text.
- **`App::graphics(source)`** draws an rs-rich-ext `PlacementSource`'s
  terminal graphics beside the cells after each frame, and releases them
  at the end. The `image` node does not place graphics yet.
- **`render_with` needs no quit**: it presses its keys, settles and reads
  the screen; an app's error is its `Err`.
- **Fixed: an app dropped without `finish()`** (a driver let go) left its
  `latest` threads asleep on its clock; dropping an `App` now closes its
  runtime.

### intuiTUIve: runtime, keys and signals from round 2 (rs-rich-intuituive, interact)

What the round-2 rebuilds of scope-tui and Yazi found missing from the
core (see [the demo gaps](docs/design/intuituive-demo-gaps.md)).

- **`on_write(signal, |value, cx| ..)`** runs after every write to a
  signal, an equal value too, where a `watch` sees two equal buffers as
  one. **`rate(signal)`** is a `Signal<u32>` of writes a second, updated
  each second of the app's clock: a frame-rate meter.
- **`latest` threads on the app's clock.** `Latest::sleep(period)` keeps
  a thread to a beat, as `every` does, and **`latest_every(period, ||
  value)`** is a thread making a value every period. In an app that
  `wait_for_tasks(true)`, both keep to the app's clock, and each turn
  waits until the threads due have sent their values and sleep again, so
  a thread-fed app can be tested frame by frame. `latest` threads now
  start when the app starts running.
- **`App::try_new(|| -> Result<Node, E>)`**: a builder that can fail
  returns its error from `try_new`, before the terminal is taken.
- **Help sections and modifier variants.** `.bind_in(section, keys,
  description, handler)` lists a binding under a section of its own in
  the help and the palette. `.bind_scaled(keys, &[("", 1.0), ("shift",
  10.0), ..], description, |cx, value| ..)` binds keys with modifiers,
  each with its value, shown as one help row. `.bind_cycle(keys,
  description, signal, &modes)` steps a signal through modes.
- **`resource_for(key, fetch)`** fetches again whenever the key changes
  and drops results for keys left behind; **`Resource::stale_after(d)`**
  keeps the last value while the next loads, for `d` at most.
- **`Signal::replace(value)`** sets a value with no `PartialEq` and
  returns the old one. `RectHandle` and `PathWatch` are `PartialEq` (the
  same handle), so they can be kept in a signal with `set`.
- **`RectHandle::new()`** and **`Node::rect_into(&handle)`**: a handle
  made before its node.
- **Keys.** `comma` names the comma key wherever keys are parsed
  (`"comma s"`, `"ctrl+comma"`; rs-rich-interact's `Key::parse`). A
  `bind_sequence` of one key panics, saying to use `bind`, where it
  silently became a `bind`.
- **The headless record keeps arguments:** rs-rich-interact's
  `Record::handoff_commands` holds each handed-off program with its
  arguments.
- **Fixed: which-key columns ran into each other.** A description too
  long for its column is cut with `…` two cells short of the next
  column's keys. The theme's `which_key.desc` and `which_key.sep` style
  the descriptions and the separator.

### A file manager's components (rs-rich-intuituive `files`, rs-rich-ext)

Round 2 of [the demo gaps](docs/design/intuituive-demo-gaps.md) asked for
the parts of Yazi that `examples/files.rs` writes by hand. The UI parts
are in intuiTUIve's new off-by-default `files` feature
(`intuituive::files`); what needs no terminal is in rs-rich-ext, with no
new dependency outside its `archive` feature.

- **Miller columns:** `files::miller(start)`: the parent, current and
  preview columns over one cursor, in draggable panes, with remembered
  positions, clicks that navigate, Yazi's movement keys
  (`Miller::navigation`) and markers and git signs through
  `MillerOptions`. With `watch` both directories follow the disk.
- **A directory model:** `files::dir_model(..)`: entries as memos,
  sorted, hidden files shown or not, filtered (smart case), read again on
  `refresh()`; `.watch()` (`watch` feature) and `.sniff()`, which types
  files by their first bytes on a worker thread.
- **File-list rows:** `files::file_list(model, cursor, options)`: icon,
  type colour across the row, marker, find highlights with the hovered
  match's place, a git sign and Yazi's line modes (size, permissions,
  mtime, btime, owner).
- **A preview pane by type:** `files::preview(path, options)`: a
  directory's entries, highlighted text, an image (`art`), an archive's
  listing (`archive`), a hex dump; loaded off the app's thread, stale
  results dropped, custom previewers first.
- **Openers, jobs, a finder:** `files::open_with` runs an opener (the
  editor through a handoff, the system opener in the background, archive
  extraction); `files::tasks()` and `tasks_panel` run copies, moves,
  deletes, trashing and extraction as cancellable jobs with progress;
  `files::finder(walk, on_pick)` fuzzy-finds over a directory walk whose
  results stream in; `files::git_status`, `files::hex_view`.
- **rs-rich-ext:** `listing` (entries, Yazi's sort orders, line-mode
  cells, a lazy tree `walk`), `open` (Yazi's opener rules, MIME and name
  globs), `file_ops` (copy, move, delete, trash with progress, conflict
  rules, cancellation), `trash` (the FreeDesktop trash: put, list,
  restore, purge; an `Unsupported` error on macOS and Windows), `git`
  (`git status --porcelain=v1 -z` per file), `archive` (tar, tar.gz and
  zip listing and extraction that never writes outside its directory;
  the off-by-default `archive` feature brings `zip` and `flate2`, both
  already in the tree), and `filetype::mime_batch`.

### intuiTUIve's audio plugin, rounded out (rs-rich-intuituive)

Round 2 of [the demo gaps](docs/design/intuituive-demo-gaps.md) found what
the scope-tui rebuild still lacked from the `audio` feature.

- **Breaking: `audio::input` returns `Signal<Option<Frame>>`**, the shape
  of `latest`'s: `None` before the first buffer, then the newest `Frame {
  number, samples }`. Numbered, so a watch sees every buffer, digital
  silence included (frames per second now count silent buffers too). The
  scope example takes it so.
- **Devices by id:** `Device { id, name, default, host }`. `input` and
  `AudioOptions::device` take the id (ALSA's `null`, `pulse`,
  `hw:CARD=…`), then a name, exactly or in part; the name is the
  description to show. `Device::configs()` lists channels, rate range,
  buffer range and sample format, as scope-tui's `audio --list` does.
- **`audio::open(AudioOptions)` before the app**, so a missing device fails
  before the terminal is taken, with `capture.samples()` and
  `capture.status()` inside it. `AudioStatus` is `Running`,
  `Stalled(since)` (no buffer for `stall_after`) or `Failed(why)` (cpal's
  stream error; glitches and reroutes are not failures): enough to enforce
  `--timeout`. `AudioOptions::latency(frames)` sets the driver's or
  server's buffer.
- **ALSA keeps quiet:** its `ALSA lib …` lines are taken by a thread-local
  handler (`snd_lib_error_set_local`) while devices are listed and opened,
  kept for `audio::messages()`, and passed to a `Log` with
  `capture.log(log)`.
- **PulseAudio and PipeWire:** the new `pulse` feature turns on cpal's own
  PulseAudio client (pure Rust, no libpulse): `Host::Pulse` lists every
  source, a sink's monitor among them, and `devices_on(host)` lists a
  host's inputs.
- **`audio::pcm(reader, Format, channels, buffer)`** reads interleaved raw
  PCM (`u8`, `s8`, `s16le/be`, `s24le`, `s32le/be`, `f32le/be`, `f64le`;
  `Format::parse("S16_LE")`) as buffers, behind the light `pcm` feature
  (below). The new `wav`
  feature (hound) adds `audio::wav(path, buffer)`, `audio::wav_writer` and
  `audio::record(samples, path, rate, channels)`.
- **`audio::picker(on_pick)`**: a list of the inputs, the default selected,
  Enter picks, `r` lists again.

### intuiTUIve: meters, sparklines and big text (rs-rich-intuituive)

- **`meter(level, peak)`**, a `LevelMeter` node; `.hold(duration)` keeps
  the highest peak marked for that long. `meter_with(.., configure)` sets
  its label, readout, decibel scale or orientation.
- **`sparkline(values)`** (and `sparkline_with`): rs-rich-ext's
  `Sparkline` over a closure that reads signals.
- **`big_text(markup)`** behind the `art` feature: markup in FIGlet
  letters through rs-rich-art, each run in its style.

Each reads aloud as its chart does.

### Sound and charts (rs-rich-ext, rs-rich-ratatui)

- **`dsp`:** test signals (`tone`, `partials`, `noise` with
  `.low_pass(..)`, `mix`), `trigger(samples, level, depth, Edge)`,
  `note_hz("A4")` and `nearest_note(hz)`, and `spectrum_full` (every bin,
  as scope-tui plots them).
- **`chart::Axis::grid([..])`**: grid lines at those values, in the axis's
  style, under the datasets (the spectroscope's 29 reference datasets
  become one call).
- **`chart::Chart::style(..)`**, ratatui's `Chart::style`: under every
  cell, and the legend's box takes it by default; `legend_style` still
  wins.
- **`chart::Spectrogram`**: a waterfall of spectra, the newest at the top,
  in a heatmap's shades, on a linear or log frequency axis.
- **`chart::LevelMeter`** and **`chart::PeakHold`**: an audio level meter,
  green to red, with peak and held-peak marks, linear or in decibels,
  across or up.
- **`palette::style_from_color_name`** moves here from rs-rich-ratatui,
  which re-exports it (its path and behaviour are unchanged, and a test
  checks it against ratatui's parser); rs-rich-ratatui now depends on
  rs-rich-ext without its default features.

### intuiTUIve's scope-tui example on round 3's components (rs-rich-intuituive)

Round 3 of [the demo gaps](docs/design/intuituive-demo-gaps.md):
`examples/scope.rs` rebuilt on the audio, DSP, chart and key components
round 2 asked for, with no hand-rolled stand-in left. Sources are
`latest_every`, `latest` with `Latest::sleep` and `audio::open`'s capture,
taken with `on_write` and counted with `rate`; files are read with
`audio::pcm`; the test signal, trigger and `--tune` are `dsp`'s; reference
lines are `Axis::grid`, the header `cells`, the keys `bind_cycle`,
`bind_scaled` and `bind_in`. New and more faithful: `audio --timeout`
ends the app as scope-tui does, `audio --list` prints each device's
configurations in scope-tui's format, and `pulse` reaches every
PulseAudio source with the `pulse` feature, `--server-buffer` included.

### intuiTUIve's Yazi example on the new components (rs-rich-intuituive)

Round 2 of [the demo gaps](docs/design/intuituive-demo-gaps.md):
`examples/files.rs` rebuilt on round 1's additions, with every workaround
gone. One lazy list per column with the line mode trailing and a marker
column; Yazi's key sequences and which-key; prompts placed with
`popup_at` (rename below the hovered row, the caret before the
extension); `cx.confirm`, `cx.notify` at the top right; rs-rich-ext's
`filetype` icons and type colours, local times and owner names; Enter and
`o` open with `$EDITOR` through `cx.handoff` or the system opener, `O`
picks one. With the crate's `watch` feature it follows the disk, with
`art` it draws image previews; CI's tapes job builds the examples with
both. The example's tests cover the new behaviour.

### Releases: one release, one commit, found for you

- **`release_cohort.py tag` finds the release and its commit.** A release
  is named by rs-rich-cli's version. With no `--commit`, the newest release
  that has tags is resumed on the commit those tags are on while any of it
  is unpublished; with nothing in progress, `HEAD` is released. On today's
  repository it picks release 0.0.19 at 2fc9772 by itself and tags only web
  0.0.2, data 0.0.3, cli 0.0.19 and python 0.0.7.
- **`--release 0.0.20`** (or `v0.0.20`) releases that one: from the commit
  its tags are on, else the last commit on main naming it.
- **Tags record their release and commit.** Each tag's message carries
  `Release:` and `Commit:` lines.
- **The release workflows refuse a split release.** `release.yml` and
  `pypi-release.yml` run `release_cohort.py same-commit <tag>` before
  building: a tag whose commit ships other code than a package of its
  release tagged elsewhere under the same number is refused, and the job
  summary names the tag's commit and release.
- **The split check counts tagged packages, not only published ones.** A
  tag on another commit still running, or one that started publishing,
  holds the release to that commit unless it ships the same files (then it
  is waited on).
- **A tag pushed again starts a fresh run.** A dead tag on the release
  commit itself is pushed again, so re-running `tag` retries a release run
  that failed before publishing; a release held only by dead tags is not
  resumed. After moving or re-pushing a tag, the wait ignores the old run,
  where it used to stop at once on its failure.
- **Template.** `templates/intuituive-app` depends on rs-rich-intuituive
  0.0.5, which #753 missed (main's interact and intuiTUIve test jobs).

### Releases: a dead tag is moved, not a dead end

- **`release_cohort.py tag` moves a tag whose release never happened.** A
  tag on another commit, for a version that is not on its registry, whose
  latest release run ran on that commit, ended without success (cancelled
  or failed) and never started its publish job, is deleted and made again
  on the commit being released; it used to stop with "resolve it by hand".
  Anything less certain (a run still going, one that started publishing, a
  run of another commit, or no run to see) still stops, now with the
  commands to clear it. A local tag on
  another commit that the remote does not have (a remote tag deleted by
  hand leaves one) was never released, so it is replaced too, where it
  stopped with "delete it first".
- **`release_cohort.py tag --from <package or tag>`** starts later in
  `RELEASES.toml`'s order and leaves the packages before it alone. It
  refuses, before tagging anything, when a later crate depends on one of
  them at a version not yet on crates.io, naming each, since that crate's
  publish would fail. Without it, `tag` already begins at the first
  unpublished package.
- **`release_cohort.py tag` refuses a split cohort.** Before tagging, it
  compares each published package's shipped files (by its `include`) at the
  commit its tag is on with the commit being released; a package changed
  under the same version stops the run, naming the package, the files and
  the commit it was released from, with `--commit <that sha>` or a bump as
  the way on. It would have stopped the run that put rs-rich-ext 0.0.17 on
  #752 beside rs-rich-intuituive 0.0.4 from #751. `--commit` takes any
  commit on main, such as an earlier one known to be good.

### Releases: tag from the local checkout; versions after a split cohort

- **`release_cohort.py tag` releases the local checkout.** It tags `HEAD`
  (or `--commit`), which must be on `main` as last fetched, and fetches
  nothing: `RELEASES.toml` and the manifests come from that commit. A run
  resumed after `main` has moved on releases the same commit, where it used
  to fetch and tag the newer `origin/main`, splitting a cohort across two
  commits.
- **Versions.** rs-rich-intuituive 0.0.4, rs-rich-ratatui 0.0.4,
  rs-rich-interact 0.0.7 and rs-rich-embed 0.0.2 were published from commits
  before #752, which changed them under the same numbers. They move to
  rs-rich-intuituive 0.0.5, rs-rich-ratatui 0.0.5, rs-rich-interact 0.0.8 and
  rs-rich-embed 0.0.3, so every published version matches its registry
  package again.

### intuiTUIve: list and table rows (rs-rich-intuituive)

What rebuilding Yazi and scope-tui found the lists and tables lack
(round 1 of [the demo gaps](docs/design/intuituive-demo-gaps.md)).

- **Row controls on `list`.** `.trailing(|i| markup)` puts right-aligned
  markup at the right of a row, inside its style and highlight, and cuts
  the item short of it. `.marker(|i| Option<style>)` keeps a one-cell
  column at the left, outside the highlight, that never moves the text.
  `.selected_style_for(|i| Option<style>)` is the highlight's style per
  item. Yazi's names and line mode are one list now, not two on one
  cursor.
- **`list_lazy(len, item, selected)`** asks only for the items it draws,
  with every list option; `list`'s closure builds every item each draw.
- **A table without a header:** `TableOptions::header(false)`. A table
  also takes `TableOptions::offset(signal)`, and
  `Column::overflow(Overflow::Crop)` cuts its cells at the edge with no
  `…`, as ratatui's do.
- **`virtual_list` options:** `.scroll_padding(n)`, `.row_style(..)` and
  `.offset(signal)` work on a `virtual_list` (and any table) as on a list.
- **`widgets::cell(markup)`**: a text that keeps one line and is cut at
  its edge, ratatui's table cell, for a `row` of cells as a one-row table.
- **The screen rect of a row:** `.row_rects(&RowRects)` on a list, a
  table or a `virtual_list` records where each row in view was drawn, so
  a popup can open at one (`rects.get(i)`).
- **Fixed: a `list` sized `Size::Auto` took the full width.** It measured
  as wide as it was offered, so `row([list(a).flex(1), list(b).auto()])`
  gave everything to the second. It is as wide as its widest item now,
  with its symbol, marker, trailing text and scroll bar.

### Charts drawn as ratatui 0.29 draws them (ext)

Round 1 of the intuiTUIve demo gaps (see
[what the rebuilt demos found missing](docs/design/intuituive-demo-gaps.md)):
four `Chart` options the scope-tui clone needs to look like ratatui 0.29's
`Chart`. Each default is the drawing as before.

- **`out_of_bounds(OutOfBounds::Drop)`** leaves out a line, bar or area
  segment with an end outside the bounds, as ratatui 0.29 does;
  `OutOfBounds::Clip`, the default, draws the part inside.
- **`layering(Layering::PerDataset)`** draws each dataset over the ones
  before, as ratatui 0.29 draws a canvas per dataset: a cell a later
  dataset draws in shows only its dots, so scope-tui's reference lines
  break where the trace crosses them. `Layering::Merge`, the default,
  shares the dots of a cell.
- **`legend_style(..)`** gives the legend's box a style of its own; by
  default it takes the x axis's.
- **`legend_clear(false)`** leaves what the datasets drew under the
  legend, unstyled around the names, as ratatui does; by default the box
  clears it.

### The outside world in intuiTUIve: files, sound and images (intuituive 0.0.4, ext 0.0.17, ratatui 0.0.4)

What the Yazi and scope-tui rebuilds found missing from the plugin side
(`docs/design/intuituive-demo-gaps.md`, "Data, sources and the outside
world"). Every new dependency is behind an off-by-default feature.
rs-rich-micro 0.0.6 and rs-rich-record 0.0.8 change only their
requirement on rs-rich-ext 0.0.17.

- **intuiTUIve `watch` feature: `watch_path(path, |event, cx| …)`** wakes
  the app when files change, through notify (the version the CLI already
  uses), with no polling. Changes less than 100 ms apart are one
  `PathEvent` (each path once: `Created`, `Modified`, `Removed`,
  `Renamed`); `watch_path_with` takes `WatchOptions` (`recursive()`,
  `debounce(d)`). It runs as long as the app, or until `PathWatch::stop`.
- **intuiTUIve `audio` feature: `audio::input(device, rate, channels,
  buffer) -> Signal<Matrix>`** captures a sound card through cpal 0.18,
  one row a channel; the newest buffer wins, so a slow frame drops
  buffers instead of falling behind. A device with fewer channels than
  asked for is spread over the rows. `audio::devices()` lists the inputs
  and is empty, not a panic, without sound. On Linux it needs ALSA's
  headers (`libasound2-dev`).
- **intuiTUIve `art` feature: `image(path)`**, `image_with(path,
  ImageView)` and `image_of(|| path, view)`: an image file as a node, drawn
  by rs-rich-art in half blocks (or quadrants, Braille, ASCII) to fit its
  rectangle, with `ImageFit::Contain`, `Cover` and `Stretch`; decoded on a
  worker thread and drawn again only when its size changes. Kitty and
  Sixel are not drawn: the cell screen cannot pass their escapes through
  yet.
- **`rich_ext::filetype`**: `icon_for(path, is_dir) -> (glyph, colour)`
  and `icon_with(path, FileFlags, Appearance)`, Yazi's default icon and
  colour rules (its dark and light presets, from nvim-web-devicons, both
  MIT); `style_for(mime, flags)`, its type colours; `mime_for`,
  `mime_for_path` and `mime_from_bytes`, MIME types by extension and magic
  bytes; and `nerd_font()`, which says honestly what can be known
  (`RICH_NERD_FONT`, or a Nerd Font file installed). No dependencies.
- **`rich_ext::fs`**: `user_name(uid)` and `group_name(gid)` (Unix,
  `getpwuid_r`/`getgrgid_r`, cached; `None` elsewhere), and with the new
  off-by-default `local-time` feature (chrono, already in the tree)
  `local_time(time, format)` and `short_local_time(time)`.
- **`rich_ext::dsp`**: `fft` of any length (radix 2, and Bluestein's chirp
  otherwise), `hann` and `spectrum(samples, rate)`, the scope example's
  tested FFT lifted into the library.
- **`rich_ext::syntax::preload(langs)`** warms core's default highlighter
  (syntect's grammars, themes and each language's patterns) on a worker
  thread, so an app's first Markdown preview does not stall for seconds in
  a debug build. It goes through the public `CodeHighlighter` API; core is
  unchanged.
- **`rich_ratatui::style_from_color_name("DarkGray")`**: a colour as
  ratatui's `Color::from_str` reads it (names in any case and spacing,
  `#rrggbb`, an index) as a rich style (`bright_black`).
- **CI** installs `libasound2-dev` for the jobs that build every feature
  (the feature matrix's `--all-features` builds, the MSRV check and the
  staged package build).
- **Docs:** "Files, sound and images" in the intuiTUIve guide, a new ext
  page, [Files, spectra and warm-ups](docs/guide/ext/files-and-signals.md),
  and the colour names on the ratatui page.

### Keys, pop-ups, chrome and the runtime, from the Yazi and scope-tui clones (intuituive, interact)

What round 1 of rebuilding Yazi (`examples/files.rs`) and scope-tui
(`examples/scope.rs`) on intuiTUIve found missing
(`docs/design/intuituive-demo-gaps.md`).

- **Key sequences:** `.bind_sequence("g g", description, handler)` and
  `.on_sequence`. A prefix waits for the next key; Esc cancels a sequence
  under way, and any other key cancels it and is then handled as usual.
  `App::which_key(true)` (or `which_key_delay(d)`) shows what can follow
  in up to three columns at the bottom of the screen. The help and the
  command palette list sequences as `g g`.
- **Any key:** `.on_any_key(|cx, key| -> bool)` takes what a node's
  bindings leave.
- **Keys a container sees first:** `.previews_keys()` on a column or any
  node: its bindings, sequences and `on_any_key` run before the focused
  node inside it gets the key.
- **The caret of an input:** `.caret(Caret::Start | End | At(n))` and
  `.select(range)` on an `input` or a `text_area`. Fixed: a prefilled
  input put its caret at the start (`input(signal("alpha".into()))`, then
  `x`, gave `xalpha`); an input's caret now goes to the end when it is made
  and when the signal is set from elsewhere. A text area keeps the start.
- **Fixed: bindings inside a pop-up's panel never fired.** With nothing
  focused, keys go down from the root through every node that holds one
  child, so the content's own bindings run.
- **The palette** shows the keys of a binding with a long description (an
  `esc` binding's keys were dropped): the description is cut instead
  (interact). `rich_interact::keymap::Binding::sequence(..)`, and a
  `sequence` field on `Binding` and `overlay::Command`, show keys pressed
  one after another as `g g`; keymaps do not look them up (interact).
- **`popup.title`:** a modal's or pop-up's panel title is drawn in the
  theme's `popup.title`, else its `title` (so nothing changes unless a
  theme sets it).
- **Ratatui's solver:** `.solver(Solver::Ratatui)` (or `RatatuiLegacy`) on
  a row or column, and `StackLayout::solver` for `layout::split`, lays the
  children out as ratatui's `Layout` does: percentages of the whole length
  and what does not fit squeezed by the same constraints through kasuari,
  ratatui's solver (a new dependency). Off by default; existing layouts
  are unchanged. Checked against ratatui over widths 10 to 200 for
  scope-tui's header and over every `Flex`.
- **Where a node is:** `node.rect_handle()` gives its rectangle on the
  screen after each frame, as a handle whose `get()` subscribes.
- **Pop-ups from a point of the screen:** `cx.popup_at(Origin, Offset,
  build)` (and `popup_at_with`): a corner, an edge's middle, the centre or
  below a rectangle, moved by a signed offset and kept on the screen.
- **A rule joined to its panel:** `rule()` draws a line across that joins
  the border of the panel it spans (`├──┤`). Fixed: `.borders(Sides::..)`
  on a node that is not a panel drew nothing; it now puts the node in a
  panel without a title with those sides.
- **Notifications:** `cx.notify(Level::Info | Warn | Error, title, body)`
  and `notify_for`: a box with the title on its edge, in the level's style
  (`notify.info`, `notify.warn`, `notify.error`, else `accent`, `warn`,
  `bad`), announced with its level. `App::toasts_at(Corner::..)` stacks
  toasts in another corner (Yazi's top right). `cx.toast` is unchanged.
- **A confirm dialog:** `cx.confirm(title, body, items, on_yes)`, centred,
  with the items and `[Y]es`/`(N)o` (y, n, Enter, Esc, Tab, the arrows and
  clicks).
- **The newest value from a thread:** `latest(|tx| ..) -> Signal<Option<T>>`;
  `Latest::send` keeps only the newest value, and returns `false` once the
  app has finished.
- **Handing the terminal to a program:** `cx.handoff(command)` and
  `handoff_then(command, |result, cx| ..)`: `App::run` leaves the
  alternate screen and raw mode, runs it, takes the terminal back and
  redraws. `Driver::take_handoff` and `Driver::handed_back` for a loop of
  your own; the headless driver records it.
- **Docs:** the intuiTUIve guide covers each of these.

## 0.0.19 cohort — published 2026-10-19

ext 0.0.16 / micro 0.0.5 / record 0.0.7 / interact 0.0.7 / ratatui 0.0.4 /
intuituive 0.0.4 / embed 0.0.2 / web 0.0.2 / data 0.0.3 / CLI 0.0.19, and
`rs-rich` 0.0.7 on PyPI. Published crate by crate, tag by tag: ext, micro,
record and interact from `22da468`, the rest and `rs-rich` 0.0.7 from
`2fc9772`, the cut the CLI's tag is on.
Unchanged and already published: core 0.0.9, macros 0.0.3, plugin API 0.0.3,
art 0.0.12, diagram 0.0.2, mermaid 0.0.5, lumis 0.0.3. See
[the release notes](docs/releases/0.0.19.md).

### 0.0.19 release test: fixes from three audits

Three independent audits of the 0.0.19 delta (intuiTUIve and the surface;
process isolation; Python, docs and packaging). Each defect was reproduced
first and has a regression test. The isolation fixes are in [One process
per browser session](#one-process-per-browser-session-web-002).

- **A link cannot drive the terminal.** A style's link URL went into OSC 8
  as it was, so one holding `ESC \` ended the hyperlink early and the rest
  ran as terminal controls, past the surface's promise that nothing drawn
  can drive the terminal. A link holding a control is dropped.
- **Drawing over half of a wide character keeps the surface and the
  terminal in step.** The half outside the rectangle stayed in the model
  while the terminal erased it, and the diff never repaired it (an overlay
  over CJK text, overlapping rectangles). That half becomes a space, and
  each paint looks one column past what changed.
- **An inline surface fits the terminal.** One taller than the terminal
  lost rows and wrote them out of order; it now takes at most the
  terminal's rows, and again when the terminal shrinks.
- **Moving through a long line is linear.** Up and Down took 23 s, and
  Ctrl+Right 46 s, on a 20,000-character line: each step measured the
  line again from its start. Both now walk it once (5 ms and 8 ms).
- **A character limit costs a keystroke only the lines it touches.** It
  copied and counted the whole buffer (776 ms a key at 100,000 lines); the
  buffer now keeps its count as it changes (65 µs).
- **The caret never stops inside a cluster.** Typing before a combining
  mark joins the two, and the caret was left between them, so the next
  character took the mark. It now goes after the cluster.
- **A keystroke in a long wrapped line costs the same at any height.**
  Following the caret wrapped the whole line again for every row of the
  view (1.25 s a key at 48 rows); each line is wrapped once.
- **The mouse wheel does not jump back in a long last line.** Scrolling
  down inside it went back to its first row.
- **Wrapped rows lay out as CSS does.** A percentage was taken of the
  container to break lines and of the line to size items, so items in one
  container came out at different widths; it is the container's now. A
  flexible item keeps the content its line was broken for before sharing
  what is left, and `align_self` survives `.padding()` and `.panel()`.
  Tens of thousands of items no longer overflow the spacing arithmetic.
- **Python's `Surface` keeps what a failed write did not send**, so the
  next `draw` or `finish` sends it, and a retried `finish` leaves the
  alternate screen. A surface still open when the program ends (at module
  level, on Ctrl+C) gives the terminal back through `atexit`. The stub
  names `draw`'s argument `f`, as the method does.
- **Docs:** the Python guide lists `Widget::claims_key` and
  `MeasureCx::stack_flow` among what is not bound yet.

A second round of audits, over workstreams 7 to 10 (a session's process
tree on Windows; responsive layout, the error state and snapshot tests;
wide and long tables; reorderable lists and the date and time pickers),
found the defects below. Each was reproduced first and has a regression
test.

- **A hidden node is hidden in every container.** A node hidden by
  `hide_below`, `show_below` or `@media { display: none }` left the Tab
  order, but inside a `split`, a `scroll` or a `switch` it was still drawn
  and still took clicks. Now it is drawn nowhere and takes no clicks, and
  the accessibility tree and linear mode leave it out, whatever holds it.
  A hidden pane leaves the other the whole split, with no divider, and a
  hidden row of an `each` takes no room. A page that a `switch` or an
  `each` builds takes the stylesheet before it first draws, so
  `display: none` holds there from the start.
- **Typing an hour into a time picker keeps its minutes.** With 09:00 the
  earliest and 09:05 shown, typing `1` wrote 01:05, which the range moved
  to 09:00, and the `4` then made 14:00; the app's signal saw 09:00 on the
  way. A first digit that would leave the range is no longer written
  until the field is complete or left. A field typed past the range stops
  at the range's end for that field and keeps the others where it can:
  `08` typed into 12:45, with 09:30 the earliest, makes 09:45. Beside a
  calendar, the line under the picker says why a field moved.
- **A calendar or a time picker whose min is after its max says so.**
  Every day or time was disabled with no word why, and the first key
  moved the value to the max. Now "the range is empty: min is after max"
  shows where the calendar's weeks go, after a time picker's fields and
  under a date-and-time picker, as rs-rich-interact's pickers say it, and
  nothing changes the value. Assistive technology is told it is in error.
- **A table column squeezed to nothing can be dragged wider again.** The
  check that keeps a column scrolled out of view from being resized
  applied to every table, so in a resizable table that fits, a column
  squeezed to no cells had no edge to drag. It applies only while the
  table scrolls across.
- **`@media(max-width: 80)` parses.** A query with no space after
  `@media` was refused with "only `@media` rules are supported". It is
  accepted, as in CSS.
- **A stylesheet with more than 64 `@media` blocks follows the
  terminal.** The 64th block and those after it shared one bit of the key
  that tells when a resize crosses a bound, so crossing a later block's
  bounds did not apply it. Each block now counts on its own.
- **A snapshot file saved with a byte-order mark still matches.** The
  mark was compared as text, so the file never matched, and the diff
  showed two lines that looked the same. It is ignored, as line endings
  and trailing spaces are.
- **The wheel carries a grabbed item, as the arrows do.** With an item
  grabbed, the wheel moved only the focus, so the item focused next showed
  as carried and Esc moved it into the grabbed one's place. The wheel is ↑
  and ↓ now, so Esc puts back the item that was grabbed.
- **A click on a reorderable list lands on the row under it.** Clicks and
  drags counted rows from the offset the last key left, not the one drawn,
  so after `focus(..)` scrolled the first page, or after the terminal grew,
  a click moved another item. Drawing and the mouse share one offset.
- **A new order typed as numbers is read in one pass.** Each number was
  checked against all those before it: 40,000 took 18 s in a debug build.
  They take 0.15 s now.
- **Tab leaves a time picker in a container.** `TimePicker` and
  `DateTimePicker` took every Tab and went round their fields, so in a
  `Column` the focus never reached the next field. Tab and Shift+Tab are
  the containers' `focus-next` and `focus-previous` now (the `date`
  keymap's `next-field` and `previous-field` are renamed): after the last
  field the focus moves on, and Shift+Tab comes back in at the last. On
  its own a picker still goes round.
- **Sticky lines a page cannot hold are still shown.** On a page shorter
  than them, the viewport kept the first that fit and never showed the
  rest: `rich pager --sticky 6` in a six-row terminal never showed lines 5
  and 6, a search found them without showing them, and the status line
  counted lines that were not on screen. The last that fit stay now (a
  table's column names) and the ones before scroll away first, so every
  line is shown. `rows`, `visible`, `status`, `show` and the pager's search
  agree, and `Viewport::top` says where the scrolling lines start.
- **A table without a header has no header pinned.** A box such as
  `SQUARE` draws the line between rows as it draws the one under a header,
  so a headerless table with `show_lines` (which rich draws in `SQUARE`)
  had its first row pinned by `rich pager`. Such a line marks a header
  only when it does not come again now. A header with a line of its own,
  as in rich's default `HEAVY_HEAD`, is still found.
- **Without seconds a picker answers what it shows.** A time picker showed
  14:30 and answered 14:30:45, and a limit with seconds refused the value
  it named: `rich date --kind time --min now` refused 01:22 with "the
  earliest is 01:22". Without seconds the pickers work to the minute now,
  whatever order the builders come in. The start, the default and a typed
  answer drop their seconds; the earliest rounds up to a minute and the
  latest down. `rich date` checks `--default` the same way.
- **A default the picker would refuse is no answer.** Without a terminal,
  an empty line, or a run asked for defaults, answered the default even
  outside `min` and `max` or refused by the validator. It is checked as a
  typed answer is now. Python's pickers raise `ValueError` for a default
  outside `min` and `max`, as `rich date` refuses one.
- **A range the wrong way round says so.** A `min` after the `max` was
  taken silently, and each answer was refused naming one limit. Every
  answer is refused with "the range is empty" now, and
  `rs_rich.tui.time_picker` and `datetime_picker` raise `ValueError`, as
  `calendar_with` does.
- **`--value` is where `rich date` starts, with `--default` or without.**
  The default set the start too, so `--value 2026-10-01 --default
  2026-10-17` opened on the 17th. An explicit value wins, in either order,
  in the library and in Python too.
- **`2026-10-17T2:30 pm` parses.** A date and time were split at the first
  space, inside the time; the first `T` or space after the date splits
  them now.
- **A year is four digits.** `26-10-17` parsed as the year 26. A date needs
  four year digits, 0001 to 9999, and the pickers stay within those
  years, so every answer prints and parses back.
- **Python's pickers stay within Python's dates.** At `date.max`, a day on
  raised `ValueError: year 10000 is out of range`, and `0000-01-01` typed
  raised it rather than `NotInteractive`. The days stop at the years 1 and
  9999 now.
- **Python's pickers refuse a value with a time zone.** One kept its clock
  and lost its offset, and a `datetime_picker` rewrote the caller's signal
  without it. It is a `TypeError` now. The docs say the pickers drop
  microseconds.
- **`help()` shows the pickers' prompts.** `DatePicker`, `TimePicker` and
  `DateTimePicker` showed `prompt=Ellipsis` in their signatures.
- **`Pager(sticky_header=-1)` is a `ValueError`**, not a `TypeError`.
- **`rich date --seconds` and `--twelve-hour` need a time.** With
  `--kind date` they were ignored; they are a usage error (exit 2) now.
- **CI: every command of the Windows web job counts.** Its step ran two
  commands in one PowerShell step, where only the last one's exit code
  failed it. Each has a step of its own now.
- **What a session runs on Windows serves as it would anywhere.** A
  program the app ran that serves an app too took the inherited
  `RS_RICH_WEB_SESSION` for its own and exited: Unix tells the two apart
  by process group, Windows had nothing to. The session's process takes
  the variable out of its environment before the app runs.
- **CI: every command of the Windows pager job counts.** It ran three
  commands in one PowerShell step, where only the last one's exit code
  failed it; the step runs in bash now.

A third round of audits, over the CLI's shell integration, `rich jump`,
`rich log` and `rich deps`, found the defects below. Each was reproduced
first and has a regression test.

- **`rich deps --save` and `--baseline` may name one file.** The report
  was saved before the baseline was read, so the comparison was against
  itself and a regression passed. The baseline is read first now.
- **`rich jump` no longer suggests `eval`.** Its help and docs piped the
  place to `eval`, where a file named `$(…)` would run. `--print shell`
  prints a safely quoted command instead: `cd -- 'DIR'`, `${EDITOR:-vi}
  +LINE -- 'FILE'`, or a `>` command as it is.
- **zsh prompts keep a link's `%`.** `rich shell link --prompt zsh`
  doubled `%` only outside escape sequences, so a URL's `%20` was expanded
  by the prompt; it is doubled inside them too.
- **A directory name cannot drive the terminal from the prompt.** The init
  scripts wrote `$PWD` into OSC 7 raw (zsh's `print` even read its
  backslashes as escapes); every shell percent-encodes it now, and zsh
  uses `print -rn --`.
- **Huge durations are usage errors, not panics.** `rich shell status
  --duration`/`--min-duration` and `--gif`'s `--speed`, `--fps` and
  `--max-duration` are bounded (exit 2 outside), and playback delays are
  computed without panicking.
- **`rich banner --width` is at most 10000.**
- **Python's `Jump` rejects two sources with one id** (a `ValueError`): the
  answer's value was looked up by id, so the second source's pick gave the
  first source's value.
- **`rich log --output jsonl` keeps late continuation lines readable.**
  Lines after a record written on a quiet spell are a
  `{"continuation": [...]}` object, and reading JSONL back folds such an
  object, and a record's `continuation` list, into continuation lines.
- **`rich log` sanitizes by default**, as `rich view` does: a log is
  untrusted, and an OSC 52 sequence in a field reached the terminal.
  `--no-sanitize` turns it off.
- **Sanitizing a log record covers its keys**, nested map keys included
  (`Record::sanitize`).
- **`rich log --follow` holds at most 1 MiB of an unfinished line** (more
  is shown in pieces, with a notice) and scans each byte for a line break
  once, not on every read.
- **`rich jump --recent` keeps control characters out.** A place with a
  newline split the recent file's entry in two, and an escape reached the
  terminal: such a place is printed only by `--print json`, never
  remembered, and skipped when read.
- **PowerShell's status line shows the whole command.** The hook passed
  `--command $last.CommandLine`, which legacy argument passing split at
  quotes; it hands it over in `RICH_SHELL_COMMAND`, which `rich shell
  status` reads when `--command` is not given.
- **bash's status line never names the wrong command.** A command history
  left out (`HISTCONTROL=ignorespace`) showed the one before it; the hook
  passes `--command` only when the history number moved on.
- **`rich shell link --tilde` matches a home reached through a symlink**
  (macOS's /var is /private/var), and a bare `rich shell link` shows the
  shell's `$PWD` when it names the working directory.
- **`--watch-interval` is at most 86400 seconds**, so a huge value is a
  usage error, not a panic.

The same round, over the libraries:

- **Documents cannot exhaust memory or the stack.** man's `\*` strings
  expanded each reference again (16 references nested eight deep is about
  10⁹ calls); a string is not expanded inside itself now, and a line takes
  at most 64 KiB of them. AsciiDoc attributes are cut at 64 KiB, with a
  warning. An Org `#+BEGIN:` with no `#+END:` recursed once per line; it
  is a keyword line now, and a closed one has the nesting limit other
  blocks have.
- **Documents keep raw control characters off the terminal.** An escape
  in an Org, AsciiDoc or man file is sanitized as it is read; tab and
  newline stay.
- **`LogView` keeps at most 1,000 continuation lines a record** and
  counts the rest (`… N more lines`); it builds only the detail rows that
  fit the page, and a time far before the first record no longer
  overflows the relative clock.
- **A horizontally scrolled table with zero-width columns** no longer
  underflows (intuiTUIve).
- **`SmallMultiples::gap`** is clamped like `bar_gap`, and its layout
  arithmetic saturates.

### Flex alignment in intuiTUIve (intuituive 0.0.4)

0.0.19 workstream 1.

- **Across the axis:** `.align_items(Align::Start | Center | End |
  Stretch)` on a row or column, and `.align_self(..)` on a child, which
  wins. `Stretch` is what rows and columns did before, and stays the
  default.
- **Along the axis:** `.justify(Justify::Start | Center | End |
  SpaceBetween | SpaceAround | SpaceEvenly)` shares the room the children
  leave; a flexible child takes it, as in CSS. Docked children keep to
  their edges and the rest are justified between them.
- **Wrapping:** `.wrap()` moves children that do not fit to a new line,
  each line solved on its own (a flexible child starts from its content,
  within its clamps); `.align_content(..)` places the lines, `Stretch` by
  default. A wrapping row with `.auto()` is as tall as its lines.
- **In the stylesheet**, with CSS's names and values: `align-items`,
  `align-self`, `justify-content`, `flex-wrap` and `align-content`. Code
  wins over the sheet. The inspector shows a stack's alignment and a
  child's `align-self`.
- **`MeasureCx::stack_flow`** lays out a custom widget's children with a
  `StackLayout` (gap, alignment, wrapping), as a row or column does;
  `stack` stays as it was.
- **Python:** `Node.align_items`, `align_self`, `justify`, `wrap` and
  `align_content`, taking the stylesheet's CSS names.
- **Docs:** "Alignment and wrapping" in the intuiTUIve guide, with the new
  `align` example and its tape (also on the recordings page).

### Positioned drawing: `rich_ext::surface` (ext 0.0.16)

0.0.19 workstream 4.

- **`Surface`**: render any rich renderable into rectangles of the terminal
  and write only the cells that changed. `surface.draw(|s| { s.render(rect,
  &renderable); s.set_cursor(x, y) })` draws a frame; cells nothing drew
  keep what they showed, and unchanged cells are never written.
- **Full screen or inline.** `Viewport::Fullscreen` draws on the alternate
  screen; `Viewport::Inline(rows)` takes rows below the cursor and leaves
  the last frame in the scrollback. `resize` keeps what was drawn and
  writes everything once. `finish` (or dropping the surface) gives the
  terminal back.
- **Synchronized output** (`synchronized(true)`, DEC mode 2026) and
  **graphics** from a `PlacementSource`, redrawn when the cells under them
  are repainted.
- **intuiTUIve draws with it.** `rich_intuituive::screen::{Screen, Painter,
  Rect, Cell}` are now re-exports of `rich_ext::surface`'s types, names
  kept; every intuiTUIve test and the ratatui benchmark gate pass
  unchanged. `Painter::repaint` writes cells whether they changed or not.
- **`rich_ext::surface::sanitize_line` and `control_picture`** make terminal
  controls in drawn text visible; `rich_interact::paint::sanitize_line`
  now calls them.
- **Python:** `rs_rich.ext.surface` (`Surface`, `Drawing`, `Rect`), with
  `file=` for any stream and a context manager. **Docs:** a new guide page,
  [Positioned drawing](docs/guide/ext/surface.md), with a ratatui
  comparison, and a section on the Python layout page.

### A text area in intuiTUIve (intuituive 0.0.4, interact 0.0.7)

0.0.19 workstream 2.

- **`text_area(text: Signal<String>)`**: a multi-line editor kept in step
  with its signal both ways. Builders: `.language("rust")` highlights it
  through the console's code highlighter (syntect, or lumis when
  registered), `.line_numbers()`, `.soft_wrap(false)` to scroll long lines
  sideways, `.read_only()` and `.placeholder(..)`.
- **Editing:** a grapheme-aware caret, word moves, Home/End and
  PageUp/PageDown; Shift selects with any move, Ctrl+A selects everything;
  Tab and Shift+Tab indent and outdent the selected lines (Escape then Tab
  moves the focus on); Ctrl+C, Ctrl+X and bracketed paste through the
  clipboard (OSC 52); Ctrl+Z, Ctrl+Y and Ctrl+Shift+Z undo and redo, a
  typed run as one step. The mouse places the caret, drags a selection,
  and double and triple clicks select a word and a line.
- **Large text:** only the lines in view are wrapped, highlighted and
  drawn; a test pages through 100,000 highlighted lines.
- **Accessibility:** a text box whose value is the caret's line, so linear
  mode reads the line the caret moves to.
- **`Widget::claims_key`** lets a widget on the focused path take a key the
  app would otherwise act on: the text area takes Ctrl+Z (not suspending)
  and, with a selection, Ctrl+C (not quitting).
- **`rich_interact::edit::Buffer`**: the editing (lines, caret, selection,
  undo and redo, the character limit, cleaning pasted text) moved out of
  `rich_interact`'s `TextArea`, which now uses it, as the intuiTUIve one
  does. Its behaviour is unchanged.
- **Python:** `rs_rich.tui.text_area(text, language=None,
  line_numbers=False, soft_wrap=True, read_only=False, placeholder=None)`.
  **Docs:** [Text areas](docs/guide/intuituive/index.md#text-areas), the
  `editor` example and its recording.

### Responsive layout, an error state and snapshot tests in intuiTUIve (intuituive 0.0.4)

0.0.19 workstream 8 (#712, #713).

- **Responsive nodes:** `.hide_below(columns)` hides a node while the app
  is narrower than that, and `.show_below(columns)` shows one only then;
  `.hide_below_height(rows)` and `.show_below_height(rows)` do the same for
  the height. A hidden node is as if a stylesheet said `display: none`: it
  takes no room, leaves the Tab order and the accessibility tree, and
  gives up the focus.
- **`@media` in stylesheets**, with CSS's names: `@media (max-width: 80) {
  … }`, and `min-width`, `min-height` and `max-height`, in cells,
  inclusive, joined with `and`. A resize that crosses a rule's bounds
  matches every node again and draws everything once; one that crosses
  none matches nothing again.
- **`viewport()`**: the columns and rows the app lays out in (the
  terminal's, less the inspector while it is shown), read like a signal,
  so a node can reflow on it. Every resize sets it.
- **An error state:** `.error_when(condition)`. The theme gives the node
  a look of its own (a panel's border in `border.error`, other text in
  `error`, each the theme's `bad` style unless named); a stylesheet
  matches it with `:error`, or CSS's `:invalid`; and assistive technology
  is told it is invalid (`AccessState::invalid`, `invalid` in linear mode,
  `aria-invalid` in a browser).
- **Snapshot tests:** `assert_screen!(driver, "name")` compares the screen
  with `tests/snapshots/name.txt`, plain text with one line a row, and
  fails with a unified diff. `UPDATE_SNAPSHOTS=1` writes the files, as
  every snapshot test in the repository does. `assert_screen_styled!` also
  keeps the rows as console markup. `intuituive::testing` has the
  functions behind them.
- **Driver helpers:** `settle()`, `press("tab enter")`, `type_text("Ada")`,
  `focused()`, `focused_name()`, and `assert_focused(name)`, which says
  where the focus is when it is not on `name`.
- Apps that use none of this behave as before.
- **Python:** `Node.hide_below`, `show_below`, `hide_below_height`,
  `show_below_height` and `error_when`; `viewport()`; `AccessState.invalid`;
  the driver helpers; `assert_screen(screen, path)`,
  `assert_screen_styled` and `assert_snapshot`.
- **Docs:** "Responsive layout", `@media` and "Errors" in the intuiTUIve
  guide; its Testing section shows a component test and an end-to-end
  test with snapshots, as does the Python page.

### Wide and long tables (interact 0.0.7, intuituive 0.0.4, cli 0.0.19)

0.0.19 workstream 9 ([#717](https://github.com/buchochelliq-labs/rs-rich-cli/issues/717) and
[#716](https://github.com/buchochelliq-labs/rs-rich-cli/issues/716)).

- **Tables scroll across.** `TableOptions::scroll_x()` keeps an intuiTUIve
  table's columns at their natural widths when they do not fit, and
  `TableSelect::scroll_x(true)` does the same for rs-rich-interact's table.
  Left and Right scroll a column at a time (Shift+Left and Shift+Right with
  a cell cursor, which Left and Right move), Shift+Home and Shift+End go to
  the ends, and Shift with the wheel scrolls too. A table that fits lays
  out as before, and a table without the option is unchanged.
- **The header stays over its columns.** It is cut where the rows are.
  `‹` and `›` at its ends say that columns are out of view on that side,
  readable without colour.
- **The cursor stays in view.** The view follows an intuiTUIve table's
  cell cursor and a `TableSelect`'s focused column.
- **Frozen columns:** `freeze_columns(n)` keeps the first `n` columns in
  place while the rest scroll, on both.
- **`virtual_table_with(columns, len, row, selected, options)`** gives a
  virtual table a cell cursor, resizable columns or a scroll across.
- **Sticky table headers when paging.** `Pager::sticky_table_header(true)`
  keeps a table's header on top while its rows scroll, when the content
  starts with a table. `table_header_rows(lines)` finds the header (title,
  top edge, header rows and the line under them) in rendered lines, so it
  works on output rendered elsewhere. `Pager::sticky_header(n)` keeps the
  first `n` lines instead, and `Viewport::sticky(n)` does the same for a
  viewport. Content that does not start with a table, and output without a
  terminal, are unchanged.
- **`rich pager`** keeps a table's header on top by default; `--sticky
  LINES` keeps the first LINES lines there instead, and `--sticky 0`
  nothing. `rich ... --pager` still runs `MANPAGER`, `PAGER` or `less`;
  with `PAGER='rich pager'` its tables page with their header on top.
- **Python:** `TableOptions().scroll_x()` and `.freeze_columns(n)`,
  `virtual_table(..., options=None)`, and `Pager(...,
  sticky_header=None)`, which takes a number of lines or `"table"`.
- **Docs:** [Wide tables](docs/guide/interact/explorers.md#wide-tables),
  the [intuiTUIve table options](docs/guide/intuituive/index.md#sorting-resizing-and-a-cell-cursor),
  the pager and viewport in the
  [interact guide](docs/guide/interact/index.md#pager), and
  [the CLI guide](docs/cli.md#ask-in-a-script).

### Reorderable lists (interact 0.0.7, intuituive 0.0.4)

0.0.19 workstream 10 (#720).

- **`rich_interact::Reorder`**: put a list in order, then Enter returns the
  items' values in the new order (`order()` has their first indices).
  Alt+↑/↓ and Ctrl+↑/↓ move the focused item, and Alt or Ctrl with
  Home/End to the top or the bottom. `K` and `J` do the same in any
  terminal, and so does grabbing: Space grabs the item, the arrows carry
  it, Space drops it and Esc puts it back. Ctrl+R restores the first
  order. With the mouse on, a drag carries a row. The keys are in context
  `reorder`. Without a terminal it reads the new order as numbers (`3 1`:
  those first, the rest keeping theirs).
- **intuiTUIve: `list(..).reorderable(on_reorder)`**, with the same keys
  and a drag. Each `Move { from, to }` goes to `on_reorder` for the app to
  make in its own items (`Move::apply`); the selection follows. The
  carried row is marked `↕`.
- **`rich_interact::components::move_item`** makes one move in a slice.
- **Python:** `rs_rich.interact.Reorder(prompt, items, *, height=10,
  mouse=False)`, and `Node.reorderable(handler)`, where
  `handler((from, to), cx)` makes each move.

### Date and time pickers (interact 0.0.7, intuituive 0.0.4, cli 0.0.19)

0.0.19 workstream 10 (#719).

- **`rich_interact::datetime`**: `Date` (moved from intuiTUIve, which
  re-exports it unchanged), `Time` and `DateTime`. They are hand-rolled,
  with no time zone and no dependency. Each parses and prints ISO 8601
  (`2026-10-17`, `14:30`, `2026-10-17 14:30`), and a time also parses
  `2:30 pm`. `today_utc()` and `now_utc()` are in UTC.
- **`DatePicker`, `TimePicker` and `DateTimePicker`** in rs-rich-interact.
  The date is picked on a month grid, the time field by field (seconds and
  a 12-hour clock are options), and Tab moves between them. `min` and
  `max` set a range: days outside it are dimmed and cannot be reached, and
  a time outside it is refused on Enter with a message naming the limit.
  `validate` refuses with a message of its own. The selected day is in
  brackets as well as reversed, so it reads without colour. Without a
  terminal each reads a typed line and checks it against the range.
- **intuiTUIve:** `calendar_options` with `CalendarOptions` (today, `min`
  and `max`; days outside are drawn in `calendar.disabled`), `time_picker`
  and `time_picker_with` (`TimeOptions`), and `datetime_picker`
  (`DateTimeOptions`): a calendar over a time picker on one `DateTime`
  signal. A change past the range stops at its end, and a line says why.
- **`rich date`** in the CLI picks a date, a time (`--kind time`) or both
  (`--kind datetime`) and prints it in ISO 8601 form. It takes `--min`,
  `--max`, `--value`, `--default`, `--seconds`, `--twelve-hour`, `--header`
  and `--mouse`. Exit codes are those of the other interactive commands.
  Without a terminal it reads a typed value, and one outside the range
  exits 3. The CLI reference is regenerated, and `docs/tapes/date.tape` is
  added (not yet recorded).
- **Python:** `rs_rich.interact.DatePicker`, `TimePicker` and
  `DateTimePicker`, on `datetime.date`, `datetime.time` and naive
  `datetime.datetime`. In `rs_rich.tui`, `calendar_with(..., min=, max=)`,
  `time_picker` and `datetime_picker`, whose signals hold a `datetime.time`
  and a `datetime.datetime`.
- **Docs:** [Reorder and the date pickers](docs/guide/interact/index.md#reorder),
  [times and reorderable lists](docs/guide/intuituive/index.md#times-and-a-date-with-a-time)
  in intuiTUIve, and `rich date` in [Ask in a script](docs/cli.md#ask-in-a-script).

### The inspector: selection, timings and an event trace (intuituive 0.0.4)

0.0.19 workstream 11 (#714, #721).

- **The inspector takes the keys.** Shift+F12, or a click on its panel,
  gives them to it; Esc, Shift+F12 again or a click on the app gives them
  back. While it has them, no key or paste reaches the app. Ctrl+C still
  quits, and F12 shows and hides it as before.
- **Four panes:** tree, node, timings and trace, chosen with Tab, `1` to
  `4` or a click on their names. The top of the panel says what the app's
  last frame drew, as before; a frame drawn for the inspector alone leaves
  it as it was.
- **Selection:** the arrows (and `k`, `j`, PgUp, PgDn, Home, End) move
  through the tree, ← and → go to a node's parent and first child, and a
  click on a line selects it. Hidden nodes are in the tree too, marked
  `hidden`. While the inspector has the keys, the selected node is outlined
  on the app: its edge cells are restyled (`inspector.selected` in the
  theme), so nothing moves.
- **Picking:** `p` selects the node under the pointer, or under a cursor
  the arrows move. A click or Enter keeps it, and the app does not see that
  click; Esc puts the selection back.
- **The node pane:** where the node is; its size and limits, and who set
  them; a column's or row's gap, alignment and wrapping; a grid's tracks,
  or a child's cell and span; `align-self`, `dock`, padding and border; its
  focus; the responsive rule that hides it; its classes and states
  (`:focus`, `:hover`, `:disabled`, `:error`, `:selected`); whether it was
  dirty and drew; its timings, role, keys and handlers.
- **Timings:** each node's draws and its own time (its children's are
  theirs), in all and the last time, slowest first; the last frame's time,
  and the average and slowest of the last 120. `s` sorts by another column,
  and `c` starts again.
- **The trace:** keys with their modifiers; presses, releases, drags and
  the wheel with the node under the pointer; each with the handler that
  took it and its node (`on_key`, `on_click`, `on_mouse`, `on_drop`, a
  widget). Resizes, pastes, the focus moving, and screens opening and
  closing. Signal writes, named by where the signal was made and where it
  was written, with the nodes they made dirty. Timers, watches, tasks done
  and jobs from other threads. Space pauses it, `c` clears it, and it keeps
  the last 1,000 events.
- **The redraw flash:** `f` marks what each frame drew on the app for
  300 ms (`inspector.flash` in the theme).
- **Nothing while closed:** nothing is measured or traced. Each hook
  checks a flag first, and the app sends the same bytes, frame for frame,
  as one without an inspector. In linear mode the inspector is off: it
  takes no columns, and F12 is the app's.
- **For tests:** `Driver::inspector_trace()`, `inspector_timings()` and
  `inspector_selected()`, with `inspect::TraceEntry`, `TraceKind`,
  `NodeTiming` and `TRACE_LIMIT`. `signal`, `Signal::update` and `set`,
  `every`, `watch`, `spawn`, `spawn_future`, `resource` and `Proxy::run`
  take where they were called (`#[track_caller]`), which the trace shows.
- **The default look changed:** a line names the panes, and a help line is
  at the bottom. The intuiTUIve tape's `inspector` screenshot needs
  recording again.
- **termwiz on Windows (#721):** CI builds rs-rich-interact and
  rs-rich-intuituive with termwiz on Windows, every target, and runs their
  headless suites there. The `files` and `scope` examples take `--backend
  NAME`, or `INTUITUIVE_BACKEND=NAME`, through `BackendKind::from_name`.
  The `files` example's test reads the path with the system's separator. A
  pass in real terminals of the mouse, resizes and Ctrl+Z on the backends
  is still the maintainer's.
- **Python:** `Driver.inspector_trace()`, `inspector_timings()` and
  `inspector_selected()`, with `TraceEntry` and `NodeTiming`.
- **Docs:** the [inspector](docs/guide/intuituive/index.md#inspector) with
  every key, [backends](docs/guide/intuituive/index.md#backends), the
  tutorial's look inside, and the porting and Python pages.

### Org-mode, AsciiDoc and man pages (ext 0.0.16, cli 0.0.19)

0.0.19 workstream 19 (#594, #595, #591, #592, #593).

- **`rich_ext::document`**: one interface for document formats. A
  `DocumentFormat` has an id, aliases, extensions, a content sniffer that
  returns a confidence, and a renderer that returns diagnostics with the
  renderable. `DocumentRegistry::with_builtins()` holds Markdown,
  reStructuredText, Org, AsciiDoc and man, and `detect` picks one from a
  file name and the content. A plugin's `SourceRenderer` joins the registry
  through `SourceRendererFormat`.
- **One look.** Org, AsciiDoc and man pages parse into a small block and
  inline model (`Document`, `Block`, `Inline`). `DocumentView` draws it in
  the look of core's `Markdown`, with the same `markdown.*` styles. New
  `document.*` styles cover TODO keywords, tags, admonitions and callouts.
- **Org**: headlines with TODO keywords, priorities and tags; emphasis;
  plain, numbered, description and checkbox lists; source, example, quote,
  verse and special blocks; tables; links, footnotes and timestamps.
  Comments, export blocks and `#+NAME` lines are not shown.
- **AsciiDoc**: the header and its attributes; sections; constrained and
  unconstrained quotes; every list kind, with `+` continuations; listing,
  source (with callouts), literal, example, sidebar, quote and verse blocks;
  admonitions; tables; links, footnotes and inline macros. `include::` is
  not followed, and a warning says so.
- **Man pages**: man(7) with its header and footer, sections, tagged and
  indented paragraphs, fonts and escapes, `.if` and `.ie`, tbl tables, and
  the common mdoc(7) macros. `eqn`, `pic` and `.so` are skipped with a
  warning.
- **Robust.** No input makes a parser panic. Nesting is capped and inline
  parsing is linear. A megabyte of ordinary text renders in well under a
  second in a release build. A deterministic stress test feeds garbled and
  deeply nested input to all three parsers.
- **`rich` (CLI)**: `--org`, `--asciidoc` (`--adoc`) and `--man` (`--roff`),
  and the same names as subcommands. `--format` takes a document format as
  well as a data format. `--syntax` still shows a file's source. The config
  file accepts the new modes, and `mode = "rst"`, which it refused before.
  The CLI reference is regenerated.
- **Detection.** `.org`, `.adoc`, `.asciidoc`, `.man`, `.roff` and `.mdoc`
  pick their format by name. `.1` to `.9` and `.asc` need the content to
  agree. A file with no extension is a document only when its content is
  unmistakable. Piped input is sniffed only with `--format auto`. A
  compressed man page such as `ls.1.gz` exits 3 with a hint to decompress
  it, since rich does not decode it.
- **Python:** `rs_rich.ext.document`, with `DocumentView`, `render`,
  `detect`, `sniff` and `formats`, and a
  [Documents](docs/python/ext/documents.md) page in the Python docs.
- **Docs:** a guide page, [Documents](docs/guide/ext/documents.md), a
  section in [the CLI guide](docs/cli.md#org-asciidoc-and-man-pages), a
  note in [PLUGINS.md](docs/PLUGINS.md#document-formats-0019), and
  [a recording](docs/recordings.md#org-asciidoc-and-man-pages).

### Blocks, text and layout, after ratatui (intuituive 0.0.4)

0.0.19 gap batch A (see [intuiTUIve against ratatui](docs/design/intuituive-vs-ratatui.md)).

- **Panel titles:** `.title_align(Align)`, `.title_right(..)`,
  `.title_bottom(..)` and `.add_title(Edge, Align, markup)` put several
  titles on the top and bottom lines. Titles take markup, styled by
  `.title_style(..)`. When they do not fit, the left ones win, then the
  centred, then the right; a lone title is cut with `…`.
- **Borders in code:** `.border(Border::Thick)` (also `Plain`, `Double`,
  `Ascii`, dashed, quadrant and `Custom(BorderSet)`), `.borders(Sides::TOP
  | Sides::BOTTOM)` for some sides, `.border_style(..)` and
  `.focus_border(..)`. `Border::None` keeps the title rows.
- **Shadows:** `.shadow(Shadow::MediumShade)` (or `Dim`, `Block`, a
  symbol) draws one under the right and bottom edges, styled by the
  theme's `shadow`.
- **A style on any node:** `.style(..)`, `.bg(..)` and `.fg(..)` fill the
  node's area under its children. Code wins over the sheet.
- **Text:** `.text_align(TextAlign::Center | Right | Justify)`,
  `.no_wrap()` and `.overflow(Overflow::Ellipsis)` on `text(..)` nodes.
- **Ratio sizes:** `Size::Ratio(1, 3)` and `.ratio(1, 3)`. Ratios share a
  track exactly: three thirds of 10 fill all 10.
- **Laying out by hand:** `layout::split(rect, axis, tracks, flow)` and
  `split_with_spacers` return the rectangles a row or column would give,
  and the room between them. `Track` is public. rs-rich-ext's `Rect`
  gains `inset`, `centered`, `offset`, `union`, `intersects`, `rows` and
  `columns`, and `Screen::get` reads a cell.
- **Collapsed borders:** `.collapse_borders()` on a row, column or grid
  draws one line between neighbours, joined with `┬`, `┼` and `┤`, as
  ratatui's collapsed borders do. The focused child's box is lit.
  Collapsed containers can nest and share their lines.
- **Drawing over children:** `Widget::draw_over` draws after a widget's
  children. `Canvas::cell(x, y)` reads what was drawn (a `CellView`),
  and `set_cell` and `fill_with` write cells. `effect(child, |canvas,
  cx| ..)` wraps any node, and `fill(symbol, style)` fills its area.
- **Layers:** `layers([..])` stacks children in one area, later ones on
  top. `.inset(..)` and `.place(across, down)` position a layer. Clicks
  go to the top layer, and the accessibility text leaves out what is
  covered.
- **Theme styles for a subtree:** `.theme_styles([("warning", "bold
  red")])` overrides theme styles for a node and its children, markup
  and widgets alike. A theme switch keeps the override.
- **In the stylesheet**, with CSS's names: `border` takes every kind
  (`thick`, `double`, `dashed`, `dotted`, …), with `border-subtitle`,
  `border-title-align`, `border-subtitle-align`, `border-sides` and
  `border-collapse`; `text-align`, `text-wrap` and `text-overflow`; and
  `width: 1/3`.
- **Nothing changes by default:** every existing snapshot and guide
  picture is unchanged. A title too long for its panel is now cut with
  `…`.
- **Python:** `Border`, `Shadow`, `Edge`, `Sides`, `TextAlign`,
  `Overflow`, `Track`, `CellView`, `layers`, `effect`, `fill`,
  `layout_split` and `layout_split_with_spacers`, the new `Node` builders,
  `Widget.draw_over` and `overlapping`, `Canvas.cell`, `set_cell` and
  `fill_with`, `Size.Ratio` and `Rect`'s helpers.
- **Docs:** the intuiTUIve guide's panels, text, layers, laying out by
  hand, effects and theme sections, its Python page, and the rows closed
  in the ratatui comparison.

### Charts, the canvas and colour, after ratatui (ext 0.0.16, intuituive 0.0.4)

0.0.19 gap batch C (see [intuiTUIve against ratatui](docs/design/intuituive-vs-ratatui.md)).

- **Charts fill their node.** A `Chart`, a `Canvas`, vertical bars, a
  `Sparkline` and a `FillGauge` take the height they are given, as
  ratatui's widgets fill their area: a `Layout` region, or an intuiTUIve
  node through `renderable(..)`. Given no height, each draws as before.
  `Chart::height` and `Canvas::height` still fix it, as `bar_width` does
  for vertical bars.
- **Gauges as ratatui draws them.** `FillGauge` fills whole cells (eighths
  with `unicode(true)`), centres its label and swaps the label's colours
  where the bar runs under it. `LineGauge` draws a label and a thin, thick
  or double line (`LineSet`), or two symbols of your own. Both take a
  ratio or a `percent`. The theme gains `chart.gauge.filled` and
  `chart.gauge.unfilled`.
- **Sparklines taller than a row.** `rows(n)` stacks eighths (or Braille
  dots) over `n` rows. `direction(Direction::RightToLeft)` draws the
  newest value on the left. `optional(..)` takes absent values, drawn with
  `absent(symbol, style)`. `styles(..)` colours each value, and
  `levels(Levels::Three)` draws halves and whole cells only.
- **Grouped bars.** `BarGroup` holds bars under a label, added with
  `BarChart::group`. `thickness`, `bar_gap` and `group_gap` set the bars'
  size and the gaps between them. A `Bar` takes `text` to show in place
  of its value, and `value_style` and `label_style`; the chart takes both
  as defaults. `values_inside(true)` writes values on the bars, as
  ratatui does.
- **Area charts and styled labels.** `GraphType::Area` fills from a line
  down (or up) to `Dataset::fill_to`, the bottom of the axis by default.
  `Labels::Markup` styles axis labels, and `Axis::labels_align` places
  them, the first x label against the y axis as ratatui does.
- **The canvas.** `CanvasPainter::map` draws the world's coastlines at
  `MapResolution::Low` or `High`. They are Natural Earth's public-domain
  110m and 50m coastlines, simplified by `scripts/world_map.py` to about
  28 KB in all. New markers are `Bar` (a lower half block), and
  `Quadrant`, `Sextant` and `Octant`, which draw 2×2, 2×3 and 2×4 dots a
  cell. `background(style)` colours every cell. `point` draws one dot,
  `area` a line with the region below it filled, and the `Shape` trait
  draws shapes of your own.
- **Colour.** `rich_ext::palette` has the Tailwind CSS and Material Design
  palettes, as `Color`s, and `hsl`, `hsv`, `oklab` and `okhsv` (Björn
  Ottosson's Oklab colour space). `blend` and `gradient` mix colours.
- **Calendars.** `CalendarOptions` takes `event(date, style)` and
  `events(fn)` to mark days, `surrounding(style)` to show the days of the
  months either side, and `first_weekday(Weekday::Sunday)`. It also takes
  `weekday_style`, `month_style` and `day_style`. `month(shown, options)`
  draws a month that cannot be focused, so a year is a grid of twelve.
  **Breaking:** `CalendarOptions` is no longer `Copy`, as it holds a
  function.
- **Python:** `Sparkline(rows=, direction=, absent=, styles=, levels=)`,
  `ChartBar(text=, value_style=, label_style=)`, `BarGroup`,
  `BarChart(groups=, thickness=, bar_gap=, group_gap=, label_style=,
  value_style=, values_inside=)`, `FillGauge` and `LineGauge` in
  `rs_rich.chart`. Python has no canvas, xy chart or palette module yet,
  and the calendar's new options are not bound.
- **Docs:** the [charts guide](docs/guide/ext/charts.md) gains sections on
  gauges, taller sparklines, grouped bars, area charts, the map and
  palettes, with six new screenshots. The Python charts page gains two
  sections and two pictures. The intuiTUIve guide covers the new calendar
  options, and the ratatui matrix marks these rows as built.

### Widgets, inputs and the app shell, after ratatui (intuituive 0.0.4)

0.0.19 gap batch B (see [intuiTUIve against ratatui](docs/design/intuituive-vs-ratatui.md)).

- **List options.** `list` moves to its own widget, with builders on the
  node: `.symbol(markup)`, `.repeat_symbol()` and
  `.highlight_spacing(HighlightSpacing::..)` for a highlight symbol;
  `.direction(ListDirection::BottomToTop)`; `.scroll_padding(n)`;
  `.multiline()` for items of several lines; `.row_style(|i| ..)` for whole
  rows; and `.offset(signal)` for the first item in view. They work on a
  reorderable list too.
- **An empty selection.** `list_optional(items, Signal<Option<usize>>)` and
  `table_optional(columns, rows, selected, options)`: `None` shows no
  highlight, and ↓ selects the first item.
- **Table options.** `TableOptions` gains `.footer(cells)`,
  `.header_gap(n)`, `.row_gap(n)`, `.column_spacing(n)`, `.symbol(..)`
  with `.highlight_spacing(..)`, `.row_style(..)`, `.multiline()`,
  `.row_height(n)` (for virtual tables), `.column_highlight()` and
  `.scroll_padding(n)`. `Column` gains `.align(Align::End)`, `.min(n)` and
  `.max(n)`. With rows of several lines, ↓ still moves a row and PgDn a
  screenful of rows. The theme names `table.footer` and `selected.column`
  are new.
- **Scrollbars everywhere.** `Scrollbar::new()` with `.side(Side::..)`,
  `.show(Show::..)`, `.symbols(ScrollbarSymbols::..)`, `.thumb(..)`,
  `.track(..)` and `.ends(..)`. `.scrollbar(bar)` puts one on a scroll, a
  list, a table or virtual list, a tree, a text area or a log, in a column
  it keeps while the bar shows. `scrollbar(position, len, view, bar)` is a
  bar on its own. A press on the track pages, an arrow steps, and dragging
  the thumb scrolls. In text mode the bar is blank. The default bar of a
  scroll is unchanged.
- **Tab strip options.** `.divider(markup)`, `.tab_padding(left, right)`
  and `.titles_align(Align)` on `tabs`. Clicks and `measure` follow them.
- **Inputs bound to signals.** `input(Signal<String>)` is a text area in a
  single-line mode: Enter goes to `.on_submit(..)` (or bubbles), Tab moves
  on, pasted lines join, and a long line scrolls to the caret. `.mask('•')`
  and `.max_length(n)` work on it. `number_input(Signal<i64>, range)` with
  `.step(n)` steps with the arrows, the page keys and the wheel, and takes
  typed digits, clamped. Both are text boxes for assistive technology.
- **Modal shadows and backdrops.** `cx.modal_with(width, height,
  ModalOptions::new().shadow(style).backdrop(style), build)` and
  `cx.popup_with(..)`. The shadow and the backdrop restyle the screen below
  wherever it draws again.
- **Printing above an inline app.** `cx.print_above(markup)` and
  `Proxy::print_above` put lines into the scrollback above the region, which
  moves down under them, as ratatui's `insert_before` does. Full-screen,
  they print once the app ends; `Driver::take_printed()` gives them to a
  loop of your own.
- **Exit values and errors.** `cx.quit_with(value)` with `App::run_for::<T>()`,
  `App::run_on_for` and `Driver::take_exit`; `cx.fail(error)`, which
  `App::run` and `run_on` then return, with `Driver::take_error`.
- **A mouse switch and a frame count.** `App::mouse(Some(true))` captures
  the mouse inline, and `Some(false)` leaves it to the terminal
  full-screen. `App::session_options(backend)` is what `run` asks for.
  `FrameStats::frames` counts the frames drawn.
- **The churn benchmark.** `tests/versus_ratatui.rs` reports two more
  scenes, not gated yet: every cell changing each frame (an RGB swatch, as
  `colors-rgb`) and a chart redrawn each tick. ratatui is ahead on both: at
  80x24 a churn frame costs 3,042 µs against its 326, and a chart 463 µs
  against 81, with fewer bytes. The dashboard's three gated scenes still
  pass.
- **Changed:** `Column` and `TableOptions` gain public fields, and
  `FrameStats` gains `frames`, so code that builds them field by field
  needs the new ones.
- **Python:** the same options and functions in `rs_rich.tui`.
- **Docs:** the guide's inputs, table, list, tab strip and scrollbar
  options, modal shadows, exit values and printing above an inline app;
  the porting guide's table.

### `rich log`: filters, follow mode and groups (ext 0.0.16, interact 0.0.7, cli 0.0.19)

0.0.19 workstream 13 (#724).

- **Records from any log.** `rich_ext::log_stream::Record` holds a log
  record's fields under the names the input gave them, and the lines that
  followed it. `Record::to_event` reads it as a `StructuredEvent`.
  `Parsers` reads logfmt, lines that start with a level, lines that start
  with an ISO 8601 time, and nginx and Apache access logs, and JSON with
  the `data` feature. A `LineParser` of your own plugs in. `Assembler`
  joins a line no parser reads (a stack trace) to the record before it,
  and keeps any other as a text record: nothing is dropped.
- **Filters.** `LevelFilter` keeps a level (`error`) or a level and those
  above it (`warn+`). `Predicate` is a small grammar: `=`, `!=`, `>`, `>=`,
  `<`, `<=`, `~` (a regular expression), `!~`, `FIELD` and `!FIELD`, with
  dots into nested fields. Levels compare by severity, numbers as numbers
  and times as instants. `Filter` takes both and ANDs the predicates.
- **Repeats, groups and a summary.** `Collapser` folds a run of identical
  records (their times aside) into the first and a count. `GroupBy` finds
  a trace, span or correlation id (`trace_id`, `request_id`, an event's
  `correlation_id`, ...); `Groups`, `GroupHeader` and `Summary` gather and
  draw them, the summary with counts by level and the groups with errors.
  `Clock` shows a time as written, since the first record, or not at all,
  and `parse_timestamp` reads RFC 3339, the access log's times and epoch
  numbers.
- **`rich log` takes them:** `--level`, `--where` (repeatable),
  `-f`/`--follow` (a truncated or replaced file is read again from its
  start), `--collapse` (on by default when following, the count growing in
  place on a terminal), `--group trace|span|correlation` with a summary
  (`--summary`, `--no-summary`), `--fields`, `--parser`, `--time
  absolute|relative|none`, `--output jsonl`, and `--redact` and
  `--redact-pattern` as `rich capture` has them. A line that is not JSON
  used to fail the command; it is now parsed, or shown as text.
- **The same output, faster.** With none of the options, JSON records print
  as before, byte for byte. Plain text that is one cell a character wraps
  without core's per-character measuring, which a test holds to core's
  output: 100,000 records take 0.57 s in a release build, down from 1.5 s.
- **A live view:** `rich_interact::LogView` shows records as they arrive,
  following the end. `/` types a filter (words, or `--where` tests), Space
  pauses, Enter shows a record's fields and lines. `rich log -i` runs it on
  a terminal; without one the records stream as usual.
- **Smaller things:** `Severity` is ordered (`Warn < Error`) and hashable.
  `Redactor::redact_field` masks a field's value as `key=value` would be
  masked. The `log.*` theme keys are in `STYLE_TABLES`.
- **Python:** `rs_rich.ext.log_stream` has the records, parsers, filters,
  folding, groups and summary (`LogRecord`, `LogAssembler`, `LogFilter`,
  `LogSummary`, ..., and the Rust names too). `rs_rich.interact.LogView` is
  the live view.
- **Docs:** [`rich log`](docs/cli.md#logs-rich-log) with every option, the
  CLI reference regenerated, the porting page, the Python pages, and
  `docs/tapes/log.tape` (not yet recorded).

### `rich deps`: sizes, baselines and licence policy (ext 0.0.16, cli 0.0.19)

0.0.19 workstream 16 (#706, #704).

- **`rich_ext::deps::sizes`**: what each crate adds to a build. `Sizes`
  reads a target directory's profile, matched to `cargo metadata`'s
  packages: rlibs (else the rmeta of a check build), proc-macros and dylibs,
  static libraries and binaries. A crate at two versions is told apart by
  its dep-info file. Of each package's artifacts the newest is taken, and
  stale ones no package claims are counted. Source sizes are optional.
  `Sizes::parse` reads `cargo bloat --crates --message-format json` too.
  `SizeReport` draws bars, a table and the total.
- **`rich_ext::deps::compare`**: two timings or size reports compared unit
  by unit. A unit regresses when it grows past a percentage and an
  absolute floor: 10% and 0.5s for build times, 10% and 64 KiB for sizes.
  New and removed units are listed, a new one past the floor regresses,
  and the totals are compared too. `Comparison::regressions()` feeds a
  gate. `Timings::to_json` and `Sizes::to_json` save a report, and the
  readers take it back.
- **`rich_ext::deps::policy`**: an allow and deny list, from rich's own TOML
  or cargo-deny's `deny.toml` (its `[licenses]` section, told apart by
  shape). Each SPDX expression is evaluated: `OR` needs one allowed
  choice, `AND` every part, and `WITH` exceptions are kept. Deny wins over
  allow, and a crate's exception wins over both. `PolicyReport` lists each
  violation with its reason.
- **Conflicts**, a heuristic and not legal advice: each dependency's
  licence set against the root crate's on a small table. GPL code in a
  permissive or proprietary crate is a conflict; weak copyleft there is
  worth a check; permissive code is fine anywhere. Within the GNU family,
  `GPL-2.0-only` conflicts with Apache-2.0 and the version 3 licences.
- **`rich_ext::deps::sbom`**: the packages and licences of an SPDX (2.2,
  2.3) or CycloneDX (1.4 to 1.6) JSON document, told apart by content, so
  the licence report and the policy work beyond Cargo.
- **`rich deps`**: `--sizes [TARGET_DIR|FILE]` with `--build-profile`,
  `--source-sizes` and `--top N`; `--baseline FILE` with `--threshold` (a
  percentage, a time or a size) and `--save FILE`, for `--timings` and
  `--sizes`; `--policy FILE`; and `--sbom FILE` with `--licenses` or
  `--policy`. A regression or a violation exits 5 once the report is shown,
  like `--audit`. Without the new options `rich deps` prints what it always
  did.
- **Robust.** Truncated JSON, odd TOML, unknown SPDX identifiers and
  documents of the wrong kind are a clear error (exit 4) or a warning row.
  Nothing panics, and directory walks are bounded.
- **Docs:** sections in [the CLI guide](docs/cli.md#dependency-sizes-baselines-and-licence-policy-0019)
  and [the ext guide](docs/guide/ext/sources.md#artifact-sizes) with three
  new pictures, the regenerated CLI reference, fixtures in
  `crates/rich-ext/tests/fixtures/deps`, and `docs/tapes/deps-policy.tape`
  (not yet recorded). The Python package binds no `deps` API, so it has
  nothing new.

### ratatui's demos, rebuilt on intuiTUIve (intuituive 0.0.4)

0.0.19 demo batch D1 (see [intuiTUIve against ratatui](docs/design/intuituive-vs-ratatui.md#3-the-demos)).

- **`demo`, ratatui's classic demo.** `examples/demo.rs` rebuilds its three
  tabs: gauges, a sparkline, lists, bars and a sine chart fed by a timer; a
  server table beside a world map; the named colours. A tick redraws only
  the parts that move, and a test checks it. The mouse picks tabs, tasks
  and servers, `?` and Ctrl+P list the keys, and `--frozen` stops the
  clock.
- **`demo2`, ratatui's showcase.** `examples/demo2.rs` rebuilds its About,
  Recipe, Email, Traceroute and Weather tabs over an Okhsv swatch, with a
  recipe, emails, hosts and weather of our own. Its colours are in
  `examples/demo2.tcss`, reloaded live. It fits any size, and destroy is an
  effect over the whole app, on the virtual clock in the tests.
- **A list takes the mouse.** A press on an item selects it, and the wheel
  moves the selection three items, as on a table. The press still reaches
  the node's own handler.
- **An idle effect costs nothing.** An `effect` that wrote no cells the
  last time it ran no longer draws everything inside it again when a child
  changes: the child draws alone.
- **Docs:** [ratatui's examples, rebuilt](docs/guide/intuituive/ratatui-examples.md),
  a new guide page; ratatui in the porting guide's credits;
  `docs/tapes/demo.tape` and `docs/tapes/demo2.tape` (not yet recorded).
- **`widgets`, a widget gallery** (demo batch D2). `examples/widgets.rs`
  rebuilds ratatui's widget examples, a tab each: blocks, text with a
  masked secret and a link, lists (one bottom to top), a table and
  ratatui's table app, tabs, scrollbars, a calendar, a pop-up over four
  shadows, collapsed borders and buttons drawn by hand. The mouse picks
  tabs, items, rows, panes and buttons.
- **`charts`, a chart gallery.** `examples/charts.rs` rebuilds ratatui's
  gauge, sparkline, bar, grouped bar, chart and canvas examples and its
  `gauge`, `weather` and `chart` apps on rs-rich-ext's charts. A tick
  draws only what moves, and `--frozen` stops the clock.
- **`layout`, a layout explorer.** `examples/layout.rs` rebuilds
  `constraint-explorer`, `constraints` and `flex`: a row of blocks you
  edit, drawn in every `Justify` mode with its spacers; each kind of size
  in real nodes; and `align_items` and wrapping, which ratatui lacks.
- **Docs:** the galleries' section of the guide page, README rows, and
  `docs/tapes/widgets.tape`, `charts.tape` and `layout.tape` (not yet
  recorded).
- **The apps (batch D3).** Seven of ratatui's smaller apps rebuilt, each
  with its tests and snapshots, and a section of
  [ratatui's examples, rebuilt](docs/guide/intuituive/ratatui-examples.md#the-apps):
    - `colors`: the named, indexed and RGB colours (an Okhsv swatch at 60
      fps, with its frames a second), the modifiers, and Tailwind's and
      Material's palettes;
    - `canvas`: a world map you move about on, pong at 60 fps, a pad to
      draw on with the mouse and growing boxes, in every marker;
    - `form`: a form whose fields are in error while they are wrong and
      which comes back from `run_for`, and a chat input with modes;
    - `calendar`: a year of `month` nodes in six looks, a move drawing
      only the months it touches;
    - `downloads`: an inline region whose workers report from threads
      through a `Proxy` and print each finished file above it;
    - `pulls`: a table loaded by a `resource`, its state in the panel's
      title;
    - `panic`: the terminal given back before a panic's message (checked
      in a PTY), and an error returned from `run`.
- **Docs:** `docs/tapes/apps.tape`, four of the apps (not yet recorded).

### Charts that read aloud, distributions and small multiples; wide tables by column (ext 0.0.16)

0.0.19 workstream 14 (#726, #702).

- **Charts read aloud.** Every chart implements `a11y::AccessibleText`:
  sparklines, bars and their groups, histograms, line, scatter and area
  charts, gauges, heatmaps, status matrices, KPI cards, timelines and the
  canvas. Each says what it is, its axes' ranges and its key values, in a
  few lines of plain words: `Sparkline, 24 values: min 12, max 52, last
  14, trend up`, `Gauge cpu: 72%, 72 percent of 0% to 100%, band high`. A
  trend is the least-squares line through the values: up or down when it
  moves by a tenth of their range, flat otherwise. The text is the same at
  any width, on any console.
- **In an intuiTUIve app** (intuituive 0.0.4), `described(move || ..)`
  draws a chart as `renderable(..)` does, and names it by that text. The
  accessibility tree, linear mode and a screen reader read the
  description, and read it again when the data changes.
- **Distributions.** `Distribution` sums values up on one line: a
  box-and-whisker strip, then the count, `min`, `p25`, `p50`, `p75`,
  `p95`, `max` and `mean`. NaN and infinite values are left out and
  counted. Quantiles are numpy's default, linear interpolation;
  `Summary::of` and `quantile` give them without drawing.
  `DistributionSet` stacks several on one scale, their numbers in columns.
- **Small multiples.** `SmallMultiples` lays charts out in a grid, each
  under its title, fitted to the width: one column at 40 cells, three at
  80, four at 120, or `columns(n)`. Sparklines, bar charts, line charts and
  `Chart`s (a `Facet` each) share one value scale. A note under the grid
  says the scale and holds the one legend. The theme gains `chart.title`.
- **Wide tables by column.** `VirtualTable` gets a window of columns as
  well as rows: `column_offset`, `visible_columns` or `fit_columns`, and
  `frozen_columns` kept on the left. Only the columns shown are fetched,
  sampled and laid out, through the new `VirtualRows::rows_columns`. Its
  default picks cells from `rows`, so existing sources keep working. The
  position line adds `columns 42–51 of 2,000`, and `scroll_columns_by`
  moves the window across. `FnCells` makes each cell on demand. A
  120-column window of 2,000 columns by 100,000 rows renders in about
  20 ms in release, its width sample included (an ignored test times it).
- **Explicit bar sizes are kept.** A vertical `BarChart` with an explicit
  `thickness`, `bar_gap` or `group_gap` keeps them when the bars do not
  all fit, as ratatui does. It draws as many whole bars as fit, from the
  first, and leaves the rest off: 24 bars 3 cells thick and 2 apart show
  the first 8 in 40 cells. Before, they were squeezed to one cell with no
  gap. Charts that set neither squeeze as before.
- **Python:** `Distribution`, `DistributionSet`, `SmallMultiples`,
  `quantile`, `summary` and `describe` in `rs_rich.chart`.
  `rs_rich.ext.a11y.accessible_text` reads charts too. `data.VirtualTable`
  takes `column_offset`, `visible_columns`, `fit_columns` and
  `frozen_columns`, with `scroll_columns_by`, `shown_columns` and
  `column_position`.
- **Docs:** the [charts guide](docs/guide/ext/charts.md) gains sections on
  distributions, small multiples and accessible text, with three new
  screenshots. The [tables guide](docs/guide/ext/tables.md) covers the
  column window. The Python charts page gains three sections and two
  pictures.

### Nested JSON Lines profiles and style rules everywhere (data 0.0.3, ext 0.0.16, cli 0.0.19)

0.0.19 workstream 15 (#705, #701).

- **Nested paths:** `rich profile` profiles JSON Lines path by path. Each key
  of a nested object is a path (`user.plan`), an array's items are
  `tags[]`, and a key of the objects in an array is `items[].sku`. A key
  holding `.`, `[`, `]`, a quote or a control character is written as a
  JSON string in brackets: `["http.status"]`. `--depth N` (4) bounds the
  steps and `--max-paths N` (200) the paths; the paths past the cap are
  named in a note and a last table row.
- **Presence and types:** each path counts the records that lack it apart
  from an explicit `null`, and the JSON types its values had. The table
  shows the mix (`str 89% · null 11%`) and the absent records, and marks a
  path of more than one type with `!`.
- **Malformed lines** are counted and skipped, with their line numbers in a
  note, rather than ending the profile.
- **`--report json`** adds `paths`, `malformed`, `malformed_lines`, and per
  path `present`, `absent`, `values`, `types` and `mixed`. Every key of a
  flat profile is kept.
- **Migration:** JSON Lines are now profiled path by path by default.
  `--flat` gives the 0.0.16 output, rows of the top-level keys, and still
  stops at a line that is not JSON. CSV and TSV are unchanged. `--depth` is
  shared with `rich deps`.
- **`rs-rich-data`:** `JsonProfiler`, `Profile::from_json_lines`,
  `TypeCounts`, `PathSummary` and `path_key`; `ProfileOptions` gains `depth`
  and `max_paths`, and `ColumnProfile` gains the presence and type fields.
  Time is linear in the input and memory is bounded by the paths and their
  samples.
- **Style rules from the command line:** `--style-rule 'latency>500:bold
  red'` styles a cell, and `--style-rule 'status=failed:row:on red'` a row.
  The grammar maps one to one onto `StyleRule`: a symbol (`==`, `!=`, `<`,
  `<=`, `>`, `>=`, `*=`, `^=`, `$=`) or a comparison word, an optional
  target, and quotes for a value that holds `:` or must stay text. `--rules
  FILE` reads `[[rules]]` tables. Rules apply to `--csv` tables, `rich
  profile`'s table and `rich inspect --table`, and nowhere else.
- **Rules in config profiles:** `[[defaults.rules]]` and
  `[[profiles.NAME.rules]]` hold rules. They add up: the defaults', then the
  profile's, then the command line's.
- **Clear errors:** a bad rule is a usage error that names it and says what
  is wrong. A rule for a column the table lacks is a warning.
- **`rs-rich-ext`:** `StyleRule::parse`, `to_spec` and `Display`,
  `Comparison::name` and `symbol`, `Target::name`, and `style_rules` on
  `data::TableView` and `data::Explorer`.
- **Python:** `rs_rich.ext.table.StyleRule` and `StyleRules` (with `parse`,
  `when` for a Python predicate, and `from_toml`), a `rules` argument on
  `TableData` and `TableView`, and `Profile.style_rules`. `Profile.from_path`
  and `from_text` profile JSON Lines path by path (`flat=True` for rows), and
  `Profile.from_records` profiles Python records. Python can now use
  conditional table styles, which 0.0.16 listed as missing.
- **Docs:** [Style cells by value](docs/cli.md#style-cells-by-value-0019)
  and [Nested JSON Lines](docs/cli.md#nested-json-lines-0019) in the CLI
  guide, the [data guide](docs/guide/data/index.md#conditional-styles), the
  [Python tables page](docs/python/ext/tables.md#conditional-styles), and
  `docs/tapes/profile-nested.tape` (not yet recorded).

### Micro assets over two rows (micro 0.0.5)

0.0.19 workstream 17 (#711).

- **Two-row sizes.** A micro asset may now be any size up to 4×2 cells: 1
  to 4 columns and 1 or 2 rows, such as a 2×2 icon or a 3×2 animation.
  Anything larger is refused with an error that names the limit. The
  manifest's `size` field takes the new values (`"2x2"`,
  `{"cols": 3, "rows": 2}`); nothing else in the format changes, and old
  packages load as before. A package with no `size` is 2×1, whatever its
  image's shape: the size is never guessed.
- **Rows reserved in any layout.** `MicroRows` (and `reserve_rows` for
  rendered segments) adds a row below each line that holds a two-row
  asset. The row is blank but for the asset's second row; the text beside
  the asset stays on the first row, and the next line moves down, so
  nothing is drawn over it. A table row or a panel grows to hold it, and
  `MicroAssetRef` reserves its own rows. The output depends only on the
  input, at every width.
- **Drawn over both rows.** Kitty prints two rows of Unicode placeholders
  for one image. iTerm2 and Sixel draw one image of the whole box from the
  second row, moving the cursor up a row and back; Sixel's is twice the
  cell height in pixels. Half-blocks draw two pixel rows a cell row. The
  fallback is the emoji or text on the first row with a blank row below.
  Animations play as one-row ones do. Where the second row was not
  reserved (a bare `Text`, Markdown, a title), only the first row's
  fallback shows.
- **The tag carries the row.** The second row's cells are tagged
  `[name, cols, rows, id, 1]`; `MicroMeta` has a `row` field and
  `with_row`, and `fallback_row` gives any row's fallback. A placeholder is
  never drawn at a size other than its asset's.
- **Built-in library:** `fun/trophy` (2×2) and `fun/campfire` (3×2,
  animated), drawn for this project.
- **`rich micro`:** `create --size` takes the new sizes and refuses
  `2x3`. `list`, `show` and `preview` give a two-row asset its row, and so
  does `rich -p --emoji` text.
- **Python:** `MicroAsset` takes the new sizes and has `rows`, and
  `MicroMarkup` reserves the row below a two-row asset.
- **Docs:** [Two rows](docs/guide/micro/index.md#two-rows) in the micro
  assets guide, with a captured example; the authoring, terminals, CLI and
  library pages; the CLI reference regenerated; and
  `docs/tapes/micro-rows.tape` (not yet recorded).

### Banners, GIF playback controls and shell integration (cli 0.0.19)

0.0.19 workstream 21 (#708, #715).

- **`rich banner TEXT`** draws a FIGlet banner with rs-rich-art's FIGlet
  renderer: `--font` takes the bundled `standard`, a `.flf` file or the
  name of one in `$FIGLET_FONTDIR` or figlet's font directories
  (`--list-fonts`); `--style`; `--gradient` with two or more colours or a
  preset (`rainbow`, `sunset`, `ocean`, `forest`, `fire`) and `--direction
  horizontal|vertical|diagonal`; `--align left|center|right`; `-w/--width`;
  `--fit wrap|plain|none` for a banner wider than the width; and
  `--force-terminal`. Text comes from stdin when none is given. Behind the
  default `art` feature.
- **GIF playback controls:** `--speed X`, `--fps N` (the 30 fps cap, which
  was fixed), `--max-frames N` and `--max-duration SECS`, next to `--loop`.
  Each needs `--gif` and a number above 0. Without `--speed` or
  `--max-frames`, playback is rs-rich-art's `Stage` as before; redirected
  output is still one frame.
- **`rich shell init bash|zsh|fish|powershell`** prints a script for a
  shell's startup file: the completions, OSC 133 prompt and command marks,
  OSC 7 for the working directory, and a status line after each command
  (`--no-completions`, `--no-marks`, `--no-status`, `--min-duration`, 2 s
  by default). It keeps the shell's own prompt hooks and `$?`.
- **`rich shell status --exit CODE --duration D --command TEXT`** decorates
  a finished command in one line (`✖ error $ cargo test  exit 101  4.2s`)
  with rs-rich-ext's `CommandView`, following `RICH_A11Y` or `--symbols`;
  quiet for a success under `--min-duration`.
- **`rich shell link [PATH[:LINE[:COL]]]...`** prints paths, the working
  directory by default, as OSC 8 file links with rs-rich-ext's
  `Hyperlinker`, plain off a terminal unless `--links always`; `--tilde`,
  `--editor TEMPLATE`, and `--prompt bash|zsh` to mark the escapes as
  zero-width in a prompt.
- **Docs:** [Draw a banner](docs/cli.md#draw-a-banner-0019),
  [speed, frame rate and limits](docs/cli.md#speed-frame-rate-and-limits-0019)
  and [shell integration](docs/cli.md#shell-integration-0019) in the CLI
  guide; the CLI reference regenerated; `docs/PORTING.md`; and
  `docs/tapes/banner.tape` (not yet recorded).

### Approval queues and jumping to anything (interact 0.0.7, cli 0.0.19)

0.0.19 workstream 20 (#471, #484).

- **`rich_interact::components::ApprovalQueue`**: review pending items one
  by one or in bulk. `a`, `r` and `s` approve, reject and skip the focused
  item and move on to the next pending one; `A`, `R` and `S` ask for a
  reason first; Space and Ctrl+A mark items for one decision on all of
  them; `u` takes a decision back; `p` hides the detail pane (label,
  description, details, decision, then the item's preview); Enter submits
  an `ApprovalReport` of every item's `Decision` (verdict and reason),
  pending ones included. `require_reason(Verdict::Rejected)`,
  `auto_submit`, `advance` and `decided` (resume a review) are options.
  With the mouse on, rows, the mark cell and the buttons under the list
  click. Decisions are drawn in the theme keys `approve.approved`,
  `approve.rejected`, `approve.skipped` and `approve.pending`. Without a
  terminal it asks item by item (`a`, `r` or `s`, a reason after a colon,
  `q` stops). The keys are in context `approve`.
- **`rich_interact::components::Jump`** over a **`JumpIndex`** of
  **`JumpSource`s**: one fuzzy box over several sources, the results
  grouped by source (each showing its best few, `limit`, and how many more
  matched), the best match focused and previewed. A source's prefix typed
  first searches only it, Tab and Shift+Tab narrow and widen, Alt+↑/↓
  jump between groups, and a click on a group's title narrows to it. Enter
  returns a `Jumped` (source id, index, label, value). `JumpIndex::search`
  works without the component. Group titles use the theme keys
  `jump.group` and `jump.more`. Without a terminal it answers with the best
  match for a line read, or for its starting query. The keys are in
  context `jump`.
- **`rich approve`** reviews ITEMs, or lines or JSON Lines on stdin
  (`--format`, `--label`), and prints one JSON line per item with its
  decision and reason, or with `--print approved|rejected` those items as
  they were read. `--require-reason`, `--auto-submit`, `--height` and
  `--mouse`. Without a terminal, ITEMs are asked about line by line, and
  items from stdin are printed as pending.
- **`rich jump`** searches the files, Markdown headings (`#`) and code
  symbols (`@`) under DIRs (`--files`, `--headings`, `--symbols`), a
  commands file (`--commands`, `>`), a recent-places file (`--recent`,
  which the pick is added to) or entries on stdin (lines or JSON), and
  prints the place: `path`, `path:line` or the command (`--print json` for
  more). `--value`, `--limit`, `--height` and `--mouse`; without a
  terminal it prints the best match for `--value`. Both commands use the
  exit codes of the other interactive commands.
- **Python:** `rs_rich.interact.ApprovalQueue`, `Jump` and `JumpSource`;
  a review answers with a list of dicts and a jump with a dict.
- **Docs:** [Approval queues and jumping to anything](docs/guide/interact/review-and-jump.md),
  [Review and jump](docs/cli.md#review-and-jump) in the CLI guide, the
  Python guide, the CLI reference regenerated, and `docs/tapes/approve.tape`
  and `docs/tapes/jump.tape` (not yet recorded). An intuiTUIve wrapper is
  not part of this workstream.

### Sharing output: PNG export and a redaction preview (record 0.0.7, ext 0.0.16, cli 0.0.19)

0.0.19 workstream 19 (#723, #722).

- **PNG of rendered output, without a tape (#723).** rs-rich-record's
  screenshot rasteriser now draws any rendered output, not only a screen
  recorded on a PTY. `rich_record::render::png` has `segments_png` (the
  segments `Console::record_output` returns), `frame_png` and
  `frame_canvas` (a rich-ext `Frame`), `image_size` and `PngOptions`
  (theme, window frame and title, caption, text size, width in cells);
  `Snapshot::from_frame` resolves the cells as rich's SVG export does
  (reverse swaps, dim blends 40% towards the background). The glyphs are
  the embedded DejaVu Sans Mono and Twemoji, so the same output gives the
  same bytes on every machine. Too large an image is an error, not an
  allocation that fails.
- **`--export-png PATH`** beside `--export-html` and `--export-svg`,
  wherever they apply, `rich capture` included: the console's width, rich's
  SVG export theme, a window titled like the SVG's, 28-pixel text as the
  screenshots. Not with `--batch` (a usage error for now). It needs the new
  `png` feature, on by default, implied by `record`, and on in the Python
  wheel's command line.
- **Preview a redaction before anything is written (#722).**
  `Redactor::preview` (plain text), `preview_ansi` (text with escapes,
  positions in its visible text; a secret in an escape's body, such as a
  hyperlink's URL, is a `hidden` finding) and `preview_args` (a command
  line, as `redact_args` masks it) mask nothing and return a `Preview`:
  each `Finding` with its line, cell column, byte span, rule, text and
  replacement, plus `counts()` per rule and a `summary()`
  (`3 matches on 2 of 40 lines (key-value 2, token 1)`). A `Preview` is a
  renderable: each line with a match, the matches highlighted, one line per
  match, and the summary. It shows the secrets unmasked, so it is for the
  person about to share, not for sharing.
- **`--redact-preview`** on `rich capture` and `rich log`, with `--redact`
  or `--redact-pattern`: the preview instead of the output (for capture,
  the command line and the output; for log, every line read once, so not
  with `--follow` or `--interactive`). A dry run: no cast, export or log
  output is written, and stderr names what a run without the flag would
  write. With `--report json`, one JSON document on stdout. Exit 0; exit 2
  without a redaction flag.
- **Python:** `Redactor.preview`, `preview_ansi` and `preview_args`
  (`title=` optional) return a renderable `RedactPreview` of
  `RedactFinding`s, with character offsets; stubs updated.
- **Docs:** [A PNG of the output](docs/cli.md#a-png-of-the-output-0019) and
  [Preview a redaction before you
  share](docs/cli.md#preview-a-redaction-before-you-share-0019) in the CLI
  guide, [Preview what would be
  masked](docs/guide/ext/badges-and-redaction.md#preview-what-would-be-masked)
  in the redaction guide, the CLI reference regenerated, and
  `docs/tapes/share.tape` (not yet recorded).

### One process per browser session (web 0.0.2)

0.0.19 workstream 3.

- **`Isolation::Processes`, the default for an app:** each browser tab's
  session runs in a process of its own. The server starts its own program
  again with `RS_RICH_WEB_SESSION` set; the same `serve` or `Server::bind`
  call in that process connects back over loopback (proving itself with a
  secret, and the server itself with another), runs the tab's app and
  exits, while the server relays the
  WebSocket. Apps need no change. A crash, a hang or a runaway allocation
  ends that tab alone, and the page is told the session ended.
- **Ending and limits:** a session's process is killed when its tab closes
  or the server stops, on Unix with its process group; should the server be
  killed, the process ends its group itself. On Windows the server puts the
  process in a job object as soon as it starts (through `win32job`, a safe
  API) and holds its only handle; the job kills the process and everything
  it started when the session ends, or when the server dies, since the
  system then closes the handle. A program the app runs inherits the
  variable but serves as it would anywhere. Connections to the session's
  port are read side by side, each for 2 s at most, so none holds up a
  session or the server stopping. `Server::session_memory(bytes)` and
  `session_cpu(time)` cap each process on Unix (through rustix, so the
  crate keeps `unsafe_code = "deny"`): on Linux from the moment it starts
  (`prlimit` from the server, which refuses a process that took more before
  it was set), elsewhere from when it reaches `serve`;
  macOS has no memory limit, so there only the CPU limit applies, and on
  Windows neither is applied.
- **`Isolation::Threads`** keeps 0.0.18's behaviour, for tests and programs
  that cannot start themselves again; `Server::session_command(program,
  args)` starts the process some other way.
- **Python:** `rs_rich.tui.Server` runs the script again with
  `sys.executable` and the script's absolute path (threads in a REPL or a
  Jupyter kernel), and takes `isolation("threads")`, `session_memory(bytes)`
  and `session_cpu(seconds)`.
- **Tests:** the new `isolated` example is served as a process of its own
  while tabs crash, hang and run out of memory on purpose; the others, new
  tabs and the server carry on, and a killed server takes its sessions
  with it; `session_probe` checks a killed server's orphans, a program the
  app runs, strangers on the session's port, and limits on one CPU. On
  Windows, a new CI job checks that a closed tab, a stopped server and a
  killed server each end the session's process and the program it started.
  **Docs:** [One process per
  session](docs/guide/intuituive/web.md#one-process-per-session).

## 0.0.18 cohort

Published on 2026-10-11: record 0.0.6 / interact 0.0.6 / ratatui 0.0.3 /
intuituive 0.0.3 / embed 0.0.1 (new) / web 0.0.1 (new) / CLI 0.0.18, and
`rs-rich` 0.0.6 on PyPI with `rs_rich.tui`. The 0.0.17 cohort's data 0.0.2
and CLI 0.0.17 were published the same day, just before it. Unchanged and
already published: core 0.0.9, macros 0.0.3, plugin API 0.0.3, art 0.0.12,
diagram 0.0.2, mermaid 0.0.5, lumis 0.0.3, ext 0.0.15, micro 0.0.4. See
[the release notes](docs/releases/0.0.18.md).

### intuiTUIve in Python: `rs_rich.tui` (rs-rich 0.0.6 on PyPI)

0.0.18 workstream 8.

- **The whole framework, with the Rust API name for name**, so one guide
  serves both: `App`, `Ctx`, `Driver`, `signal`, `memo`, `watch`,
  `every`, every node builder (`text`, `label`, `renderable`, `leaf`,
  `column`, `row`, `grid`, `each`, `switch`, `list`, the scrolls,
  `component` and `repeating` for `rs_rich.interact` components, `Log`)
  and every builder on `Node`, every widget (`table`, `table_with`,
  `virtual_table`, `virtual_list`, `tabs`, `tree`, `tree_with`,
  `tree_lazy`, `calendar`, the split panes, `menu_bar`, `open_menu`,
  `context_menu`), the palette and help, toasts, animations, themes, the
  stylesheet, accessibility and linear mode (`Driver.accessibility()`).
  Where Rust has a closure, Python takes a function:
  `text(lambda: f"{count.get()}").on_key("+", lambda cx: count.update(lambda c: c + 1))`.
- **Signals hold Python objects.** `update(f)` keeps a value `f` changed in
  place (`f` returning `None`). A signal given to a widget that needs a
  typed one (a selection, a ratio, a `datetime.date`, a tree path) becomes
  one, keeping its value, and what read it before follows it.
- **Widgets in Python:** subclass `Widget` (`draw`, `event`, `measure`,
  `layout`, `scroll`, `caret`, `role`, `access_state`, ...) and make a node
  with `widget(w)`. The `DrawCx`, `Canvas`, `EventCx`, `MeasureCx` and
  `Ctx` they get are lent for the call; using one later raises.
- **Threads:** the app runs on the thread that called `run()` (or a
  `Driver` method), with the GIL released while it waits; callbacks take it
  back. `spawn` runs Python work on a thread of its own, `spawn_async`
  awaits a coroutine on an asyncio loop (a new one, or `loop=` yours),
  `resource` loads in the background, and `Proxy` reaches the app from any
  thread. Signals, memos, logs and resources raise `RuntimeError` on
  another thread.
- **Exceptions:** one raised by any callback stops the app and is raised
  from `run()` (or the `Driver` call that ran it), after the terminal is
  restored. A served session reports it to `sys.unraisablehook`. A
  resource's fetch raising is the resource failing.
- **Headless testing:** `run(app, script)` with `rs_rich.interact`'s
  `Script` returns a `Run` (frames, the last screen, the bytes, whether the
  app quit), and `App.render_with` as in Rust.
- **Serving and embedding:** `serve` and `Server` (rs-rich-web) run one app
  per browser tab, each on its session's thread; `terminal`,
  `terminal_with` and `ReplayHost`, and `web_view` with `ProgramEngine`
  (rs-rich-embed) put a program or a web page in a pane.
- **Not bound,** listed in the guide: traits other than `Widget`
  implemented in Python (`PtyHost`, `WebEngine`), the Rust `Console` a
  widget's contexts hold, `App::console`, `App::run_on` with a backend of
  your own, termion and termwiz (off in the wheel), and type-filtered
  drops (a Python drop target takes any dragged Python value).
- **Type stubs** for every class in `_native.pyi`, checked against the
  module by a test; the to-do app, the planner and the counter (also
  served to a browser) ported to Python under `crates/rich-py/examples/tui`
  and tested as the Rust ones are; a guide page,
  [intuiTUIve from Python](docs/guide/intuituive/python.md), and a page on
  the Python site.

### 0.0.18 release test: fixes from three audits

- **A masked line prompt turns echo off before it shows.** It wrote the
  prompt, then turned echo off, so a key typed as soon as the prompt showed
  (a pasted password, a script typing ahead) was echoed by the terminal.
  `LineIo::prompt_secret` writes the prompt once echo is off; its default
  writes, then reads, as before.
- **Linear mode writes no terminal controls.** An accessible name, a value,
  a toast or an announcement holding ESC, a C1 control, CR or BS reached
  the terminal raw in linear mode (a label could clear the screen, an
  announcement write the clipboard through OSC 52). Each control is now
  written as a visible symbol, as on the cell screen.
- **Linear mode writes every node, however few rows the terminal has.**
  It laid out in the terminal's rows, so on a short terminal (or a new
  pseudo-terminal that reports no size) the nodes below were never written
  and Tab moved the focus onto them in silence. It now lays out as tall as
  the content (up to 4096 rows), 80 columns wide when the terminal reports
  none.
- **Linear mode shows the cursor again after a suspend.** Back from
  Ctrl+Z or `fg`, the session hid the cursor and nothing showed it again.
- **A lazy tree whose children repeat an ancestor's key opens level by
  level.** `tree_lazy` grew every loaded level, open or not, so a child
  with the key of its own ancestor (a folder linked back up the tree)
  recursed until the process aborted. Only open items are grown now.
- **Large stylesheet padding no longer overflows.** `padding: 40000` (or
  65535 with a border) parsed, then panicked at draw time in a debug build
  and wrapped in a release one; the insets saturate.
- **Stylesheet comments and quoted values hold any character.** A comment
  holding a brace, a comment between a selector and its `{`, and a quoted
  `border-title` holding `;` or `/*` were rejected or cut short; comments
  are blanked first and quoted strings are kept whole. A sheet with a bad
  rule is still reported and not applied, with the last good one kept.
- **The wheel over a table's header scrolls the rows** again, as over the
  rows.
- **Browsh's server listens on this computer only.** `BrowshEngine::new()`
  started `browsh --http-server-mode` with Browsh's own configuration,
  whose default binds every interface, so anyone who could reach the
  machine could have it fetch and render pages from there. The engine
  now starts Browsh with a configuration of its own (a temporary
  directory, removed when the engine goes) that binds `127.0.0.1`, in a
  process group of its own that is killed, Firefox included, when the
  engine goes; and spaces and control characters in an address are
  percent-encoded in the request to it rather than written raw.
- **A served program can no longer write the browser's clipboard.** The
  xterm.js page put every OSC 52 copy on the clipboard, in program mode
  too, so any output (`cat` of a hostile file, a fetched page, a log
  line) could silently replace it with a command for a later paste. The
  page now registers its OSC 52 handler for apps only, whose copies the
  server itself sends.
- **A web view address can no longer become a browser's option.**
  `ProgramEngine` put the address on the terminal browser's command line
  as it was, so a link such as `--renderer-cmd-prefix=…` (from a feed, a
  document, a paste) reached Carbonyl, a Chromium, as a switch that runs
  a command. Now `open` refuses an address that starts with `-` or has a
  control character in it, and Carbonyl, found by `detect`, gets `--`
  before the address.
- **No spelling of a sandbox switch reaches Chrome.** `ChromeEngine`
  dropped only arguments that began `--no-sandbox`, but Chromium also
  reads `-no-sandbox` (and `/no-sandbox` and any case on Windows), and
  `--disable-setuid-sandbox`, `--disable-seccomp-filter-sandbox` and
  their kin passed untouched. Switches are now normalised (dashes, a
  Windows slash, case, a value) before the filter, which drops every
  switch that turns part of the sandbox off, `--single-process` and
  `--no-zygote` included.
- **Chrome's profile directory is private.** It was created with the
  default mode (world-readable on most systems); it is now `0700` on
  Unix. The `ChromeEngine` docs and the embed guide now say that its
  DevTools port on `127.0.0.1` can be reached by other local users while
  the view is open.
- **A slow request no longer holds an rs-rich-web server's connection for
  ever.** The 10-second limit on sending a request applied to each read,
  so a client sending a byte every few seconds kept its connection (and
  thread) for hours, and 64 such clients locked everyone else out with
  connections closed unanswered. Now the whole request head must arrive
  within 10 seconds, and a connection past the 64 still sending theirs is
  answered `503`.
- **A paste flood no longer grows a served program's memory without
  bound.** `LocalPty::write` queued every input on an unbounded channel,
  so a page pasting into a program that was not reading its input (or
  reading slower than the network) could exhaust the server's memory.
  Now `LocalPty` holds at most 1 MiB of unread input and refuses more
  with `WouldBlock`, and an rs-rich-web program session stops reading the
  page while the program's input is full, so the flood waits in the
  network.
- **A program that ignores `SIGHUP` no longer outlives its session.**
  rs-rich-embed's `LocalPty` only hung up on its program, so one that
  ignored the hang-up kept running after its pane, its browser session,
  `Handle::stop` and `rich serve` were gone, and its waiter thread waited
  for ever. Now a program still running a second after it was hung up on
  is killed with `SIGKILL`, with its whole process group (Unix);
  `Handle::stop` returns once every program has ended, a request still
  arriving when it stops is not served, the new `Server::run_until` serves
  until a closure returns, and `rich serve` waits for Ctrl+C, `SIGTERM`
  or `SIGHUP` and exits only once every program has ended.
- **`rich record` no longer hangs on a program that asks and does not
  read.** The answers to a program's queries were written from the thread
  that reads its output, with a blocking write, holding the lock typing
  needs: a program that asked the device attributes many times without
  reading its input (a `cat` of a file full of them, a raw-mode program)
  filled its input queue, its output stopped being read, and the tape's
  next keystroke never returned. Typing and answers now go through a
  writer thread of their own: output is always read, typing never waits on
  the program, and answers beyond 4 KiB waiting are dropped.
- **rs-rich-record's emulator ends a control sequence where vt100 does.**
  An ESC inside a CSI sequence was kept as part of it, so a query after a
  sequence cut off (`ESC [ 1 ESC [ c`) went unanswered and the program
  waited two seconds; and a CSI that never ended kept every byte after it.
  ESC now starts a new sequence, CAN and SUB cancel one, and at most 32
  bytes of a sequence are kept (a longer one is no query).
- **A large paste no longer freezes a termion session.** While a
  bracketed paste's end had not arrived, the termion backend searched the
  whole paste for it again after every 1 KiB read: quadratic, 13 seconds
  for 1 MiB and minutes for 4 MiB, with no repaint meanwhile. It now
  searches only what each read added.
- **A terminal that hangs up ends a termion or termwiz session's input.**
  When the terminal went away but the process lived on, its reads
  returned nothing at once, forever: the session spun at full CPU (and
  termion, with part of a sequence pending, never returned). Reading
  nothing from a terminal that said it had input is now the end of input,
  an error from `read`.
- **termwiz reads a sequence split across reads whole.** termwiz's own
  reader read a sequence that one read cut short (`ESC [ 1 ; 5`, then `C`
  30 ms later; half a mouse report) as typed characters at once: Alt+[,
  `1`, `;` and so on, typed into whatever had the focus. On Unix the
  session now reads the terminal itself and gives termwiz's parser what it
  read, holding a sequence cut short until its rest comes, as the termion
  backend does: the same event as from one read, Ctrl+Right and the mouse
  press. A sequence's rest is waited for up to 500 ms (no one types
  `ESC [`), an ESC alone 100 ms before it is the Esc key; both backends
  wait so.
- **An inline termion or termwiz session finds the cursor among keys.**
  Inline with the mouse, these backends ask the terminal for the cursor's
  row, and took the first `ESC [` in what came back for the answer: a key
  or mouse report that arrived just before it (an arrow key, the wheel)
  made the session wait its full two seconds before the first paint, place
  the region at row 0 so clicks missed, and drop the key. The answer is
  now found wherever it is among other input, and the keys that came with
  it are read as typed.
- **termwiz leaves xterm's modifyOtherKeys off.** termwiz's raw mode
  turns modifyOtherKeys up to level 2 and its cooked mode leaves it at 1;
  only dropping its terminal set it back to 0. So after Ctrl+Z, SIGTERM,
  SIGHUP or SIGQUIT, and for a program handed the terminal, the shell or
  the program got `CSI 27;…~` for keys such as Shift+Enter. Every way the
  session gives the terminal back now sets it to 0, and taking it back
  sets it up again.
- **A suspend while a session starts no longer pushes the kitty flags
  twice.** The session marked the kitty keyboard flags pushed before it
  wrote the push: a SIGTSTP in between popped nothing, pushed them on
  `fg`, and the session then pushed them again, one push more than the one
  pop at exit. They are now marked with the write that pushes them, which
  a suspend waits for.
- **Answers to a program's queries are capped, and the terminal pane
  sends them.** rs-rich-record's emulator kept every answer it owed (the
  device attributes, the synchronized output report) until taken, and
  rs-rich-embed's terminal pane never took them: a program that asked over
  and over grew memory without end, and one that asked at its start (any
  rs-rich-interact session) waited two seconds for an answer that never
  came. The pane now sends the answers to its program, and at most 4 KiB
  of them wait to be taken; more are dropped.

### Synchronized output (rs-rich-interact 0.0.6, rs-rich-intuituive 0.0.3)

0.0.18 workstream 11. rs-rich-record changes with it.

- **Each frame is one synchronized update** (DEC private mode 2026) on a
  terminal that has it: the painter's bytes for a frame, the final cursor
  placement and show or hide included, go out between `CSI ? 2026 h` and
  `CSI ? 2026 l` in one write, so the terminal shows the frame whole. No
  tearing, and a screen reader following the cursor no longer sees it jump
  across each changed region. A frame with nothing to paint writes nothing
  at all. Inline and on the alternate screen, with crossterm, termion and
  termwiz, from the event loop and from intuiTUIve's `App::run_on`.
- **Detected without a wait.** The session asks DECRQM
  (`CSI ? 2026 $ p`) in the kitty keyboard protocol's start-up round trip,
  ahead of the kitty query and the device attributes query that ends it,
  and reads the answers itself: the mode reported set or reset means yes;
  not recognised, permanently reset or no report, no. Asked once per
  process, on Unix, only when the kitty query is: a `legacy_keys` session,
  a termion session or one whose standard output is not the terminal asks
  nothing. The session now sends the kitty query itself on every backend
  (crossterm's own query is no longer used on Unix).
- **Forcing it:** `RICH_SYNC_OUTPUT=0` turns it off and `=1` on (for a
  terminal that has it but does not answer, and for Windows, where nothing
  reads the answer); `SessionOptions::synchronized_output: Option<bool>`
  and intuiTUIve's `App::synchronized_output(Option<bool>)` force it from
  code, over the environment. `Session::synchronized_output` says whether
  it is on.
- **Never left open.** Exit, a hand-off, Ctrl+Z or SIGTSTP, a panic and the
  signals that restore the terminal end any update a cut-short write left
  open, before the other modes.
- **API:** `Backend::synchronized_output` and `Backend::write_frame` (both
  with defaults; the event loop and intuiTUIve write frames through
  `write_frame`), `Headless::synchronized_output` for tests that assert the
  wrapped bytes, and `synchronized_update` with the
  `BEGIN_SYNCHRONIZED_UPDATE` and `END_SYNCHRONIZED_UPDATE` constants for a
  loop of your own. **Breaking:** `SessionOptions` has a new field (build it
  with `..SessionOptions::default()`).
- **rs-rich-record** answers DECRQM for mode 2026 (recognised, reset), so
  programs in tapes write synchronized updates; its VT emulator passes over
  the mode, so tapes show the same screens.

### intuiTUIve: rough spots (rs-rich-intuituive 0.0.3, rs-rich-record 0.0.6)

Workstream 1 of the [0.0.18 plan](docs/plans/0.0.18.md).

- **`.autofocus()`**: a node that takes the focus when its screen opens
  (and whenever the screen has nothing focused), before the first
  focusable node. One that `each` or `switch` builds after the first frame
  still takes the focus, unless the user has moved it. The `planner` example's tree uses it instead of taking
  the menu bar out of the Tab order.
- **`Resize` after the first frame**: a widget is told its size once it
  has first been laid out, not only when the size later changes.
- **`Ctx::palette()`**, to pair with `Ctx::help()`; `command_palette()`
  is a deprecated alias for one release.
- **Tapes use the mouse and every key** (rs-rich-record): `Click`,
  `RightClick`, `MiddleClick`, `DoubleClick`, `Drag`, `ScrollUp`,
  `ScrollDown` and `MouseMove` at a cell, sent as SGR mouse reports; and
  `F1` to `F24`, `Shift+Tab`, `Delete`, `Insert` and `Alt+x`, and the
  arrows, `Home`, `End`, `PageUp`, `PageDown`, `Delete`, `Insert` and
  function keys after `Shift+`, `Alt+` and `Ctrl+` (`Alt+Left`,
  `Ctrl+PageDown`). A cell too large for an SGR report is an error. The
  `intuituive` tape records the planner's menu bar and the `files` tape
  its right-click menu.

### intuiTUIve and interact: the kitty keyboard protocol (rs-rich-interact 0.0.6)

0.0.18 workstream 2. rs-rich-intuituive changes with it.

- **The kitty keyboard protocol** is turned on when the terminal answers
  its query (asked once per process), and popped on every way out: exit,
  a hand-off, Ctrl+Z or SIGTSTP (and pushed again on `fg`), a panic and
  the signals that restore the terminal. `SessionOptions::legacy_keys`
  and intuiTUIve's `App::legacy_keys` keep it off; `Session::kitty_keys`
  says whether it is on. It is only asked when standard output is the
  terminal: crossterm writes the query there, so in
  `answer=$(rich write)` it would land in the answer.
- **Key names mean the key.** `"ctrl+i"` parses as Ctrl+I, not Tab;
  likewise `"ctrl+m"`, `"ctrl+["`, `"ctrl+\"`, `"ctrl+]"`, `"ctrl+^"`,
  `"ctrl+_"` and `"ctrl+@"`, which 0.0.5 turned into the keys a legacy
  terminal sends. `"ctrl+shift+a"` is Ctrl and `A`. `"shift+a"` is still
  `"A"`, F1 to F24 are names, and `"shift+1"` is still an error.
- **Legacy terminals keep working.** Bindings match with the new
  `Key::matches`: a key from a terminal without the protocol fires a
  binding for any key that terminal sends alike (its Tab fires `tab` or
  `ctrl+i`, its Enter `enter` or `ctrl+m`, its Esc `esc` or `ctrl+[`, its
  Ctrl+4 to 7 `ctrl+\ ] ^ _`, its Ctrl+Space `ctrl+@`, its Ctrl+H `ctrl+h`
  or `backspace`), and a binding that names the key wins over one it
  could be. With the protocol, every key is `Key::exact` and fires only
  its own binding. `Keymap`, item actions, the action menu, plugin
  components and intuiTUIve's `on_key`, palette and help keys all match
  this way. `Key::legacy` gives the key a legacy terminal sends for a
  name, and `Key::pick` chooses among bindings.
- **Releases and repeats.** A key let go arrives as the new
  `Event::KeyUp` (and in intuiTUIve as `WidgetEvent::KeyUp`, offered along
  the focused path); a repeat arrives as a press. Bindings fire on presses
  only. Plain text keys, Enter, Tab and Backspace have no release: the session does not ask for
  every key as an escape code (flag 8), which would cost typed text its
  shifted and layout forms. `event::from_crossterm` delivers releases too, and the new
  `event::from_crossterm_kitty` reads keys from a terminal with the
  protocol pushed. Python components (`rs_rich.interact`) are not handed
  releases.
- **Headless:** scripted keys arrive as a legacy terminal sends them, as
  before; `Headless::exact_keys` stands for a terminal with the protocol,
  and `Script::key_up` lets a key go. The PTY tests answer the protocol's
  query, as a terminal with it and as one without.
- **Breaking:** `Key` has a private field, so it is built with
  `Key::new`, `Key::with`, `Key::ctrl`, `Key::char` or `Key::parse`, not a
  struct literal; `Event` has a new variant; `SessionOptions` has a new
  field (build it with `..SessionOptions::default()`).
- **rs-rich-record** answers a program's device-attributes query (as a
  VT220, the way xterm does by default), so a recorded program that asks
  for the kitty protocol does not wait two seconds for an answer.


### intuiTUIve: accessibility (rs-rich-intuituive 0.0.3)

0.0.18 workstream 6.

- **`App::accessible(true)`**, on by default with `INTUITUIVE_ACCESSIBLE`
  or `RICH_A11Y=screen-reader`.
  - The terminal's cursor sits on what has the focus: the caret, else the
    selected row, cell, tab or menu entry. Screen readers follow it with
    no bridge.
  - Text mode: boxes are drawn as blanks (titles stay), there is no
    colour, a `>` marks the selected item in lists, tables, trees and tab
    strips, and animations jump to their end.
- **Roles and names:**
  - `Widget::role` (default `Role::Group`) is set by every built-in.
  - `Widget::cursor` gives the selected item's place.
  - `Node::role`, `Node::label` (the accessible name) and `Node::live`
    (announce changes).
  - `Driver::accessibility()` returns the tree: depth, role, name, value,
    focus and place.
  - `Driver::cursor()` (workstream 7): where the last frame left the
    caret, for a renderer that draws the screen itself (rs-rich-web's DOM
    renderer).
- **Announcements:** toasts, a screen or dialog opening, live nodes and
  `Ctx::announce` go to `App::announcer` and wait in
  `Driver::take_announcements()`.

### intuiTUIve: accessibility states and linear mode (rs-rich-intuituive 0.0.3)

0.0.18 workstream 10.

- **States:** every `AccessNode` has an `AccessState`: `expanded`,
  `checked`, `selected`, `busy` and `position` (the selected item's place,
  `3 of 10`).
  - Built-ins set what they know: lists, tables, tabs, menus, the menu
    bar and the calendar their selection and its place; trees whether the
    selected item is expanded; `tree_lazy` busy while a level loads.
  - `Node::expanded_when`, `checked_when` and `busy_when` set them in
    code, like `selected_when` (which now counts too). `checked_when`
    makes the role `Role::CheckBox` unless one is set; `Role::Switch` is
    new too.
  - `Widget::access_state` is the hook for widgets of your own, and
    `AccessState::item` builds a widget of items' states.
  - The states, and whether a node is disabled, are read while it draws,
    so a signal only they read draws it again when it changes, and linear
    mode writes the new line.
  - `AccessNode::aria_attributes()` maps the states to ARIA for a browser.
- **`Node::access_hidden`** (ARIA's `aria-hidden`): a node and everything
  inside it leave the accessibility tree, linear mode's lines,
  announcements and the names of what holds it, and are still drawn.
  Nothing inside it is in the Tab order or takes the focus when its screen
  opens; a click still reaches it. In text mode, menu separators and a
  split's divider are blank.
- **Linear mode:** `App::linear(true)`, or `INTUITUIVE_ACCESSIBLE=linear`.
  - No screen is drawn: the tree is written as lines of text in reading
    order, then only the lines that changed after each event.
  - The focus moving is written as `→` and the focused node's line, and
    toasts and `Ctx::announce` as their text.
  - There is no cursor addressing, alternate screen, mouse or colour,
    and no synchronized output (workstream 11), since lines are appended
    as they come.
  - `AccessNode::describe()` gives the line (`Files, list, 3 of 10:
    main.rs, selected`). `Driver::render` returns the lines, and
    `Driver::is_linear` says whether the app is in linear mode.
- **Less cursor travel:** in the cursor mode the cursor is hidden while a
  frame is written and shown again once it is parked on the focus. A frame
  that changes nothing on the screen does not move it.
- **Names:** an `Input` or `TextArea` is a text box. A node named by the
  text it shows speaks for its children, which leave the tree. A tree's
  value is its item's label, without the arrow. A calendar parks the
  cursor on its selected day.
- **Testing with screen readers:** a new guide page with an Orca, NVDA and
  VoiceOver checklist and a results table for the release test, and the
  `access` example it walks through.

### intuiTUIve: stylesheets (rs-rich-intuituive 0.0.3)

0.0.18 workstream 4.

- **`App::stylesheet(css)` and `App::stylesheet_file(path)`** style and
  lay out nodes with a CSS subset. A file is read again when it changes;
  a sheet that does not parse is reported in a toast with its line and
  column, and the last good one stays. `Stylesheet::parse` checks one.
- **Selectors:** kinds (`label`, `table`, `panel`…), `#name`, `.class`,
  `:focus`, `:focus-within`, `:hover`, `:selected`, `:disabled`, and
  descendants. CSS specificity, then order, decides.
- **Properties:** `color`, `background` and `text-style` (with `$name`
  for a theme style's colour); `border` and `border-title`; `size`,
  `min-size`, `max-size`, `padding`, `gap`, `grid-columns`, `grid-rows`,
  `display: none` and `dock`.
- **Colours inherit** under what a node draws. **Code wins** over the
  sheet. States and `class_when` restyle; layout follows kinds, names and
  fixed classes.
- **New builders:** `Node::class`, `Node::class_when`,
  `Node::selected_when` and `Node::disabled_when`. A disabled node leaves
  the Tab order (a focused one gives the focus to the next node), and its
  widget, click and key handlers see no input. The inspector
  shows classes.
### intuiTUIve: the widget gaps (rs-rich-intuituive 0.0.3)

0.0.18 workstream 3.

- **Scrolling across:** `scroll_x` and `scroll_both` (and
  `scroll_both_with`, with both offsets in signals) for content wider
  than the view, with a bar along the bottom. ←/→, Shift+wheel and
  Shift+PgUp/PgDn move across, and a focused node far to the right is
  scrolled into view.
- **Tables:** `table_with(columns, rows, selected, TableOptions)`.
  - `.sort(signal)`: a header click sorts by that column and a second
    click reverses; `s` sorts by the cursor's column; the header shows ▲
    or ▼.
  - `.sort_rows(signal)`: sorts plain rows itself, numbers as numbers.
  - `.cells(signal)`: a cell cursor, moved with ←/→ (or h/l) and clicks,
    in the `selected.cell` style.
  - `.resizable()`: drag the gap after a header to resize a column.
- **`tree_lazy(roots, children, selected)`** loads each level on another
  thread when it is opened, showing "loading…" until it arrives and an
  error in red if it fails. `LazyItem::leaf` and `LazyItem::branch` make
  its items.
- **`.hover_style(style)`** lays a style over a node while the pointer is
  over it. That style, and `.focus_style` on a container, now cover the
  container's whole area, gaps included.
- **`.tooltip(markup)`** shows after the pointer rests on a node for
  600 ms, or below the focused node with F1. A key, a click or moving
  away hides it. It uses the theme's `tooltip` style.
- **Drag-and-drop:** `.draggable(value)` and `.on_drop(|value: &T, cx|
  …)`. The type is checked: a target only lights up for, and only takes,
  values of its type. The source is dimmed while a drag lasts, the target
  is drawn in `drop.target`, and Esc cancels.

### Serving intuiTUIve apps to a browser: rs-rich-web 0.0.1 (new)

A new crate, from the 0.0.18 plan's workstream 7: phase 1 (intuiTUIve
apps), phase 2 (any terminal program) and phase 3 (a DOM renderer). The
framework does not depend on it, and the CLI only behind its
off-by-default `serve` feature, so neither gains network code by default.

- **`rich_web::serve(addr, || app())`** serves an intuiTUIve app. Each
  browser tab gets its own `App` on its own thread, driven through the
  framework's `Driver`: its frames go to the page as terminal output over a
  WebSocket, and timers, animations and background tasks keep drawing
  while nobody types. `Server` adds options (`max_sessions`, `token`,
  `title`, `allow_origin`) and `spawn`, which serves in the background and
  returns a `Handle` that stops it. The framework is re-exported as
  `rich_web::intuituive`.
- **The page** is xterm.js 6.0.0 with its fit add-on 0.11.0 (MIT),
  vendored unchanged from the npm registry, checked against pinned SHA-256
  hashes by `build.rs`, embedded in the binary and served by the crate. It
  is never fetched from a CDN. The page fills the window, follows its size,
  and puts text the app copies (OSC 52) on the browser's clipboard,
  answering each copy so the app says "Copied" only when it worked.
- **Input**: `rich_web::input::decode` turns what xterm.js sends (control
  characters, `ESC [`/`ESC O` keys with modifiers, F1 to F20, SGR mouse
  reports, bracketed pastes) into the rs-rich-interact events crossterm
  gives for the same bytes, so key bindings behave the same in a terminal
  and in a browser.
- **Security, on by default:** the server listens only where it is told
  and warns about a non-loopback address. A random 128-bit token from the
  operating system is part of the printed URL, and the page and the
  WebSocket both require it. The WebSocket's `Origin` must be the server's
  own page, sent to a `Host` the server answers to (against DNS rebinding),
  or an origin allowed with `allow_origin`. Sessions are capped (8 by
  default, `503` beyond). The page has a content security policy and
  cannot be framed. There is no other authentication and no TLS; the
  guide says to put a proxy that has both in front.
- **Synchronous:** `tungstenite` for the WebSocket and `httparse` (already
  in its tree) for the few request heads served, with no async runtime.
  The plan's default was `tiny_http` for HTTP. It was dropped because its
  upgraded connection is one boxed blocking stream with no read timeout, so
  a session could not both wait for keys and draw on a timer. The crate
  adds four packages to the lock file: tungstenite, sha1, data-encoding and
  chacha20.
- An example, `cargo run -p rs-rich-web --example serve`, and a guide page,
  [Serving an app to a browser](docs/guide/intuituive/web.md). The tests
  cover the token, origin and session-cap checks (accepting and refusing),
  the input decoding table, and an app driven end to end over a real
  WebSocket on 127.0.0.1. The page was also tried in headless Chromium:
  typing, keys, a click, a resize and quitting.
- Registered like the other crates: the workspace, `RELEASES.toml`, the
  release scripts and their tests, the release workflow's tags, the
  release-readiness pattern, the CI feature matrix, `AGENTS.md`,
  `CONTRIBUTING.md`, the architecture, branching and porting docs, and the
  README and home-page tables.
- **Phase 2, any terminal program:** `rich_web::serve_command(addr,
  command)`, `Server::bind_command` and `Server::bind_host` run a program
  per tab on a pseudo-terminal, through rs-rich-embed's `PtyHost`
  (`LocalPty` for a command: a PTY on Unix, ConPTY on Windows; any other
  host with `bind_host`), and stream it to the same xterm.js page. Keys,
  pastes and mouse reports go to the program as the page sent them, and
  resizes resize the PTY. Its output goes to the page as binary messages,
  and the page keeps 5,000 lines of scrollback. Its exit closes the
  session with its status as the close's reason, which the page shows
  ("The program exited with code 3."); a program that cannot start says
  why; closing the tab or stopping the server ends the program. The same
  token, origin and session-cap checks as an app's, and the page's title
  is the program's name. `Command`, `ExitStatus`, `LocalPty` and `PtyHost`
  are re-exported.
- **Output for a slow page is capped:** `Server::max_buffered` (1 MiB by
  default). The page acknowledges each message once drawn; with half the
  cap sent and not drawn, the server stops reading the host, and
  `LocalPty::backpressure` (new in rs-rich-embed) stops reading the
  program at the other half, so its writes wait as on a slow terminal.
  Memory stays bounded and nothing is dropped.
- **Phase 3, a DOM renderer:** `Renderer::Dom` (`Server::renderer`, or
  `&renderer=dom` in the page's address; xterm.js stays the default, and
  a program's page is always xterm.js) draws an app as a grid of styled
  spans instead of xterm.js, in xterm.js's palette, with wide characters
  and the text box's caret (the new `Driver::cursor`). Over the grid, the
  app's accessibility tree as ARIA: each node of `Driver::accessibility()`
  an element with `AccessNode::aria_attributes()`, nested by depth and laid
  over the cells it describes; a widget of items gets an element for its
  selected item with the item's states; text and a status read as
  `AccessNode::describe()`. The focused node is focused in the page, and
  announcements go to `aria-live` regions (assertive when urgent). Keys,
  clicks, drags, the wheel, pointer movement and pastes become the bytes
  the xterm.js page sends, so the server reads both pages alike. The page's
  script is the crate's own (no third-party code), under the same content
  security policy.
- **The DOM wire protocol is versioned** (1) and documented in the `dom`
  module: `hello`, `frame` (changed lines as styled runs, new styles as
  CSS, the caret), `tree`, `say` and `copy`, as JSON; the page sends what
  the xterm.js page sends. `rich_web::dom::Dom` builds the messages from a
  `Driver`, so they are tested without a browser.
- **`rich serve` in the CLI (rs-rich-cli 0.0.18),** behind a new `serve`
  feature, off by default (a default build has no `serve` and no network
  server, and the Python wheel does not enable it): `rich serve [--bind
  ADDR] [--port N] [--max-sessions N] [--allow-origin ORIGIN]... --
  PROGRAM [ARGS...]` serves the program as above, on `127.0.0.1:8080`
  unless told otherwise, prints the address with its token, and stops on
  Ctrl+C. `rich doctor` reports the feature. CI checks the CLI with it in
  the feature matrix and on the MSRV, and runs its tests (the parsing,
  help, the spec/parser drift checks, and a program served on 127.0.0.1).
  `docs/cli-reference.md` is generated from a default build, so it does
  not list `serve`; `docs/cli.md` and the CLI guide do.
- **Tests (phases 2 and 3):** a program end to end over a real WebSocket
  on 127.0.0.1 (`sh` on a PTY, Unix: input, a resize seen by `stty size`,
  the exit status); a test host anywhere (input, pastes and mouse reports
  passed through, resizes, the exit, the page closing and the server
  stopping both ending the program); a program that cannot start; the
  token, origin and session-cap refusals for a program; output held for a
  page that draws nothing, then all of it, nothing dropped, once it
  catches up. The DOM renderer's messages for a small app (every line
  first, then only what changed; styles as CSS; a wide character; the
  caret; the tree with roles, names, item states and the focus; a focus
  move; a resize; announcements and copies), the page chosen by the server
  or the address, and a DOM session end to end (keys, an announcement, a
  copy answered by the page, a click). The test helpers moved to
  `tests/common`.
- rs-rich-web now depends on rs-rich-embed (and through it rs-rich-record
  and portable-pty): `RELEASES.toml` publishes rs-rich-embed first, and
  `AGENTS.md`, the architecture and branching docs say so.

### intuiTUIve and interact: termion and termwiz backends (rs-rich-interact 0.0.6)

0.0.18 workstream 5 (#677, #678). rs-rich-intuituive changes with it.

- **Two more backends**, each behind an off-by-default feature of
  rs-rich-interact: `termion` (Unix only) and `termwiz` (Unix and Windows).
  `SessionOptions::backend` picks the library a `Session` drives the
  terminal with, `BackendKind::Crossterm` by default; `BackendKind::ALL`
  lists the ones the build has, and `BackendKind::from_name` reads one. The
  library reads keys, the mouse, pastes and resizes, measures the terminal
  and turns raw mode and the alternate screen on and off; the modes a
  session turns on, and their return on every way out (finishing, Ctrl+C,
  a panic, SIGTERM and the rest, a hand-off, Ctrl+Z, SIGTSTP and `fg`), are
  the same code for all three. `run`, `EventLoop::terminal` and the CLI's
  components keep crossterm.
- **Each with its own event conversion:** `event::from_termion`, and
  `event::from_termwiz` and `event::from_termwiz_kitty`, for loops of your
  own. Neither library says which button a release is of, so they take
  the new `HeldButton`. termion's parser wants an event's bytes complete
  and has no timeout, so the termion session waits on the terminal itself,
  cuts what it reads into events (pastes, whose markers termion does not
  know, cut out first), and drops what termion cannot read.
- **And its own probes.** termwiz's probe (terminfo, `COLORTERM`,
  `NO_COLOR`) gives the colours `run` paints with
  (`Session::color_system`) and whether the mouse and bracketed paste are
  used. The kitty keyboard protocol stays where the library can read it:
  crossterm as before; termwiz asks the terminal itself and pushes only
  the protocol's first flag (exact keys, no releases), since it does not
  read release reports; termion never turns it on, so its keys arrive as a
  legacy terminal sends them (`Key::matches`), as workstream 2 defines.
  termion reports no modifiers with the mouse and no movement without a
  button.
- **intuiTUIve:** `App::run_with(backend)` runs on the one named, and
  `App::run` is `run_with(BackendKind::Crossterm)`; the crate's `termion`
  and `termwiz` features turn on rs-rich-interact's. `App::run_on` now
  suspends on Ctrl+Z (on a backend that can, unless a node on the focused
  path binds it) and draws everything again on `fg`, through the new
  `Driver::suspend`. It also stopped drawing over and over after a SIGTSTP
  sent from outside: it never took the session's resumed flag, so every
  read returned another resize.
- **Tests:** a PTY conformance suite (`tests/pty_backends.rs`) runs against
  every backend the build has: one table of the bytes a terminal sends and
  the events they must arrive as (where the library reads them), the kitty
  protocol's keys, resizes, and the terminal given back after finishing,
  Ctrl+C, a panic, a hand-off, Ctrl+Z and SIGTSTP. intuiTUIve runs an app
  on each backend in a PTY too (`tests/backends.rs`). CI builds and tests
  each feature on Linux and macOS.
- **Breaking:** `SessionOptions` has a new field (build it with
  `..SessionOptions::default()`).

### Embedding: a terminal pane and a web view (rs-rich-embed 0.0.1, new; rs-rich-record)

Workstream 9 of the 0.0.18 plan. A new crate, `rs-rich-embed`
(`rich_embed`), puts other programs and web pages inside an intuiTUIve
app. Each pane sits on a trait, as the framework's terminals sit on
`Backend`, so what runs behind it can be swapped, including for one an app
writes itself. See `docs/guide/intuituive/embed.md`.

- **`terminal(cmd)`, a terminal pane:**
  - runs any program: a shell, `htop`, an editor, a terminal browser;
  - keys go to it as an xterm sends them (cursor key modes, modifiers,
    F1 to F24, Alt as Esc), Ctrl+C and Tab included; pastes go bracketed
    when it asked; the mouse goes in the reports it asked for (X10, UTF-8
    or SGR); resizes reach it;
  - its screen is followed by rs-rich-record's `Terminal` (vt100 with
    rich's character widths) and drawn in the pane, with scrollback
    (Shift+PgUp/PgDn, and the wheel when the program does not want the
    mouse);
  - its exit is a signal (`status()`) and a callback (`on_exit`);
  - `release_keys` names keys that go to the app instead, and the program
    is ended when its pane leaves the tree.
- **`PtyHost`**, behind the pane: start, write, resize, read, notify,
  exit status, kill. `LocalPty` runs a program through portable-pty (a
  PTY on Unix, ConPTY on Windows); `ReplayHost` plays bytes back, and its
  `ReplayHandle` feeds output from any thread and reads back what was
  sent. Output arriving on a host's thread wakes the app through its
  `Proxy`, and is read on the app's thread between frames.
- **`web_view(url)`, a web page in a pane,** with an address bar (back,
  forward, reload, the address, a loading mark; Ctrl+L to type an
  address), and the address, title, loading state, back, forward and
  errors as signals on a `Copy` `WebHandle`. Setting the address opens it.
  Keys, the mouse and pastes go to the page, and its size to the engine.
- **`WebEngine`**, behind the view: frames of cells (drawn as they are)
  or pixels (drawn as coloured half blocks; the kitty, sixel and iTerm2
  protocols are not used yet, since intuiTUIve diffs a grid of cells).
  - **`ProgramEngine`** (default): a terminal browser the app names, or the
    first of Carbonyl, Chawan, Browsh, w3m and lynx on `PATH`
    (`RICH_EMBED_BROWSER` overrides), run in a pane.
  - **`ChromeEngine`** (feature `chrome`, off by default): a headless
    Chrome or Chromium the user installs, driven over the DevTools
    protocol with tungstenite on a thread of its own, no async runtime.
    The sandbox stays on (`--no-sandbox` is dropped), the profile is a
    temporary directory removed with the engine, and downloads are
    denied. Screencast frames, `Input.dispatchKeyEvent`,
    `dispatchMouseEvent` and `insertText`.
  - **`BrowshEngine`** (feature `browsh`, off by default): Browsh's HTTP
    server mode, a page as plain text, read-only.
  - No browser is a dependency, ships with the crate, or is downloaded.
- **rs-rich-record:** `Terminal::with_scrollback`, `set_scrollback` and
  `cell_text` (a cell's text as rich prints it, clusters whole), for the
  pane's scrollback and drawing.
- **`LocalPty::backpressure(bytes)`:** past that much unread output, stop
  reading the program, so its writes wait instead of the oldest output
  being dropped (past 16 MiB, without it); with it, nothing is dropped at
  any limit, 16 MiB or more. The exit is still reported after the last
  output. rs-rich-web's program sessions use it.
- **Tests:** the pane against `ReplayHost` (output, keys, the mouse,
  pastes, resizes, scrollback, the exit, released keys, the program ended
  with its pane); `sh -c 'printf hi; exit 3'` on a real PTY (Unix); the
  web view against a fake engine (the signals, the bar, input, cells and a
  pixel frame as half blocks) and `ProgramEngine` over replayed hosts; the
  DevTools message encoding without a browser. `tests/chrome.rs` runs
  against a real Chrome only when `RICH_EMBED_CHROME` names one.
- **`examples/embed.rs`:** a shell pane beside a w3m web view.

## 0.0.17 cohort

ext 0.0.15 / micro 0.0.4 / record 0.0.5 / interact 0.0.5 / ratatui 0.0.2 /
intuituive 0.0.2 / data 0.0.2 / CLI 0.0.17. Unchanged and already published:
core 0.0.9, macros 0.0.3, plugin API 0.0.3, art 0.0.12, diagram 0.0.2,
mermaid 0.0.5, lumis 0.0.3, Python 0.0.5. See
[the release notes](docs/releases/0.0.17.md).

### intuiTUIve: sharp edges fixed (rs-rich-intuituive 0.0.2, rs-rich-interact 0.0.5)

Found by probing the framework the way a new user would; each has a test
in `crates/rich-intuituive/tests/sharp_edges.rs`.

- A memo that wrote a signal it read overflowed the stack and aborted the
  process. Writing a signal while a memo (or a watch's source) computes now
  panics with a message that says what to do instead.
- Writing another signal inside `Signal::update` panicked ("a signal read
  inside its own update") whenever a memo read both. Changes made inside an
  update now reach readers once the outermost update is done.
- A node that wrote a signal it read while drawing redrew forever at full
  CPU. After a few frames in a row its own writes stop drawing it again,
  and a toast names the node; a write once (a scroll keeping the focus in
  view) still draws it again.
- A watch whose callback set its own source ran 100 rounds on every
  update, without end. What is still pending after 100 rounds is dropped,
  and a toast says so.
- Binding Ctrl+C never ran the binding: the app quit first. Ctrl+C now
  quits only when no node on the focused path binds it.
- Key names that parsed but could never fire: `"shift+a"` is now `"A"`;
  `"ctrl+i"`, `"ctrl+m"` and `"ctrl+["` are Tab, Enter and Esc;
  `"ctrl+\"`, `"ctrl+]"`, `"ctrl+^"` and `"ctrl+_"` are Ctrl+4 to 7, as
  terminals send them (rs-rich-interact). `"shift+1"`, `"f0"` and `"f99"`
  are errors, and a binding with no keys (`" "`, `""`) panics, pointing at
  `"space"`.
- Deleting the focused row of an `each` sent the focus to the first
  focusable node; it now goes to the next row, or the one before it if it
  was last.
- A signal made by one app and read by another (two panes of one program)
  showed its value but never redrew the reader; it now does.
- `watch()` outside an app panicked naming `signal()` and `memo()`; each of
  `signal()`, `memo()`, `watch()` and `spawn()` now names itself.
- After a clock jump a timer caught up tick by tick (a day's jump took over
  a second of busy loop); it now ticks once and keeps to its step.
- Toasts did not show on screens under three rows tall or too narrow for
  their box; the newest now shows reversed on the bottom row.

### intuiTUIve examples show the framework (rs-rich-intuituive 0.0.2)

- **`meters`** (new): a board of meters, each a `Widget` of its own. It
  shows retained drawing (a new sample redraws only a meter's inside
  rows), `Focus` and `Resize` events, the pointer over a sparkline, a
  board that sees keys before the focused meter, animations and toasts.
  `--frozen` stops the clock for screenshots.
- **`planner`** (new): the components together. A menu bar, a tree of
  projects, `hsplit`/`vsplit` panes, a calendar for the due date, a
  virtual list of a hundred thousand log rows, and the help and command
  palette made from the bindings.
- **`files`**: the columns are split panes whose dividers drag; every key
  is a described binding, so `?` and Ctrl+P open the built-in help and
  palette; `y` yanks the path to the clipboard; a right click opens a menu.
- **`Ctx::copy(text)`**: a handler puts text on the clipboard, with the
  same toast and hand-off (`Driver::take_copies`) as a mouse selection.
- A right click on a table, list, tree or calendar selects what is under
  it and then reaches the node's own `on_mouse` handler, so a context menu
  is about the row it selected. Only a left press is used up by them.
- The examples' tests are in `tests/meters.rs`, `tests/planner.rs` and
  `tests/files.rs`; the `intuituive` and `files` tapes record them.

### intuiTUIve widgets: rs-rich-intuituive 0.0.2

The components the two example ports showed were missing, built from
`docs/design/intuituive-widgets.md`:

- **`Widget`** (`intuituive::widget`): a public trait for controls written
  outside the crate, with the powers built-in nodes have. A widget:
  - sizes itself and lays out child nodes;
  - draws cells on a clipped `Canvas`;
  - takes keys and mouse events in its own coordinates;
  - holds the focus and places the caret;
  - asks to `redraw()` or to `capture_mouse()`.

  `widget(w)` makes a node of one.
- **The mouse:**
  - Every event (press, release, drag, movement, the wheel) goes to the
    deepest node under the pointer and bubbles up.
  - `Node::on_mouse` handles any event; drags can be captured.
  - Hover is state that a widget reads. The terminal's movement reports
    are turned on only when something asks.
  - A press focuses the deepest focusable node under the pointer.
- **`scroll(child)` and `scroll_with(child, offset)`:**
  - a viewport onto a child laid out at its full height;
  - scrolled by keys, the wheel and a scrollbar, and the focus moving
    inside scrolls its node into view;
  - drawn offscreen with its own damage;
  - clicks and the caret are translated through it.
- **`widgets::table` and `widgets::virtual_table`:** sized columns, a
  sticky header, and a row selection that keys, clicks and the wheel move.
  The virtual one asks only for the rows in view.
- **`widgets::tabs`**, a tab strip. Both it and the tables are built on
  the public trait.
- **`Ctx::popup(anchor, Placement, width, height, build)`:** a modal placed
  next to a node, flipped when there is no room, and closed by Esc or a
  press outside it.
- **`Signal::update` no longer holds the runtime's value table while its
  closure runs**, so the closure can read other signals. That read used to
  panic with a borrow error.
- **Both examples are rebuilt on these:**
  - `examples/files.rs`: the current directory is a `table` with a size
    column, the preview is a `scroll` (J/K and the wheel), the filter prompt
    is a `popup` above the status line, and Yazi-style tabs come from
    `tabs` (`t`, 1–9, Ctrl+W).
  - `examples/scope.rs` draws with rs-rich-ext's new `Chart` instead of its
    own plot code, and the spectroscope uses a log axis.

  Tests are in `tests/widgets.rs`, `tests/components.rs`, `tests/files.rs`
  and `tests/scope.rs`, and the tapes are re-recorded.

### intuiTUIve: every node a widget, and loops of your own (rs-rich-intuituive 0.0.2)

- **Every built-in node is a `Widget`.** Text, labels, renderables,
  columns, rows, grids, panels, padding, `each`, `switch`, logs, hosted
  components and `scroll` are implementations of the public trait, drawn
  by one generic node. The benchmark gate is unchanged.
- **`Widget` trait v2:**
  - `retained()` widgets keep what they drew and send only what changed;
  - `viewport()` and `scroll()` show children through a scrolling window;
  - focus, hover, resize and paste events (`WidgetEvent::Focus`, `Hover`,
    `Resize`, `Paste`);
  - `DrawCx::hovered` redraws only on entering and leaving, and
    `DrawCx::pointer` gives the pointer inside the widget;
  - with nothing focused, keys go to the node under the pointer;
  - helpers: `Canvas::markup`, `render`, `border`, `restyle`, `clear` and
    `scroll_up`; `MeasureCx::measure`, `extent` and `stack`.

  `WidgetEvent` is no longer `Copy`, and is `non_exhaustive`, so a `match`
  on it needs a `_` arm.
- **`App::driver`**: run an app from your own loop. You feed time and
  events, get back the bytes of each frame, and can read the frame as
  cells (`Screen::lines`). `App::run` and `run_on` are built on it.
  `examples/in_ratatui.rs` hosts an app inside a ratatui program.
- **`rich_interact::event::from_crossterm`** is public, for loops that
  read crossterm (rs-rich-interact 0.0.5).
- The split pane highlights its divider from its own pointer tracking and
  redraws only the divider.

Tests are in `tests/widget_v2.rs` and `tests/driver.rs`. termion and
termwiz backends are tracked in #677 and #678.

Fixed after the first round of use:

- `.focus_style()` on a container goes over its children instead of under
  them, and a retained widget with a focus style redraws in full only when
  the focus comes or goes.
- Containers can see keys before the focused node inside them:
  `Widget::previews_keys` and `WidgetEvent::Preview`.
- `EventCx::rect` for keys and paste is on the screen inside a `scroll`,
  so a popup a key handler anchors there lands in the right place.
- The "Copied" toast waits for the loop to say the copy worked:
  `Driver::copied`. `run` tells it only after a successful copy.

### intuiTUIve: more components and features (rs-rich-intuituive 0.0.2)

Components and features that other terminal frameworks, ratatui above all,
leave to each app:

- **`widgets::tree`** and `tree_with`: nested items that expand and
  collapse, with the selection as a path.
- **`widgets::hsplit` / `vsplit`** and `split_with`: two panes with a
  divider that the mouse drags and Alt+arrows nudge.
- **`widgets::calendar`** and `calendar_with`: a month grid over a `Date`
  signal, with the date arithmetic in `Date`.
- **`widgets::virtual_list`**: a list that asks only for the rows in view.
- **The command palette and help** (`Ctx::command_palette`, `Ctx::help`,
  `App::palette_key`, `App::help_key`) list every binding made with a
  description on the focus path, so commands are declared once.
- **`menu`**: `menu_bar`, drop-down menus (`open_menu`) and
  `context_menu` at the pointer.
- **Pop-ups anchor to a node or a `Rect`** (`Anchor`), and
  `EventCx::rect()` and `Ctx::pointer()` give the places to anchor to.
- **Toasts**: `Ctx::toast` and `toast_for`.
- **Animations**: `Ctx::animate` eases a `Signal<f64>` to a value with an
  `Easing`, driven by the app's clock.
- **Text selection with the mouse captured**: a drag where nothing uses
  the press selects text and copies it with OSC 52;
  `App::selectable(false)` turns it off.
- The palette and help group bindings by the node's name only, not by the
  builder that made it.

Tests are in `tests/widgets_more.rs` and `tests/features.rs`. The design
note records the gaps building these found in the `Widget` trait, and
designs for accessibility and serving an app to a browser, which are not
built.

### Charts: rs-rich-ext 0.0.15

- **`chart::Chart`**, laid out as ratatui's `Chart`, so a ported app looks
  the same:
  - `Axis`: bounds, a title, labels (`None`, `Auto`, `Values` or `Text`), a
    linear or log `AxisScale`, styles;
  - `Dataset`: a name, a `GraphType` (scatter, line or bar), a `Marker` and
    a style;
  - a legend box in any corner, hidden when it would cover too much of the
    plot.
- **`chart::Canvas`**, ratatui's `Canvas`: shapes in world coordinates
  (points, line, polyline, rect, circle, text) on Braille, half-block,
  block, dot or ASCII markers, in layers, clipped to the bounds.
- `LineChart` and the other charts are unchanged.

### Version bumps

`rs-rich-ext` 0.0.15 and `rs-rich-intuituive` 0.0.2 change packaged code, and
a 0.0.x requirement matches one version. So each crate that depends on them
moves too, with its requirement:

| Crate | Version |
|---|---|
| `rs-rich-interact` | 0.0.5 |
| `rs-rich-micro` | 0.0.4 |
| `rs-rich-record` | 0.0.5 |
| `rs-rich-data` | 0.0.2 |
| `rs-rich-ratatui` | 0.0.2 |
| `rs-rich-cli` | 0.0.17 |

No code in these crates changed.

## intuiTUIve 0.0.1 / ratatui 0.0.1 — published 2026-10-07

Two new crates, published from `main` at `d9a929f`.

### intuiTUIve: rs-rich-intuituive 0.0.1 (new crate)

`rs-rich-intuituive` (`intuituive`) is a framework for full-screen terminal
apps, built on `rs-rich-interact` (an app is a `Component`, so sessions, the
blocking and headless drivers and every existing component carry over),
`rs-rich-ext` and core `rs-rich`. Core is unchanged. See the guide page
[Terminal apps](docs/guide/intuituive/index.md) and the design note
[docs/design/intuituive.md](docs/design/intuituive.md).

- A per-app reactive runtime: `signal`s and `memo`s (reading one while
  drawing subscribes the node; writing one redraws exactly its readers), a
  `Proxy` that runs a closure on the app's thread from any other, and timers
  (`every`).
- Its own retained cell screen, which keeps hyperlinks, and a damaged diff:
  only the changed cells in the damaged rectangles reach the terminal.
- A retained tree of nodes: `text!`, `label`, `renderable` (any rich
  renderable: a `Table`, `Markdown`, `Syntax`, a chart), `column`, `row`,
  keyed `each`, `Node::panel`, `Log` for appended lines, and `component` for
  any rich-interact component.
- Input: focus with Tab, `on_key` bindings that bubble from the focused node
  up, and `on_click` routed by the layout the last frame kept.
- Headless testing through rich-interact's driver (`App::render_with`,
  `App::run_on`).
- Layout: `Size::Auto` (a child as big as its content, measured again when
  a signal the content reads changes), `min_size` and `max_size` clamps
  resolved the way CSS flexbox resolves flexible lengths, `gap`,
  `padding`, a `grid` with column and row tracks and children spanning
  several, and `switch` (one child shown by key, the others kept with their
  state: tabs, wizard steps). A container whose layout changes clears its
  area, so moved children leave nothing stale behind.
- Screens: `Ctx::push`, `Ctx::modal` (centred over the screen below, which
  keeps drawing), `Ctx::pop` and `Ctx::replace`. Each screen keeps its own
  focus and timers; keys and clicks reach the top screen only.
- Background work: `spawn` (a closure on its own thread), `spawn_future`
  (any runtime-independent future), and `resource` (a value that loads in
  the background, with `Load::Loading`, `Ready` and `Failed`, and drops
  stale results on reload). Results arrive on the app's thread with a
  `Ctx`; `Proxy::run_with` gives other threads the same. `App::wait_for_tasks`
  makes headless tests of them deterministic.
- Themes: `Theme::dark`, `light` and `mono` presets, named styles usable in
  markup (`[accent]…[/]`, `muted`, `good`, `warn`, `bad`, and the
  framework's `border`, `border.focused` and `title`), and `Ctx::set_theme`
  to switch at run time.
- Inline mode: `App::inline(rows)` runs in a region below the prompt, moving
  the cursor relative to it, and leaves the last frame in the scrollback.
- An inspector, docked on the right (`App::inspector(true)`, or
  `INTUITUIVE_INSPECT=1` for any app; F12 toggles it): the node tree with
  sizes, the nodes that drew in the last frame, the focus, the frame's cost
  (nodes drawn of those dirty, damage, bytes sent) and the theme file's
  state. `Node::name` labels nodes for it.
- Live styles: `App::theme_file(path)` reads rich's theme format and
  reloads it when the file changes, keeping the last good styles (and
  showing the error) when an edit does not parse; `Theme::with_config` and
  `Theme::load` read the same format.
- `repeating(make, on_done)`: a component built fresh after each answer,
  for entry boxes; `Node::focus_style(style)` highlights a leaf, such as a
  list row, while it has the focus.
- `list(items, selected)`: a scrolling list with a selected row (ratatui's
  `List` and `ListState` in one node), with ↑↓ jk, Home/End and
  PageUp/PageDown built in, and a `selected` theme style.
- `watch(source, on_change)`: run a callback, with a `Ctx`, whenever a value
  derived from signals changes; a screen's watches stop when it closes.
  `Memo::get_untracked` and `with_untracked`, and `Node::no_focus`.
- A hosted component (a ratatui widget through rs-rich-ratatui's
  `RatatuiComponent`) now draws again when the signals it reads change.
- `wait_for_tasks` also waits for tasks that watches start.
- `examples/files.rs`: a Yazi-style file manager (parent, current and
  preview columns, vim keys, background-highlighted previews, hidden files,
  sorting, filtering), with tests over a temporary tree; and a porting guide
  for ratatui apps (`docs/guide/intuituive/porting.md`), whose "keep your
  widgets" step is tested in `tests/ratatui_widgets.rs`.
- `examples/scope.rs`: a rebuild of scope-tui, an oscilloscope,
  vectorscope and spectroscope. It has scope-tui's keys and options, a
  debounced trigger, spectrum averaging with a Hann window, and Braille
  plots drawn on rs-rich-ext's `DotCanvas`. It reads raw PCM from a file
  or stdin, or plays a built-in test signal. Tests are in `tests/scope.rs`,
  recordings in `docs/tapes/scope.tape`, and the porting guide has a
  walkthrough.
- A panel whose focus state changes redraws only its border's edges, not
  its contents, and a focus move elsewhere leaves it alone.
- `intuituive::rich` and `intuituive::interact` re-export rs-rich and
  rs-rich-interact, so an app needs one dependency.
- A cargo-generate template (`templates/intuituive-app`, compiled and tested
  against the crate in CI) and a tutorial that builds a to-do app
  (`docs/guide/intuituive/tutorial.md`, its finished app
  `examples/todo.rs` with tests).
- A benchmark gate against ratatui 0.30 (`tests/versus_ratatui.rs`, a new
  `ratatui-gate` CI job, run in release): the same ops dashboard drawn both
  ways, failing if intuiTUIve is slower on a status tick or a selection move,
  more than 1.5 times slower on a log append, or sends more bytes. At 80x24
  in the run before this entry: a tick 14 µs against ratatui's 92 (18 bytes
  against 37), a selection move 43 against 112, a log append 46 against 92.
  ratatui is a dev-dependency only.
- The crate added to RELEASES.toml (after `rs-rich-ratatui`, so after
  `rs-rich-ext` and `rs-rich-interact`), the workspace dependency table, the
  release scripts and their tests, the CI feature matrix, the release
  workflow and readiness pattern, BRANCHING, the release skill, AGENTS.md,
  the docs nav and the version tables.

### ratatui interop: rs-rich-ratatui 0.0.1 (new crate)

`rs-rich-ratatui` (`rich_ratatui`) connects rs-rich and ratatui both ways,
productionised from the intuiTUIve design spike's interop prototype. It
depends on core `rs-rich` and `ratatui-core` 0.1 only (the crate ratatui
0.30's `Buffer`, `Rect`, `Style` and `Widget` live in), and nothing else in
the workspace depends on it or on ratatui. Core is unchanged.

- `RichWidget`: any rich renderable (a `Table`, `Panel`, `Markdown`,
  `Syntax`, markup) as a ratatui `Widget`, by reference or by value, with
  `RichWidget::markup`, `.console(&console)` and `.fill_height(true)`. Each
  segment sits at the column rich measured for it, and styles patch the
  cells beneath, as ratatui's own widgets do.
- `style`: `to_ratatui_style` / `to_rich_style` and `to_ratatui_color` /
  `to_rich_color`, lossless for the named, 256-colour and RGB colours, the
  terminal default and nine attributes (each on, off or unset); what is lost
  (colour names, `underline2`/`frame`/`encircle`/`overline`, hyperlinks and
  meta, ratatui's underline colour) is documented on each function.
- `buffer`: `lines_to_buffer`, `buffer_to_lines`, `buffer_area_to_lines`,
  and `cells`, a non-allocating walk over a rectangle of a buffer (column,
  row, symbol, width and a `CellStyle` key per leading cell) with a
  `StyleCache`, for a consumer converting into its own cell screen.
- `RatatuiComponent`, behind the off-by-default `interact` feature: a
  ratatui drawing closure, with optional state and event handler, as a
  `rich_interact::Component`, under every rich-interact driver.
- An example (`rich_in_ratatui`) drawing a Table and Markdown in a ratatui
  frame, the guide page [Using rich with ratatui](docs/guide/ratatui.md), and
  the crate added to RELEASES.toml (after `rs-rich-interact`), the release
  scripts and their tests, the CI feature matrix, the release workflow and
  readiness pattern, BRANCHING, the release skill, AGENTS.md and the version
  tables. Its MSRV is the workspace's 1.90 (ratatui-core needs 1.88).

## 0.0.16 cohort — published 2026-10-06

Data 0.0.1 (new crate) / diagram 0.0.2 / mermaid 0.0.5 / ext 0.0.14 /
micro 0.0.3 / record 0.0.4 / interact 0.0.4 / CLI 0.0.16 / Python 0.0.5.
Published from `main` at `c287f1f`; see
[the release notes](docs/releases/0.0.16.md#publication).

### 0.0.16 release test: fixes from three audits

Three independent audits covered the 0.0.16 delta: `rs-rich-data` with
`rich profile` and `--infer`; the schema model, SQL DDL, diffs, timeline,
DOT clusters, ER diagrams and `rich schema`; and the conflict and record
views, virtualised tables, style rules, the Cargo supply-chain reports,
`rs-rich-record`'s `write_selected` and `rs_rich.data`. They found 36
issues; each was reproduced with a test that failed (or hung, or panicked)
before its fix. See the [release notes](docs/releases/0.0.16.md#what-the-release-test-found-and-fixed).

- Panics on hostile input: an Arrow IPC footer with a negative dictionary
  length (`arrow::read_schema` now reads only the footer's schema), and a
  SQL `FOREIGN KEY` with no column list.
- Time and memory from small inputs, now linear or bounded: JSON Lines
  records with many keys, keys first seen after the sample (now at most 100
  named), long runs of mismatched conflict markers, very wide workspaces in
  `rich deps`, SQL DDL with many tables, tokens or key columns (a 16 MB file
  peaked at 2.3 GB; now 183 MB, with a `MAX_TOKENS` limit), `SchemaDiff`
  and `SchemaTimeline` over large schemas, nested DOT clusters, and ER key
  resolution. `rs_rich.data.Profile` refuses more than 1,000 bins, and
  `VirtualTable` shares its rows instead of copying them on every call.
- Terminal controls: text from `cargo audit`, `--timings`, licence and
  metadata files is sanitized when the supply-chain reports draw it, and
  `rich schema` now sanitizes by default (like `rich view` and the text
  `rich diff`; `--no-sanitize` turns it off) and honours `--sanitize`.
- Wrong results: SQL columns named `key`, `index` or `period` were dropped;
  PostgreSQL `$$` bodies were read as SQL; DDL without semicolons (`GO`
  scripts) lost every table after the first; `PRIMARY KEY CLUSTERED`
  failed; `rich schema /dev/stdin` lost its first six bytes to the Arrow
  check; NaN or infinite statistics from huge or infinite floats; streamed
  CSV stripped a mark from every record and could sniff BOM input
  differently from `read`; a CVSS vector repeating a metric was scored;
  timings summed to infinity; lower-case SPDX copyleft ids went
  unrecognised; integer style rules compared through `f64`; a
  consolidation dependent could be listed twice; and the timeline's
  "N more fields" count never showed.
- `rich profile` drew wider than its width between about 40 and 80
  columns; the summary table now gives way.
- `rs_rich.data`: a JSON Schema `SchemaError` has `line = None`, as its
  stub says.
- Review of the release test: `rich schema` sanitizes the file's name in
  every view and error (two-schema diffs and Arrow files showed it raw),
  keeps the whitespace the SQL reader splits words by when it sanitizes DDL,
  and keeps both of two JSON property names that read the same once
  sanitized (the second is numbered).
- The `micro-doctor` docs tape waits for the prompt before each command.

### Recorder: write what the tape asks for, and say what was skipped

From integration feedback on `rs-rich-record` 0.0.3 as a library:

- `rs-rich-record`: `record::write_selected` writes the files a tape's
  `Output` lines ask for (`Recording::formats` applied for you), so
  passing `Formats::ALL` honours `Output png` instead of writing every
  format. It returns `Written` (`formats`, `paths`, `skipped`, and
  `is_complete()`): an MP4 the selection calls for but FFmpeg is missing
  for is listed in `skipped` rather than silently absent, even when the
  recording is too long or too large for video. `record::write`
  is unchanged and now says that it takes formats as given.
- The crate docs' example used `Formats::ALL` with `write`, the mistake
  above; it now uses `write_selected`. The README gains a standalone
  `Cargo.toml` and `main.rs` for using the recorder without the CLI.
- `rich record` writes through `write_selected`; its output, including
  "ffmpeg not found: skipped …", is unchanged.

### Tabular foundation (0.0.16 workstream 1)

[Plan](docs/plans/0.0.16.md), #214, #215, #261, #267.

- `rs-rich-ext` `schema::model`: the format-neutral schema model, re-exported
  as `schema::{Schema, Field, DataType}`. A `Schema` is an ordered list of
  `Field`s, each with a name, a `DataType`, nullability, the source's own
  type name (`with_native_type`) and a description. `DataType` covers `any`,
  `unknown`, `boolean`, `integer`, `float`, `decimal(p,s)`, `string`,
  `binary`, `date`, `timestamp[tz]`, and the nesting `list<…>`, `struct` and
  `map<…, …>`, whose fields are a field's `children()`. `SchemaTree` and
  `SchemaDiff` are unchanged: they still read JSON Schema directly, and the
  mappings into the model arrive with workstream 4.
- `rs-rich-ext` `table::rules`: conditional styles (#215). A `StyleRule`
  names a column (a header or an index), a `Comparison` (`eq`, `ne`, `lt`,
  `le`, `gt`, `ge`, `contains`, `starts_with`, `ends_with`, `empty`,
  `not_empty`) against a value, or a Rust predicate (`StyleRule::when`),
  and a style for the cell, the row or the column (`Target`). Numeric rules
  read numbers from text cells, so CSV rows need no conversion. No
  expression language. `TableData::style_rules` applies them while
  rendering (without rules the output is unchanged), and
  `StyleRules::resolve` gives any other renderer each row's and cell's
  style. With the `toml` feature, `StyleRules::from_toml` and
  `from_toml_value` read `[[rules]]` tables (`column`, `op`, `value`,
  `style`, `target`), refusing unknown keys, ops and styles by index.
- New crate `rs-rich-data` 0.0.1 (`rich_data`), on `rs-rich-ext`'s public
  API: the `RowSource` contract (column names, an optional `Schema`, rows of
  `table::Value` one at a time) and `Rows` in memory, which become a
  `TableData` with numeric columns right-justified. Adapters (#214):
  - `csv`: the CLI's port of Python's `csv.Sniffer` and `csv.reader`
    (`sniff`, `has_header`, `read_rows`, `Dialect`) moved here unchanged
    with its tests, and `CsvReader`, which sniffs as `rich --csv` does (or
    takes a delimiter, dialect, fallback and header), applies universal
    newlines, drops blank lines and keeps every field of a ragged row.
    Cells stay text as written.
  - `jsonl`: JSON Lines, whole (`read`) or streaming from a reader
    (`source`), choosing columns and a schema from the first 1,000 records
    and naming keys seen later (`unknown_keys`) instead of dropping them
    silently; lines over 64 MiB are refused, schemas stop at 32 levels.
  - `serialize`: any `serde::Serialize` rows, through `serde_json`'s data
    model, whole or streaming.
  - `arrow` (off-by-default feature, `arrow-array`/`arrow-schema` 60):
    `RecordBatch` rows, a streaming `BatchSource`, and Arrow schemas mapped
    into the model with Arrow's type names kept as native types.
  - Type inference (#267), opt-in: `Inferrer` reads every cell and picks
    integer, float, boolean, date, timestamp, null or text per column, with
    `Evidence` (how many cells parsed as each type), configurable null
    tokens and per-column overrides; `Inference` renders as a table,
    `summary()` says it in words, and `apply` converts the cells and
    attaches the schema.
  - Column statistics (#261): `Stats` gives count, nulls, distinct, min,
    max, mean, median, quantiles and the top values per column, rendered as
    a table, or as a summary under each heading (`Stats::headers`).
- `rs-rich-cli`: `--csv` and `rich chart` read CSV through `rs-rich-data`.
  `rich --csv` output is byte-identical: the same reader, and 84 runs over
  twelve inputs (sniffed, ragged, quoted, BOM, headerless, no delimiter;
  `--head`, `--tail`, titles, a terminal, stdin and auto-detection) compare
  equal before and after the move. `--csv` never infers types.
- Release tooling: `rs-rich-data` is in `RELEASES.toml` (after
  `rs-rich-interact`, before `rs-rich-cli`), `scripts/release.py` and its
  tests, the release workflow's tag filter, the release-readiness pattern,
  the CI test matrix (default and all features), the Python workflow's
  paths, `docs/BRANCHING.md`, the release skill, `AGENTS.md`'s dependency
  graph and versioning table, and the version tables.

### Schemas (0.0.16 workstream 4)

[Plan](docs/plans/0.0.16.md), #268, #342, #347.

- `rs-rich-ext` `schema::model` grows from workstream 1's minimal model:
  a `Field` is also `required` (a value must be present) and has
  `Constraint`s (`Const`, `Enum`, `Format`, `Pattern`, length, bounds and
  item limits, `Default`, `Deprecated`/`ReadOnly`/`WriteOnly`,
  `Additional`, `PrimaryKey`, `Unique`, `References(ForeignKey)` and
  `Other`), values kept as a `Literal` as the source writes them, metadata
  entries, a `FieldKind` (JSON Schema's pattern properties, other
  properties, tuple and array items, composition groups and branches, and
  conditions) and the reference its type came through (`reference`,
  `Unexpanded` for a recursive or unresolved one). A `Schema` can hold
  several named `tables` (a SQL file), table-level constraints (keys over
  several columns), a description, metadata and a `truncated` flag;
  `primary_key()` and `foreign_keys()` read the keys back. Workstream 1's
  constructors and accessors are unchanged.
- `schema::json`: JSON Schema → model (`to_field`, `to_model`).
  `SchemaTree` and `SchemaDiff` are rebuilt on the model: the tree draws a
  model `Field`, and the diff is a comparison engine over the model that
  JSON Schema drives by following its `$ref`s lazily. Their JSON Schema
  output is byte-identical: every existing test and snapshot passes
  unchanged, and a differential check against the previous implementation
  over 24,000 generated schemas (trees at two depths, diffs, coloured)
  found no difference.
- `schema::sql`: a small `CREATE TABLE` subset → model (`parse` →
  `Parsed { schema, notes }`): column types as written, `NOT NULL`,
  `DEFAULT`, `PRIMARY KEY`, `UNIQUE` and `REFERENCES`/`FOREIGN KEY` on
  columns or tables, `CONSTRAINT` names, quoted identifiers, comments, MySQL
  `ENUM` and `COMMENT`. `CHECK`, indexes and other statements are skipped
  with a `Note` naming the line; unreadable DDL is a `SqlError` with its
  line. Bounded by `MAX_INPUT` (16 MiB), `MAX_STATEMENTS`, `MAX_COLUMNS`,
  `MAX_NESTING` and `MAX_NOTES`.
- Schema diff for any two schemas (#268): `SchemaDiff::models(&old, &new)`
  compares model schemas (fields by path, tables by name, nullability, keys and
  metadata), with breaking changes marked, so DDL and Arrow versions diff
  like JSON Schema. It is the same `SchemaDiff` type and rendering;
  `SchemaDiff::summary()` is now public.
- `SchemaTree::from_model` draws any model schema (tables with their
  columns). `SchemaTree::schema()` now returns `Option<&Value>` (`None` for
  a tree made from a model), beside a new `model()` and `root()`.
- Schema evolution timeline (#347): `schema::SchemaTimeline` puts a series
  of versions (`push`, `push_json`, placed with `at`) on `chart::Timeline`,
  a row per field spanning the versions it is in and styled where it was
  added, changed or broken, a milestone per version with its counts, and
  every change listed under it.
- `rs-rich-data` `arrow` (the Arrow schema explorer, #342): the mapping
  marks non-nullable fields required, keeps field and schema metadata
  (sorted by key) and names nested types by kind (`List`, `Struct`, `Map`,
  `FixedSizeList(n)`) since their children are fields; `arrow::tree` draws
  an Arrow schema and `arrow::diff` compares two.
- `rs-rich-data` `er` (behind the new, off-by-default `er` feature, which
  adds `rs-rich-diagram`): ER diagrams of a schema (#247). `er::model`
  turns a model schema into `rich_diagram::ErModel` (a box per table with
  its keys, an edge per foreign key, `N:1` or `1:1`); `er::from_sql` reads
  DDL straight to an `ErDiagram`.

### Large and SQL-shaped tables (0.0.16 workstream 2)

[Plan](docs/plans/0.0.16.md), #235, #260.

- `rs-rich-ext` `table::virtualized`: `VirtualTable` renders one window
  (an offset and a height) of a `VirtualRows` source and fetches nothing
  else, so a ten-million-row source renders in constant memory. A source
  fetches rows by index (`row`), optionally knows its `row_count`, and may
  fetch a range at once (`rows`); `Vec`s and slices of rows, references,
  `Box`, `Arc` and closures (`FnRows`) implement it. Column widths are
  fixed by the caller (`widths`, or a column's `ColumnOptions::width`) or
  sampled once from the header and the first 100 rows (`sample`, at most
  10,000), capped at 40 cells (`max_column_width`), so scrolling never
  moves a column; cells end in `…` rather than wrap unless `wrap(true)`,
  and `fit_window(true)` widens sampled columns to fit a window rendered
  once. A position line reads `rows 1,001–1,040 of 1,000,000`, or `of
  1,041+` when the source cannot count. For a viewport: `page()`,
  `set_offset`, `scroll_by` (stopping at both ends), `set_height` (at most
  10,000 rows), `max_offset` and `chrome_lines`. Also `row_numbers`, a
  `null_marker` and a `footnote`. Ragged rows are padded with nulls or cut.
  New theme keys `table.position`, `table.null` and `table.row_number`.
  Interactive scrolling stays with `rs-rich-interact`'s viewport.
- `rs-rich-data` `window`: `Rows` implements `VirtualRows`, and `RowWindow`
  reads one pass of any `RowSource` (CSV, JSON Lines, serde, Arrow),
  keeping only the window, the first rows for the width sample and the
  count; an offset past the end gives the last full window, and
  `count(false)` stops one row past the window.
- `rs-rich-data` `sql`: `ResultSet` shows a query result the way a
  database shell does, through `VirtualTable`: typed alignment from the
  schema (integers, floats and decimals right, booleans centred, the rest
  left; a column with no type by its sampled cells), `NULL` in
  `table.null` apart from empty text, which stays empty, and a footer —
  `(3 rows)`, `(1 row)`, `(1,001+ rows)`, with the elapsed time when given
  (`(3 rows, 12ms)`). `limit` (default 1,000) and `offset` window a large
  result, which then shows its position above the count. No database
  connection: rows come from `Rows`, a `RowWindow`, or any `VirtualRows`
  (`from_parts`). `typed_columns` and `alignment` are public for other
  renderers.

### Profiling and data quality (0.0.16 workstream 3)

[Plan](docs/plans/0.0.16.md), #343, #344, #345, #346, #269.

- `rs-rich-data` `profile`: a `Profile` of any row source. Each column has
  its inferred type (through `infer`), null count and rate, distinct values,
  min, max, mean, median and quartiles (through `stats`), and a
  `Distribution`: histogram bins from `rich_ext::chart::Histogram` for a
  numeric column (#345), the most common values and the rest for any other.
  A `MissingMap` (#346) buckets the rows in order and counts each column's
  nulls per bucket, drawn as a `rich_ext::chart::Heatmap` of the percentage
  null. The model is `serde` (`to_json` adds `sampled`) and renders as a
  heading, a column table, each distribution and the map, with
  `profile.*` theme keys (`profile::STYLES`).
- Profiles are bounded: `Profiler` reads one row at a time and keeps a
  uniform reservoir sample (Algorithm R with a fixed seed, so the same input
  gives the same profile) of `ProfileOptions::sample` rows (10,000 by
  default); types, distinct values, statistics and distributions describe
  the sample, and the row count, null counts and the map (at most 20
  buckets, neighbours merged as rows arrive) count every row. The heading
  prints the sample size (`sampled 10,000 of 3,000,000 rows`). Null tokens
  (`infer::DEFAULT_NULL_TOKENS`: empty, `null`, `NULL`, `NA`, `N/A`) are
  null in every column. A 3,000,000-row, 76 MB CSV profiles in 35 MiB.
- `csv::CsvReader::source` streams CSV/TSV from any `BufRead` (#343):
  `CsvSource` sniffs the dialect and header from the first 1,024
  characters as `read` does, parses a record at a time (quoted fields may
  span lines, up to `csv::MAX_RECORD`, 64 MiB), pads short rows and counts
  rows cut to the columns (`longer_rows`). Its rows equal `read`'s for the
  same text. `csv::Header::UnlessNumeric` is `rich chart`'s header rule
  (a header unless the first row is all numbers), for input where the
  sniffer's guess fails on one empty cell. JSON Lines streams through the
  existing `jsonl::source` (#344).
- `rs-rich-data` `quality` (#269): a data-quality result model,
  `CheckResult` (check, column, `Status` pass/warn/fail/error, observed,
  expected, message, `FailingRows`), and `QualityReport`, rendered like
  `rich_ext`'s `TestReport`: failures, errors and warnings first with their
  failing rows as a table, then a table of every check and a summary line,
  with data shown with its controls made visible and `quality.*` theme
  keys. `quality::not_null` and `quality::unique` check `Rows`. No check
  engine.
- `rs-rich-cli`: `rich profile [FILE]` profiles CSV/TSV or JSON Lines (by
  extension, else by the first character) from a file, a URL or stdin.
  Files and stdin are streamed, so a long input is sampled rather than read
  into memory. `--sample N`, `--columns a,b` and `--top N` choose what is
  kept and shown; `--report json` writes the profile document to stdout, as
  `doctor --report json` does, and failures keep the stderr envelope. An
  unknown column, an unreadable line or empty input is a data error
  (exit 4); a row with more cells than the header is noted, not refused.
  In `--help`, completions, `docs/cli.md`, `docs/cli-reference.md` and
  `docs/PORTING.md`.

### The CLI, Python and docs (0.0.16 workstream 7)

[Plan](docs/plans/0.0.16.md), #247, #342.

- `rs-rich-cli`: `rich schema` reads SQL DDL and Arrow as well as JSON
  Schema. DDL is known by its `.sql` or `.ddl` extension, or by text (on
  stdin, say) starting with `CREATE` or a SQL comment, and is read through
  `rich_ext::schema::sql`: the tree is titled with the file, and what the
  reader skipped (`CHECK` constraints, indexes, other statements) is listed
  dimmed under it with its file and line. DDL that cannot be read is a data
  error (exit 4) naming the line. Arrow IPC files and streams (`.arrow`,
  `.feather`, `.arrows`, `.ipc`, or an Arrow file's `ARROW1` magic) are
  read behind the CLI's new off-by-default `arrow` feature; without it an
  Arrow input is a usage error (exit 2) naming the feature. JSON Schema
  output is byte-identical.
- `rich schema OLD NEW` compares any two of the formats through
  `SchemaDiff::models` (two JSON Schemas still follow their `$ref`s through
  `SchemaDiff::new`, unchanged). A JSON Schema or Arrow file against DDL of
  a single table compares with that table.
- `rich schema --er FILE` draws the schema as an ER diagram through
  `rs-rich-data`'s `er` feature, now on in the CLI: a box per table with
  `PK`, `FK` and `UQ` markers and an edge per foreign key with its
  cardinality; a schema without tables is one box. `--er` takes one schema
  and only applies to `rich schema`.
- `--infer` on `--csv` tables (and `rich FILE.csv`/`.tsv`): each column's
  type from `rich_data::infer` under its heading (on a second, dim line),
  number columns right-aligned even with null tokens such as `NA`. Opt-in:
  without it the table is rich-cli 1.8.1's, byte for byte, and with another
  mode it is a usage error.
- `rs-rich-data`: `arrow::read_schema` reads the schema of an Arrow IPC
  file or stream (from any `Read + Seek`) without reading its batches,
  through `arrow-ipc` 60 (no compression codecs) behind the existing
  `arrow` feature; anything else is a `DataError`, not a panic.
- Tapes for `rich profile`, `rich schema --er`, `rich deps --features` and
  `rich diff --conflicts`, with their fixtures under
  `docs/tapes/fixtures/data`, recorded into `docs/media/tapes` and added to
  the recordings page and the gallery.
- In `--help`, completions, `docs/cli.md`, the regenerated
  `docs/cli-reference.md`, `docs/PORTING.md` and the CLI README. No core
  change.

- Python (`rs-rich` 0.0.5 on PyPI, already bumped): `rs_rich.data` binds
  `rs-rich-data` (with its `er` feature) and the 0.0.16 views.
  `read_csv`, `read_tsv`, `read_jsonl` and `read_file` (format by
  extension, then by the first character) read into `Rows`, which renders
  as a table and becomes an `rs_rich.ext.table.TableData`; `infer` returns
  an `Inference` (each column's type, evidence and summary; `apply`
  converts the cells and sets the schema); `Stats` the column statistics;
  `Profile` profiles rows, text (`from_text`) or a streamed file
  (`from_path`), with `to_json`/`to_dict`; `CheckResult`, `QualityReport`
  (`from_json` reads another tool's results) and `check_not_null` /
  `check_unique`; `ResultSet` and `VirtualTable` (`offset`, `height`,
  `scroll_by`, `fit_window`). The schema views: `Schema.from_json_schema`
  and `Schema.from_sql` (the model, with `SchemaField`s, tables and the
  reader's notes), `SchemaTree`, `SchemaDiff` (`SchemaChange`s, breaking
  count), `SchemaTimeline` and `ErDiagram` (also `from_sql`), each taking
  a `Schema` or a JSON Schema as a `dict` or JSON text. `ConflictView`
  (`MergeConflict`s) and `RecordView` are also in `rs_rich.ext.diff` and
  `rs_rich.ext.data`. Errors carry `line`: `DataSourceError` (`DataError`
  in `rs_rich.data`), `SchemaError`, `ConflictError`. Type stubs, tests
  against the crates' doc examples, a
  [Data, schemas and developer views](docs/python/data.md) page whose
  examples the tests run, and its API reference page. No Rust library
  change: the bindings use the crates' public API.
- Docs: a [tabular data guide](docs/guide/data/index.md) (adapters,
  inference, statistics, conditional styles, profiles, quality reports,
  virtualised tables and result sets), a [schema guide](docs/guide/data/schemas.md)
  (the model, JSON Schema, SQL DDL and Arrow readers, tree, diff, timeline
  and ER diagrams) and a [developer-views](docs/guide/data/developer-views.md)
  landing page (merge conflicts, the record inspector, Cargo supply-chain
  reports, schema diffs), linking to the detailed sections in the tables,
  sources, diffs and structured-data pages rather than repeating them; in
  the site's nav and the guide index.

### Records and conflicts (0.0.16 workstream 5)

- `rs-rich-ext` 0.0.14: `data::RecordView` (#270), the record inspector:
  one record as a `field | type | value` table, nested maps and sequences as
  `Explorer` trees opened `depth(n)` levels (default 1), `expand(path)` and
  `collapse(path)` for one branch, `branch(path)` for a branch's tree on its
  own, strings cut with their length (`max_string`, default 200) and
  containers with `… N more` (`max_items`, default 20), and `redact` before
  display. `data::OpenBranch` is the drill-down hook: open or fold a branch
  by path.
- `rs-rich-ext` 0.0.14: `diff::ConflictFile` and `diff::ConflictView`
  (#339): a parser for merge-conflict markers, diff3's `|||||||` base and
  labels included, CRLF-safe, that reads markers as git does (seven characters,
  or a longer `conflict-marker-size` when the whole conflict uses it, so a
  nested `<<<<<<<<` is text) and returns a `ConflictError`
  naming the line for markers out of order, nested or never closed, with
  `MAX_CONFLICT_SOURCE` (16 MiB) and `MAX_CONFLICTS` (10,000) limits. The
  view numbers each conflict, keeps context lines, and shows ours, base and
  theirs side by side when each column fits (`ConflictLayout::Auto`) or
  stacked, highlighted through the `SourceDiff` path (plain without the
  `syntax` feature). New theme keys `diff.conflict.ours`, `.base`, `.theirs`
  and `.label` in `diff::STYLES`.
- `rs-rich-interact` (next patch): `DataExplorer` implements
  `rich_ext::data::OpenBranch`, opening a branch and its ancestors by path.
- `rs-rich-cli` 0.0.16: `rich diff --conflicts FILE` (or `-` for stdin),
  with `--side-by-side`, `--context N` and `--language NAME`, and a summary
  line. It exits 0 with or without conflicts, as text diffs do; markers that
  do not parse exit 4 naming the line, more or fewer than one file and
  `--threshold` are usage errors (2).
- Docs: [Merge conflicts](docs/guide/ext/diffs-and-test-reports.md#merge-conflicts),
  [The record inspector](docs/guide/ext/structured-data.md#the-record-inspector),
  the explorer guide, `docs/cli.md`, the CLI reference and `docs/PORTING.md`.
  Nothing under `crates/rich/src` changes.

### Cargo and the supply chain (0.0.16 workstream 6)

`rich deps` gains five reports beside the tree, each reading what Cargo or
its tools already wrote; none runs a scanner or reaches the network
(#420, #419, #421, #330, #418, #331). See
[Supply-chain reports](docs/guide/ext/sources.md#supply-chain-reports).

- `rs-rich-ext` `deps::duplicates`: `DepGraph::consolidation` and the
  `Consolidation` renderable: each crate resolved at several versions, who
  pulls each version, and the version most dependents use (the newest by SemVer on
  a tie); `DepTree::consolidation(true)` adds it under a tree.
- `deps::features`: `FeatureGraph` reads `cargo metadata`'s resolved
  features, `[features]` tables and declared dependencies (renames matched
  through the resolve graph); `FeatureTree` draws each enabled feature's
  definition (`dep:`, `name/feature`, weak `name?/feature`, entries Cargo
  did not act on marked `(off)`) and which dependents asked for what.
- `deps::timings`: `Timings::parse` reads the `cargo build --timings` HTML
  report's `UNIT_DATA`, the array alone, or `timing-info` JSON lines;
  frontend (rmeta) and codegen times come from Cargo's `sections`, else
  `rmeta_time`. `TimingsReport` draws the slowest units as bars and a table.
- `deps::audit`: a generic `Advisory` model (id, kind, package and version,
  patched and unaffected ranges, severity, CVSS score, title, link, aliases,
  date), `AdvisoryReport::from_cargo_audit` for `cargo audit --json` (the
  warning table and the older list), CVSS 3.x base scores
  (`audit::cvss3_score`), and a report grouped by severity with counts.
- `deps::licenses`: an SPDX expression reader (`AND`, `OR`, `WITH`,
  parentheses, the old `/`) that classifies an expression as permissive,
  weak copyleft, copyleft, unrecognised or invalid, and `LicenseReport`,
  grouping packages by canonical expression with copyleft, unknown, file-only
  and missing licences marked.
- Every reader is bounded (`deps::MAX_INPUT`, 64 MiB; `deps::MAX_RECORDS`,
  100,000 records; serde_json's 128-level nesting limit; licence expressions
  at most 1,024 bytes and 32 parentheses deep) and returns an error, never
  panics, on malformed input. New theme keys: `deps.feature`, `deps.off`,
  `deps.copyleft`, `deps.unknown`, `deps.critical`, `deps.high`,
  `deps.medium`, `deps.low`, `deps.info`.
- `rs-rich-cli`: `rich deps --duplicates` adds the consolidation summary
  after the existing output (without `--duplicates` the output is
  unchanged); `--features [--package CRATE]`, `--licenses` (honours
  `--no-dev`), `--timings FILE` and `--audit FILE` (`-` for stdin; neither
  runs Cargo). `--audit` exits 5, the gate class, when the report lists a
  vulnerability, after showing it, with one `"code": "gate"` envelope under
  `--report json`; warnings alone exit 0. Only one report at a time, and an
  option that does not apply to it is refused with exit 2.

### Diagrams: cluster frames and ER (0.0.16 workstream 4)

`rs-rich-diagram` 0.0.2 frames clusters, honours `rank=same`, and draws
entity-relationship diagrams (#240, #247). See
[Clusters and same-rank groups](docs/guide/diagram/index.md#clusters-and-same-rank-groups)
and [ER diagrams](docs/guide/diagram/index.md#er-diagrams).

- **Cluster frames.** The graph model gains `Cluster` (an id, a label, its
  nodes and an optional parent), added with `Graph::cluster` or
  `Graph::add_cluster`. The layered layout draws each as a dashed frame
  (`╌`/`╎`; `-`/`:` in ASCII) around its nodes, the label in the top border
  (the bottom one when edges cross the top everywhere), nested clusters as
  nested frames. Ordering keeps a cluster's points together in every rank,
  with sibling clusters in one left-to-right order throughout; ranks a
  cluster spans without a node of its own get an invisible placeholder; and
  after placement points move right until each frame clears its
  neighbours. Edges cross frames whole. A node listed in clusters that do
  not nest is framed in the deeper one, with a note; the separation is
  bounded, and a graph it cannot settle is drawn without frames, with a
  note.
- **`rank=same`.** `Graph::same_rank` / `add_same_rank` groups nodes into one
  rank; the layout ranks the group as one node. A group an edge runs within
  cannot be drawn in one rank and is dropped, with a note.
- `Drawing::notes` (new field) carries what the layout could not do. The
  `Dot` renderable prints it under the drawing.
- **DOT.** `subgraph cluster_…` subgraphs are now framed (their old "drawn
  without their frames" note is gone), a cluster opened inside another
  records it as its parent, and `{ rank=same; … }` in a subgraph is
  honoured. Other `rank` values, and `rank` on the whole graph, are noted as
  not applied. `dot::Cluster` is now `rich_diagram::Cluster` (it gains
  `parent`).
- **ER diagrams.** New `rich_diagram::er`: an `ErModel` of `Entity`s and
  `Column`s (type, primary key, foreign key, unique, nullable),
  `Relationship`s from one entity's columns to another's with an optional
  `Cardinality`, and `Group`s, all plain public structs with chaining
  constructors so a schema or SQL DDL reader outside this crate can fill
  it. `ErDiagram` draws it (left to right by default): each entity a box
  with its name ruled off from aligned `name  type  keys` rows (`PK`, `FK`,
  `UQ`, `?` for nullable), each relationship an edge labelled with its
  columns and cardinality (a self-reference is marked `↻`, its label in a
  note), each group a cluster frame, and a dim `ER:` note for anything it
  cannot draw. New `Shape::Table` draws those boxes.
- **Byte-identical without them.** A graph with no clusters and no
  same-rank groups lays out exactly as before: every existing snapshot,
  Mermaid's included, is unchanged, and a corpus of 600 random graphs in
  every direction was compared before and after. The DOT `services`
  fixture's snapshot changes because its cluster is now framed.
- Mermaid's subgraphs are not passed to the layout as clusters yet, so
  they are still drawn without frames; `rs-rich-mermaid` moves to 0.0.5
  only because it requires `rs-rich-diagram` 0.0.2. Nothing under
  `crates/rich/src` changes.

## 0.0.15 cohort — published 2026-10-05

Diagram 0.0.1 (new crate) / mermaid 0.0.4 / ext 0.0.13 / micro 0.0.2 /
record 0.0.3 / interact 0.0.3 / CLI 0.0.15 / Python 0.0.4. Unchanged and
already published: core 0.0.9, macros 0.0.3, plugin API 0.0.3, art 0.0.12,
lumis 0.0.3. Published from `main` at `61d031a` by the protected release
workflows; see [the release notes](docs/releases/0.0.15.md#publication).

### 0.0.15 release test: fixes from three audits

Three audits covered the 0.0.15 delta; each finding was reproduced, then
fixed with a regression test that failed (or aborted, or hung) before the
fix. See the [0.0.15 release notes](docs/releases/0.0.15.md#what-the-release-test-found-and-fixed).

- `rs-rich-diagram`: DOT nesting deeper than 64 levels is refused instead
  of overflowing the stack; sources, nodes and edges are capped while
  groups expand (64 KiB, 500, 2,000, shared with Mermaid as
  `rich_diagram::{MAX_SOURCE, MAX_NODES, MAX_EDGES}`); strict-graph dedupe
  is a hash lookup; `layout::draw` refuses more than 2,000 edges or 5,000
  nodes before laying out. DOT handles `\E`, `\T` and `\H`, lexes numerals
  as Graphviz does, skips a comment before `=`, notes invisible and
  outline-free nodes, and writes ASCII-only notes on ASCII consoles.
- `rs-rich-ext` `deps`: a workspace member reached through another member
  keeps its dev-dependencies; members are expanded at the top level; trees
  stop at 128 levels (`MAX_TREE_DEPTH`). `schema`: trees and diffs have
  budgets (`MAX_ENTRIES`, `MAX_CHANGES`, `MAX_COMPARISONS`,
  `SchemaDiff::is_truncated`) and say when they stop; cycles are found by
  resolved target as well as by reference; the `additionalProperties`
  path keeps the depth guard.
- `rs-rich-ext` `chart`: labels the compact form cannot write exactly are
  written in full on axes, timelines and histogram edges; histogram edges
  are exact and values are counted against them; `ValueFormat::Fixed` caps
  its decimals at `ValueFormat::MAX_DECIMALS` (17) instead of panicking;
  ±1e308 ranges render with finite scales; huge sizes saturate or are
  capped (65,535 rows); gauge legends are ASCII on ASCII consoles; a
  non-finite KPI delta reads `= -`.
- `rs-rich-cli`: `rich deps --why` exits 4 with a reason when the chosen
  kinds reach no match; `rich deps` clears the `rustc` wrapper variables for
  `cargo metadata`; `rich chart` refuses non-finite values and rows longer
  than the header (exit 4); `rich doctor` reports every build feature;
  `--help` lists `.mmd`, `.mermaid`, `.dot` and `.gv` among the
  auto-detected types.
- `rs-rich-micro`: `:micro:` tokens are left alone wherever the Markdown
  parser sees code, and in link and image destinations; the ranges come
  from `pulldown-cmark`, now a direct dependency.
- `rs-rich-interact`: a `Confirm` choice answers to either case again; a
  rebound or installed key takes precedence over an earlier action's
  declared key; `FilePicker::rebind` keeps its own actions to the picker.
- Python 0.0.4: `rs_rich.chart` takes any iterable where it took a list,
  any pair as a point, and `None` in a timeline as a gap, and refuses
  out-of-range decimals with `ValueError`.

### Docs refresh for 0.0.15

- The README, the home page, the guide index, the CLI guide and walkthrough,
  [Using the CLI](docs/cli.md), Getting started, Architecture, Known issues
  and the crate READMEs describe the 0.0.15 cohort: charts, `rs-rich-diagram`
  and DOT, `rich chart`, `rich dot`, `rich deps` and `rich schema`,
  `rs_rich.chart` and `rs_rich.diagram`, with cross-links between the Rust,
  CLI and Python pages. Stale statements fixed: release history stopping at
  0.0.11, `rs-rich` 0.0.7 and `rs-rich-ext` 0.0.11 in install snippets, the
  CLI README's version, macOS "not covered by CI", "`rich` writes no cache"
  (micro assets have one), `rs-rich-macros` "not on crates.io", the ext
  registry described as internal, the missing crates in the architecture and
  guide tables, `rich doctor --json` (it is `--report json`), and the Python
  command line's claim to every command (`rich record` is not in the wheel).
  `AGENTS.md`'s dependency graph now shows `rich-cli → rich-micro` and
  `rich-art`, and what `rich-py` depends on.

### 0.0.14 follow-ups (0.0.15 workstream 6)

The [0.0.14 known limits](docs/releases/0.0.14.md#known-limits) that were
small. Plugin components over the dylib and WASM ABI stay out: they need an
ABI change of their own.

- `rs-rich-micro` 0.0.2 (unreleased, so no bump): `PreparedMarkdown` and
  `MarkdownView` put `:micro:name:` in Markdown. Each token outside code
  and URLs becomes stand-in cells of the asset's width, so the layout sizes
  it as the asset; the view turns them back into tagged placeholders after
  rendering, which `MicroView` and `MicroGraphics` draw. `\:micro:` stays
  literal, as Markdown drops the backslash itself. What is code and what is
  a URL comes from pulldown-cmark, with the options core's `Markdown` uses
  (a new direct dependency, the version already in the tree): indented
  blocks, fences in quotes and list items, code spans across a line break,
  link and image destinations, autolinks and reference definitions keep
  their tokens as written; link text expands.
- `rs-rich-cli` 0.0.15: `:micro:name:` expands outside `--print` too: in a
  `--panel`'s `--title` and `--caption` (always, as their `:emoji:` codes
  do), in a CSV table's title and caption (with `--emoji`, where its
  `:emoji:` codes expand), and in `--markdown` documents. On a terminal they
  draw as images; exports, the pager and `--watch` (one file or several)
  keep the fallback.
- `rs-rich-cli` 0.0.15: `rich asset --kind micro` and `rich explore --icons`
  draw the assets as images (Kitty, iTerm2, Sixel or blocks) where the
  terminal can, not only their emoji fallback. The mode is chosen for
  standard error, where the pickers paint, so `name=$(rich asset --kind
  micro)` draws too, and the Kitty images are deleted when the command ends.
- `rs-rich-interact` 0.0.3 (unreleased, so no bump): `run_with_graphics`
  runs a component with a graphics source (`MicroGraphics::source`), as
  `EventLoop::graphics` does for a loop.
- `rs-rich-interact` 0.0.3: `Confirm`, `Pager`, `TextArea`, `ColorPicker`,
  `Form` and `FilePicker` match keys through their keymaps, so their own
  `rebind` (new on each) and the process-wide overrides (`keymap::install`,
  the CLI's configuration) change what a key does. `pager_keymap`,
  `pager_search_keymap` and `Viewport::act` are new. Rebinding a form's
  `next` away from Enter also takes Enter's submit on the last field;
  Ctrl+S still submits.
- `rs-rich-interact` 0.0.3: a key rebound (or installed) onto an action now
  does it even when an action declared earlier in the same context had the
  key, and that action loses it (`Pager::rebind("scroll-down", keys("n"))`
  used to leave `n` on next-match and scroll-down with no key).
  `FilePicker::rebind` sends its own actions (`open`, `up`, `hidden`) to the
  picker only, so rebinding `up` (to the parent directory) no longer takes
  the list's cursor-up with it; `select.up` names the list's. A `Confirm`
  choice with an uppercase key answers to the lowercase press again, as in
  0.0.14.
- `rs-rich-cli` 0.0.15: `rich micro preview` no longer labels a still image
  with a frame time ("100 ms"); only an animation's frames are timed. The
  `micro-create` docs tape is re-recorded.

### The CLI, Python and docs (0.0.15 workstream 5)

- Charts and diagrams no longer print a blank line after themselves. Every
  `rich_ext::chart` renderable, `rich_diagram::{Diagram, Dot}` and
  `rich_mermaid::Mermaid` ended its last line with its own newline, so
  `print` (which adds one, as core's renderables expect) left an empty line
  after each, in Rust, in Python and from `rich chart`, `rich dot` and
  `rich mermaid`. They now leave that newline to `print`, and an empty chart
  prints nothing. Tests compare what `print` writes and fail on a doubled
  newline; the screenshots, the `chart`, `deps` and `tour` tapes and the
  Python pages are regenerated.
- `rs-rich-cli` 0.0.15: `rich chart [FILE]` draws data from a file, a URL
  or stdin with `rich_ext::chart`: `--kind spark|bar|line|scatter|heatmap`
  (line by default), `--x COLUMN` for positions or labels and a repeatable
  `--y COLUMN` per series, by header or 1-based number, and `--width`. It
  reads CSV and TSV (sniffed as `--csv` sniffs), JSON (records, numbers,
  rows, or an object of columns), JSON Lines and whitespace-separated
  numbers; an empty cell or `null` is a gap. A missing column lists the
  columns there are, and a value that is not a number names its row and
  column; both exit 4. The parsing stays in the CLI (it reuses the CLI's
  port of Python's `csv` sniffer, and picking columns is command routing);
  `rs-rich-ext` and core are unchanged.
- Python (`rs-rich` 0.0.4 on PyPI, already bumped): `rs_rich.chart` binds
  every chart renderable (`Sparkline`, `BarChart`/`ChartBar`, `Histogram`,
  `LineChart`/`Series`, `Gauge`/`Band`, `BulletChart`, `Heatmap`,
  `StatusMatrix`/`ChartState`, `KpiCard`, `Timeline`/`TimelineSpan`), built
  from keyword arguments, with `Bar`, `State` and `Span` as the Rust names.
  `rs_rich.diagram` binds `rs-rich-diagram`: a chainable `Graph`,
  `Diagram`, `draw_graph`, `Dot` and `parse_dot` (`DotError` carries `line`
  and `construct`). Type stubs, tests against the crates' doc examples, and
  [Charts](docs/python/chart.md) and [Diagrams](docs/python/diagram.md)
  pages.
- Docs: tapes for `rich chart`, `rich deps` and a ```` ```dot ```` fence in
  Markdown, with cards and sections in [Terminal recordings](docs/recordings.md);
  a `rich chart` section in the [charts guide](docs/guide/ext/charts.md);
  Diagram, ASCII and DOT screenshots in the [diagrams guide](docs/guide/diagram/index.md)
  from `cargo run -p rs-rich-diagram --example services -- --svg docs/media/guide`;
  Charts and Diagrams in the [gallery](docs/gallery.md); the CLI reference
  and `docs/PORTING.md`.

### Diagrams from real sources (0.0.15 workstream 4)

- `rs-rich-diagram` 0.0.1 (unreleased, so no bump): a DOT (Graphviz) parser
  (#240), `rich_diagram::dot::parse` → `DotGraph`, for the subset people
  write by hand: `graph`/`digraph`, `strict` (repeated edges merge), node and
  edge statements with chains (`a -> b -> c`) and `{ … }` groups,
  `label`/`shape`/`style` (and `dir`, `arrowhead`, `arrowtail`, `minlen`,
  `rankdir`), `node`/`edge`/`graph` defaults scoped to their subgraph,
  subgraphs and `cluster…` clusters, quoted IDs with `+`, and `//`, `/* */`
  and `#` comments. Node ports, HTML-like labels, `record` shapes, bodiless
  subgraph references and several graphs in one file are refused with a
  `DotError` naming the construct and its line, never drawn partially.
  Painting-only attributes are ignored; cluster frames and `rank`
  constraints are accepted and noted under the drawing. A `Dot` renderable
  draws a source (or the error and the source). New `plugin` feature
  (`rs-rich-plugin-api`, optional): `plugin::DotPlugin` registers `dot` and
  `graphviz` fence renderers and a `dot` source renderer. New `graphviz`
  feature: `graphviz::render_svg` runs Graphviz's own `dot -Tsvg` with the
  source on stdin, a timeout and size caps.
- `rs-rich-ext` 0.0.13: `rich_ext::deps` (#248), behind `data`: `DepGraph`
  from `cargo metadata --format-version 1`, `duplicates`, `paths_to`, and the
  `DepTree` (as `cargo tree` prints it, with `(*)` repeats, build and dev
  sections, `(duplicate)` marks and an optional summary) and `WhyTree`
  (`cargo tree -i`) renderables. `rich_ext::schema` (#244), behind `data`:
  `SchemaTree` (types, `(required)`, constraints, local `$ref`s and
  `$anchor`s resolved with cycle protection, `oneOf`/`anyOf`/`allOf`, items,
  `not`, `if`/`then`/`else`) and `SchemaDiff` (added, removed and changed,
  with breaking changes marked).
- `rs-rich-cli` 0.0.15: `rich dot FILE` (alias `graphviz`; `.dot` and `.gv`
  detected) draws DOT natively and exits 4 naming the construct and line it
  refuses; ```` ```dot ```` fences draw in Markdown; `--dot-backend
  text|graphviz|off` and the `dot_backend` key, where `graphviz` makes
  `--export-svg` write Graphviz's own SVG (falling back to the text
  drawing's, with a warning, without `dot`) and a working-directory
  `rich.toml` cannot choose it. `rich deps [MANIFEST]` runs `cargo metadata`
  (or reads `--metadata FILE`) and draws the tree, with `--why CRATE`,
  `--graph` (through the layout), `--depth N`, `--duplicates` and
  `--no-dev`. `rich schema FILE` draws a schema; `rich schema OLD NEW` what
  changed. `rich plugins` lists the built-in `dot` plugin. Depends on
  `rs-rich-diagram` directly (features `plugin`, `graphviz`).
- Fixtures and tests for each source: DOT files under
  `crates/rich-diagram/tests/fixtures/dot` (snapshots in Unicode and ASCII,
  and the refused constructs), `cargo metadata` and two schema versions
  under `crates/rich-ext/tests/fixtures/sources`, and CLI tests over the
  same files. Nothing under `crates/rich/src` changes; `rs-rich-plugin-api`
  is unchanged.
- Docs: a DOT section in the [Diagrams guide](docs/guide/diagram/index.md),
  [Dependency graphs and JSON Schemas](docs/guide/ext/sources.md) with
  output examples, the CLI reference, `docs/PORTING.md`, `docs/PLUGINS.md`,
  the crate READMEs, and `AGENTS.md`'s dependency graph (`rich-diagram`'s
  `plugin` feature now exists, and `rich-cli ──▶ rich-diagram`).
- Review fixes after #647: `rich deps --why --no-dev` (tree and `--graph`)
  leaves out paths that only a dev-dependency makes; duplicate versions are
  counted over the dependency kinds shown, so a version reachable only
  through a hidden dev edge no longer marks the visible one; `--graph
  --duplicates` keeps only the packages that lead to a duplicate.
  `DepGraph::dependents` and `paths_to` take the kinds to follow, and
  `WhyTree::kinds` and `DepGraph::duplicates_in` are new. `rich schema OLD
  NEW` compares keywords beside a `$ref` (they were dropped for the
  referenced schema), and a removed property is breaking only when the new
  schema's `patternProperties` or `additionalProperties` would refuse it.

### Diagrams (0.0.15 workstream 3)

- New crate `rs-rich-diagram` 0.0.1 (`rich_diagram`), depending on core
  `rs-rich` only: a graph model (`Graph`, `Node`, `Edge`, `Shape`,
  `Stroke`, `Head`, `Direction`), the layered layout (`draw` → `Drawing`),
  and a `Diagram` renderable (#227, #258) that measures to its drawing and
  crops each line at the width it is given, in box drawing or ASCII (which
  follows `Console::ascii_only` unless set). A chaining builder draws a
  diagram from code: `Graph::new(Direction::LeftRight).node("a",
  "API").edge("a", "b").label("reads")`. Labels lose their control
  characters before drawing. No `plugin` feature yet: nothing in this crate
  needs `rs-rich-plugin-api` until the DOT plugin (workstream 4).
- `rs-rich-mermaid` 0.0.4: the layout moved out unchanged; a parsed
  `Flowchart` converts into a `rich_diagram::Graph` (`Flowchart::to_graph`)
  and draws through `rs-rich-diagram`. `flowchart::{Direction, Shape, Node,
  Edge, Stroke, Head}` are now re-exports of `rich_diagram`'s types (same
  variants and fields), `layout::Diagram` is an alias of
  `rich_diagram::Drawing`, and `draw` keeps its signature. Every Mermaid
  snapshot is byte-identical, and a new test checks that random graphs built
  with the builder render as their Mermaid source.
- `rs-rich-cli` 0.0.15: requires `rs-rich-mermaid` 0.0.4 (manifest only from
  this workstream).
- Release tooling: `rs-rich-diagram` is in `RELEASES.toml` (before
  `rs-rich-mermaid`), `scripts/release.py`, the release workflow's tag
  filter, the release-readiness pattern, the CI test matrix (all features),
  the Python workflow's paths, `docs/BRANCHING.md` and the release skill.
  Its first version goes up through the release workflow's new-crate path
  with its own `rs-rich-diagram-v0.0.1` tag, after `rs-rich` and before
  `rs-rich-mermaid` 0.0.4.
- Docs: a [Diagrams guide](docs/guide/diagram/index.md) with a builder
  example and its output, the crate README, and `AGENTS.md`'s dependency
  graph and versioning table.

### KPI and status charts (0.0.15 workstream 2)

The renderables a dashboard is made of, in `rich_ext::chart` next to the
charts of workstream 1 and built on the same `Scale`, `ValueFormat`,
`Charset`, `Sparkline`, axis fitting and `chart.*` theme keys. Nothing under
`crates/rich/src` changed.

- `Gauge` and `BulletChart` (#251): a value against a range with a target
  marker (`│`) and threshold `Band`s. The track is shaded by band (`░`/`▒`,
  `.`/`:` in ASCII), the bar takes the style of the band the value is in
  and the band's name is written after the value. One line by default;
  `.full_width(true)` fills the width and adds a scale line and a legend.
  `BulletChart` stacks gauges with aligned columns.
- `Heatmap` (#250): a labelled grid of values as shades, five in blocks
  (` ░▒▓█`) and ten in ASCII (` .:-=+*#%@`), with `chart.heat.1`–`5`
  colours and a legend; missing values are `·` (`?`). Narrow widths merge
  neighbouring columns by their mean.
- `StatusMatrix` and `State` (#254): rows × columns of states, built-in
  `pass` `✓`, `fail` `✗`, `skip` `○`, `flaky` `≈` (`+ X - ~` in ASCII) or
  your own, each a symbol and a colour, with a legend that counts them.
- `KpiCard` and `Status` (#256): a label, a value, a delta with an arrow and
  sign (`▲ +8.5%`, `▼ -14`, `=`; `^`/`v` in ASCII) coloured by
  `higher_is_better`, an optional sparkline and a status (`✓ ok`,
  `! warning`, `✗ critical`). Cards measure to their content or take
  `.width(n)` / `.expand(true)`, so they sit in `Columns`, a table or a
  `Layout`.
- `Timeline`, `Span` and `Milestone` (#252): labelled ranges on a numeric
  (seconds or any unit) scale; overlapping ranges on a row are stacked,
  touching ones alternate `█`/`▓`, lengths are written after ranges,
  milestones are marked `◆`. When the range is wider than the plot, idle
  gaps are cut out (`≈` on the axis) so short ranges keep a column. No new
  dependency.
- New theme keys: `chart.track`, `chart.target`, `chart.ok`,
  `chart.warning`, `chart.critical`, `chart.unknown`, `chart.delta.*`,
  `chart.kpi.*`, `chart.heat.1`–`5`, `chart.state.*` and `chart.milestone`.
- Tests: `tests/kpi.rs` snapshots each renderable in blocks, Braille and
  ASCII with colour on (exact ANSI) and off, its shrinking order, a `Table`
  cell, a `Panel`, cards in `Columns`, and a dashboard `Layout` updating in
  `Live`; every new renderable joins the 1–200 width sweep and the
  ASCII-only check in `tests/charts.rs`.
- Example: `kpi_dashboard` (cards, a status matrix, gauges and a timeline
  in a `Layout`, updating in `Live`; `--ascii`, `--no-color`,
  `--frames N`). Docs: a section per renderable in the
  [Charts](docs/guide/ext/charts.md) guide, with screenshots from
  `guide_charts`.

### Charts (0.0.15 workstream 1)

`rich_ext::chart` (#221, #257): charts drawn with text, as renderables over
core's public `Renderable` and `Measurement`. Nothing under `crates/rich/src`
changed.

- `Sparkline`: one line, one cell per value (`▁▂▃▄▅▆▇█`, two values per
  cell in Braille, `_.-:=+*#` in ASCII). Given fewer cells than values it
  resamples, each cell the mean of an equal bucket. `.min_max(true)` and
  `.threshold(t)` style the cells and say it in words (`min 2 max 9`,
  `3 > 80`), so they read without colour.
- `BarChart`: label, bar and value per row, with eighth-cell blocks, half-cell
  Braille or `#` and `=` in ASCII; negative values extend left of zero. It
  shrinks the bar, then drops the values, then cuts the labels.
  `.orientation(Orientation::Vertical)` stands the bars up side by side
  (`▁▂▃▄▅▆▇█`, `⣤⣿`, or `#` and `.`), labels underneath and values just
  above each bar; negative values hang below zero. `Histogram` takes it too.
- `Histogram`: raw values into equal bins labelled `[low, high)`, rounded to
  round edges unless a range is given.
- `LineChart` and `Series`: lines and scatter points on a Braille canvas
  (`DotCanvas`, 2×4 dots per cell), with a y axis, x labels and a legend.
  Series differ by colour and marker; with no colour and more than one
  series the plot switches to markers at cell resolution. ASCII plots with
  `*`, `+`, `o`, `x` and `.`.
  Each row and column stands for one exact value at its centre; axis labels
  sit on evenly spaced rows and columns and name that value, never a
  rounded one. Without a fixed range the scales round out to a step that
  fits whole rows and columns, and without `.height(..)` the chart picks
  6 to 10 rows to suit its labels.
- Shared: `Scale` (linear, round ticks, NaN and infinities skipped, never an
  empty range, explicit bounds), `ValueFormat` (`1.2k`, `3.4M`, fixed
  decimals) and `Charset` (an ASCII console, by encoding or `Fidelity`,
  always gets ASCII). Theme keys `chart.*` in `extended_theme()`.
- Tests: snapshots for every chart in blocks, Braille and ASCII with colour
  on and off; every chart at widths 1 to 200 on four consoles never writes
  a line wider than its width, and the ASCII consoles' output holds no
  character above U+007F; charts in a `Table` cell, a `Panel` and `Live`.
- Docs: a [Charts](docs/guide/ext/charts.md) guide page with screenshots
  from `examples/guide_charts.rs`, and `examples/charts.rs`
  (`cargo run -p rs-rich-ext --example charts`).
- Versions, as the plan's package impact table says: `rs-rich-ext` 0.0.12 →
  0.0.13 for the new module, and its dependents for their requirement on it
  (Cargo reads `0.0.x` as exact): `rs-rich-micro` 0.0.2, `rs-rich-interact`
  0.0.3, `rs-rich-record` 0.0.3 (manifest only), `rs-rich-cli` 0.0.15, and
  PyPI `rs-rich` 0.0.4, whose wheel bundles them. `RELEASES.toml` synced.

### Micro asset docs

- A [built-in library](docs/guide/micro/library.md) page shows every
  built-in micro asset magnified from its own file, animations at their real
  frame timings, with its alt text, fallbacks and licence.
  `scripts/micro_gallery.py` regenerates the page and its images, and CI runs
  it with `--check`.
- A `micro-modes` tape records the same line with emoji and as half-blocks,
  then `rich micro preview fun/heart`. The recordings page gains a Micro assets
  section with it and the existing `micro`, `micro-chrome` and `explore-icons`
  tapes, which it did not show before.
- Two more tapes: `micro-create` (a PNG through the pipeline, `rich micro
  create --add`, listed and used, with a source image drawn for the docs) and
  `micro-doctor` (the mode `rich doctor` picks for a plain terminal, kitty,
  iTerm2, a known cell size and `RICH_MICRO=text`). The library page opens
  with a contact sheet of every asset.

## 0.0.14 cohort — published 2026-10-04

Art 0.0.12 / plugin API 0.0.3 / Mermaid 0.0.3 / lumis 0.0.3 / ext 0.0.12 /
micro 0.0.1 (new crate) / interact 0.0.2 / record 0.0.2 / CLI 0.0.14 /
Python 0.0.3. Core 0.0.9 and macros 0.0.3 were already published. The crates
were published from `main` at `324c65b` and `python-v0.0.3` at `61c002e`; see
[the release notes](docs/releases/0.0.14.md#publication).

### Release numbers and tagging

- `RELEASES.toml` lists every published package (the eleven crates and PyPI
  `rs-rich`) with its version, in publication order. CI checks it against the
  manifests and the workspace dependency graph; `scripts/release_cohort.py sync`
  updates it after a bump.
- `scripts/release_cohort.py tag` tags the merged commit on `main` from it: it
  skips any version already on crates.io or PyPI, pushes one annotated tag at a
  time, and waits for each release before the next. `status` and `--dry-run`
  show the plan.
- `release.yml` and `pypi-release.yml` fail a tag whose version is already
  published, in their first job, before the full gate or any build.
- Rust 1.99's Clippy (`needless_borrows_for_generic_args`) flagged one call in
  rs-rich-ext's completion generator; fixed so every CI job builds again.
- `release_cohort.py tag` reads committed manifests as UTF-8. It used the
  locale's encoding, so on Windows (cp1252) even `--dry-run` failed on the
  non-ASCII in rs-rich's manifest. `release.py` likewise. An unreachable
  registry now prints one error line (with the macOS certificate hint when that
  is the cause) instead of a traceback, and Python older than 3.11 is named.
- `pypi-release.yml`'s test job installs Pillow, as `python.yml`'s does: the
  micro assets page's example uses it, so the first `python-v0.0.3` run failed
  after building every wheel, before anything reached PyPI. A test now keeps
  the release job's test dependencies a superset of CI's.

### 0.0.14 release test: fixes from audit B

The release-test audit B of interact's composition, keymaps, kit,
built-ins, chrome, explorers and plugin components (B1–B8, and three of its
suspected issues). Each finding was reproduced, then fixed with a
regression test that failed before the fix (the spinner and viewport tests
cover new builders). Versions: no bumps; every crate touched
(`rs-rich-interact` 0.0.2, `rs-rich-cli` 0.0.14, `rs-rich-plugin-api`
0.0.3, docs only) is already unpublished.

- A plugin component's panic, caught by its `PluginView`, also ran the
  session's panic hook, which took the live session out of raw mode (and off
  the alternate screen, mouse and paste) and printed the panic over the
  view. Panics caught through the new `session::catch_panic` now leave a
  live session alone; uncaught panics still restore the terminal.
- `TreeSelect`, `DataExplorer` and `rich explore` took quadratic time on
  wide documents (each parent found by scanning back over earlier siblings)
  and memory proportional to nodes × depth (a path, JSONPath and guide
  string per node): a 400 KB nested array took 2.4 GB. Parents now come from
  a depth stack, `last` from one pass, paths and JSONPaths are worked out
  for the focused or copied node only (new `TreeSelect::paths_with`), guides
  are drawn for the rows on screen, and the explorer's preview draws at most
  a screenful of each container's children; `DataExplorer::focused` now
  returns an owned `Path`. `rich explore` refuses a document with more than
  1,000,000 nodes (exit 3) with a hint to narrow it with `--inspect
  --select`.
- `TextBuffer` (and so `Input`) could leave the caret inside a grapheme
  (typing in front of U+FE0F or a combining mark, deleting what kept two
  clusters apart, `set_caret`), and `window` measured the cells before the
  caret with `cell_len` but dropped whole graphemes, so it underflowed: a
  panic in debug builds, a wild caret column in release. The caret now
  always snaps to a grapheme boundary, and both sides of the window are
  counted grapheme by grapheme with saturating arithmetic.
- `kit::keep_ancestors` (and so `FilterState::set_tree` with a caller's
  parent list) panicked on a parent index past the list. Such a parent now
  ends the walk up.
- `Tabs` with no tabs panicked on Tab and Shift+Tab (an unchecked index of
  the active tab), and on a click below its bar (a container's mouse routing
  trusted a rect for a child that was not there); from Python,
  `Tabs().headless("shift+tab")` raised `PanicException`. Both are checked
  now.
- A plugin component never received the key names it declared: a key bound
  to its action arrived in canonical form (`esc` as `escape`, `Control+X` as
  `ctrl+x`), and Shift+Tab always as the undocumented `backtab`. A bound key
  now arrives as the first name the plugin declared for the action, spelled
  as declared, and `Key`'s `Display` writes Shift+Tab as `shift+tab` (which
  `Key::parse` already read), so plugins, Python components (`event.key`)
  and the docs agree.
- `TableSelect` copied (Ctrl+Y, Alt+Y; text, CSV and JSON) and handed
  actions the cells as drawn, with newlines turned into control pictures and
  tabs into spaces, so CSV quoting and JSON escaping never saw them. Copies
  and action values now carry the cells and headings as given; only the rows
  on screen are sanitised.
- `Overrides::parse` silently unbound an action bound to `#` (stripped as a
  comment) or `,` (a separator), as did a bare `,` in
  `Overrides::from_pairs` and `try_keys`. A line with no keys is now an
  error, `none` unbinds explicitly, `#` starts a comment only at the start
  of a line or after a space, and key names can be quoted (`"#"`, `","`,
  `'"'`). A bare `,` is an error everywhere.
- `StatusBar` spinners ignored `RICH_A11Y=reduced-motion` (and
  `no-animation`, `screen-reader`) and `RICH_ANIMATION=0`, which stop every
  other animation. They now hold their first frame and the bar stops
  ticking; `StatusBar::animate(bool)` overrides.
- `Viewport` run as a component advertised `viewport.*` bindings but matched
  hard-coded keys, so rebinding changed the help, not the keys. It now looks
  its keys up in its keymap (new `Viewport::rebind`, `viewport_keymap` and
  `ScrollState::act`); installed overrides apply too.
- `Breadcrumbs` and `Tabs` measured crumbs and titles with `cell_len` (a
  control is no cells) while the painter shows each control as a one-cell
  picture, so a click after a title holding controls landed on the wrong
  crumb or tab. Both now lay out and measure the text as painted.

### 0.0.14 release test: fixes from audit A

Versions: none bumped; `rs-rich-micro` 0.0.1, `rs-rich-ext` 0.0.12,
`rs-rich-cli` 0.0.14 and `rs-rich-interact` 0.0.2 are already unpublished.
Nothing in `rs-rich` changes.

- **F1**: control characters (C0, C1, DEL) are refused in every manifest and pack string (`version`, `license`, `author`, `description`, package and image paths through `safe_relative`) and in `micro create`'s metadata, and every `rich micro` message and table cell that quotes a package name, path or error shows controls instead of running them.
- **F2**: `rich -p --emoji` with `:micro:` assets exports (`--export-html`/`--export-svg`), pages (`--pager`) and captures the fallback, never Kitty placeholder cells or the blank cells under an iTerm2/Sixel image; the terminal shows the fallback too in that case.
- **F3**: the `CSI 16 t` cell-size query keeps typeahead: it is skipped when input is waiting, and stops at a `CSI c` sentinel reply. Bytes read during the exchange that are not a reply are dropped, never replayed with `TIOCSTI`, which would feed terminal replies to the shell as typed input.
- The cell-size query waits on the terminal with `select`, not `poll`: macOS's `poll` rejects `/dev/tty` (`POLLNVAL`), so the query never ran there. Found by the macOS CI job.
- The cell-size query waits up to 500 ms, not 100 ms. The device-attributes request sent with it ends the wait as soon as the terminal answers, so only a terminal that answers nothing waits the full time; the shorter timeout let a reply over a slow link (or a loaded CI runner) arrive late and reach the shell as typed input.
- **F4**: the cell-size query runs only from the terminal's foreground process group, with `SIGTTOU`/`SIGTTIN` blocked, so a background `rich -p --emoji` job is no longer stopped.
- **F5**: `rich micro create --output` (and `add`, `install`) check the destination with `symlink_metadata`, and `create` only removes what it created: a dangling link at the destination is refused and left alone.
- **F6**: only regular files are opened as `.richmicro`/`.zip` archives; a FIFO, socket or device in a layer directory is skipped with a warning instead of hanging `rich micro` (including `--project` commands in an untrusted project, which still never draw that project's assets).
- `RICH_CLIPBOARD=1` no longer writes OSC 52 to output that is not a terminal; the override applies on terminals only, as the docs now say.
- The on-disk micro image cache names its files by a SHA-256 of the source bytes and geometry instead of a 64-bit FNV-1a, so a crafted package cannot replace another asset's cached image.

### Components from Python and plugins: plugin API 0.0.3, Python 0.0.3

Versions: `rs-rich-plugin-api` 0.0.2 → 0.0.3 (its source changed and 0.0.2
is published), `rs-rich-lumis` 0.0.2 → 0.0.3 for its requirement on it
(`rs-rich-mermaid` 0.0.3 was already unpublished), and PyPI `rs-rich`
0.0.2 → 0.0.3. `rs-rich-ext` 0.0.12 and `rs-rich-interact` 0.0.2 were
already bumped. `PLUGIN_API_VERSION` stays 1 and the runtime ABI stays 1.0:
everything here is added, with default bodies.

0.0.14 workstream 4 ([plan](docs/plans/0.0.14.md)): components written in
Python, and components that plugins ship, compose with the built-ins.
Nothing in `rs-rich` changes.

- **Python components** (`rs_rich.interact`). Subclass `Component`:
  `render(context)`, `handle(event)` and `keymap()`, with defaults for
  `focusable()`, `mouse()`, `tick()`, `start(context)` and
  `default_value()`. `handle` returns `None`, `Done(value)`, `Cancel()` or
  the new `Ignored()`, which lets a key bubble to the container. The
  component is bridged to a Rust `Component`, so it runs through the same
  drivers as the built-ins, headless included.
- **Containers from Python.** `Column`, `Row`, `Stack`, `Split`, `Tabs` and
  `Layers` (with `Layer.modal` and `Layer.popover`), plus `Label` and `Map`,
  hold built-ins, Python components and each other. Each run builds a fresh
  Rust tree. A built-in's answer finishes the composition as its Python
  value unless a `Map` says otherwise. Containers take `on`, `shortcut`,
  `rebind` and `with_mouse`, and children are checked when added.
  `Keymap` and `Binding` declare keys, and `keymap(component)` lists a
  composition's bindings.
- **Errors come out of the run.** The GIL is released between events and
  taken for each call into Python. An exception from `handle`, `render`, a
  `Map` or binding callback, or a layer factory ends the run, and `ask`,
  `run`, `headless` or `degrade` raises it. So does a callback that returns
  something other than a flow. A container that contains itself raises
  `RecursionError`. `rs_rich.plugins.Capability` knows the `component` kind.
- **Plugin components** (`rs-rich-plugin-api`).
  `PluginRegistrar::component(name, factory)` registers a named
  `PluginComponent`, from the new `rich_plugin_api::component` module:
  keys by name, a `ComponentView` of core segments, and a text answer, so a
  plugin still depends on core only. `Capability::Component` reports it.
  `ExtensionRegistry` hosts it with `component(name)`, `component_names()`
  and `create_component(name)`, which catches a panicking factory.
  `rich_interact::plugin::PluginView::mount(&registry, name)` mounts it
  beside the built-ins. Its keys are listed under the registered name and
  can be rebound there. A plugin component that panics shows the panic in
  its pane and stops taking focus, without taking the app down.
- **Linked and in-process plugins only.** A component keeps state, and the
  runtime ABI is stateless text (a WASM call runs in a fresh instance), so
  a native or WASM plugin cannot register one in this release.
  `rich plugins list` shows linked components as `component "name"`.
  There is no CLI command to mount one yet.
- Docs: [Building your own components](docs/guide/interact/custom-components.md)
  has Python and plugin sections, [Plugins](docs/PLUGINS.md) the new
  registrar method, and the Python guide's
  [Interactive components](docs/python/interact.md) the `Component` class
  and the containers.

### Micro assets: pipeline, built-in library, `rich micro` and the showcase

Versions: `rs-rich` (PyPI, `crates/rich-py`) 0.0.2 → 0.0.3. `rs-rich-art`
0.0.12, `rs-rich-micro` 0.0.1, `rs-rich-interact` 0.0.2 and `rs-rich-cli`
0.0.14 change but are unpublished and keep their versions. Nothing in
`rs-rich` changes.

0.0.14 workstream 7 ([plan](docs/plans/0.0.14.md), #568, #571, #572, #581,
#582, #587): the tools to make and use micro assets, and the interactive
views that show them.

- **Pipeline (#571, #572).** `rich_micro::pipeline::Pipeline` fits any PNG,
  APNG, GIF or JPEG to exactly an asset's cells, frame by frame: brightness,
  contrast, gamma and grayscale; contain, cover (with an anchor) or stretch;
  an optional unsharp mask; transparency cut at a threshold, kept, flattened
  or keyed from a colour; an optional palette with dithering; then
  animations are deduplicated and rate-limited. `Processed::preview`
  magnifies the result. `rich_micro::create::write_package` writes a package
  directory or `.richmicro` zip and reads it back, so what it returns is
  what the registry accepts. Built on new `rs-rich-art` graphics helpers:
  `fill_pixels`, `adjust`, `sharpen`, `threshold_alpha`, `reduce_colors`.
- **Built-in library (#581).** `status/success`, `status/warning`,
  `status/error`, `status/info`, `status/loading` (animated), `dev/bug`,
  `dev/branch`, `dev/terminal`, `dev/package`, `fun/heart` (animated),
  `fun/star`, `fun/cat` and `fun/coffee` (animated), drawn for this project
  by `examples/gen_builtin.rs` and embedded (`rich_micro::builtin`). Each has
  alt text, an emoji and a text fallback and the MIT licence; no
  third-party logos; every name is namespaced, so none is an emoji code.
  Tests enforce all of that and that the committed files match the
  generator. `MicroRegistry::builtin()` loads it;
  `MicroRoots::builtin_set` (on in `from_env`) puts it under
  `MicroRegistry::load`. A `PackageLocation::BuiltIn` locates its files.
- **CLI (#568, #582).** `rich micro list | show | preview | add | remove |
  create | install | uninstall | packs`, with `--report json` (or `--json`).
  `add`, `remove`, `install`, `uninstall` and `create --add` use the user
  layer unless `--project`. The project's `.rich/micro/` loads only when
  trusted: `--micro-project`, or the new `micro_project` key in the user's
  own config; a working-directory `rich.toml` cannot set it (ignored with a
  warning, as for `plugins`). `:micro:name:` expands in `--print` markup
  with `--emoji`, drawn as the terminal can. `rich asset --kind micro` and
  `rich explore --icons` (status assets for `true`, `false` and `null`).
- **Interact.** `Select::set_icons` (and `TreeSelect`/`TableSelect`),
  `StatusItem::Icon` / `StatusItem::icon`, `Breadcrumbs::icons`,
  `Palette::category_icon` / `Overlays::category_icon`,
  `DataExplorer::icons` and `kit::icon` keep an icon's style metadata, so
  the painter's graphics draw micro assets in rows, badges, crumbs and
  palette categories. A new `micro` feature adds `AssetKind::Micro`,
  `AssetPicker::micro` and `StatusItem::micro`; the `micro_showcase`
  example shows them all. Migration: an exhaustive `match` on `AssetKind`
  or `StatusItem` needs the new variants (or a wildcard when another crate
  may turn on `micro`).
- **Python.** `rs_rich.micro`: `MicroRegistry`, `MicroAsset` and
  `MicroMarkup` (renderables drawn as the console can), `micro_markup`,
  `micro_load_package`, `micro_create_package` and `micro_mode`, with stubs
  and tests; `AssetPicker("micro")`.
- **Docs (#587).** [Authoring micro assets](docs/guide/micro/authoring.md),
  [micro assets in the CLI](docs/guide/micro/cli.md), a
  [terminal compatibility matrix](docs/guide/micro/terminals.md), a Python
  page, and tapes of `rich micro`, micro assets in the chrome and palette,
  and `rich explore --icons`, run in CI by `rich record --check`. They are
  recorded in a PTY with no image protocol, so they show the emoji and
  half-block fallbacks.

### Micro 0.0.1 (new crate): model, packages, registry, markup

- New crate `rs-rich-micro` (`rich_micro`, #565), the foundation for micro
  assets: emoji-sized inline images written `:micro:name:`. Core is
  unchanged (`:micro:` is not an emoji code); the crate depends on
  `rs-rich`, `rs-rich-ext` and `rs-rich-plugin-api`, plus `zip` (read-only,
  deflate through flate2's pure-Rust backend) and `serde_json`.
- Model (#566, #579, #585): `MicroAsset` with a validated name
  (`[a-z0-9_-]` segments, `/` namespaces), kind, size in cells (2×1 default,
  1×1 allowed, at most 4 columns; more than one row is refused), images,
  mandatory alt text, an emoji or text fallback that must fit, aliases and
  origin, version, licence and author.
- Packages (#580): `.richmicro` zips or directories with a `manifest.json`
  (`schema_version: 1`), a static image and an optional GIF, APNG or WebP
  animation or frame list; packs with a `pack.json`. Read-only. Hard
  `Limits` on archive, file and manifest size, entries, bytes read, pixel
  dimensions, frames and decoded bytes, checked from image headers without
  decoding; zip-slip, absolute paths, backslashes and symbolic links out of a
  package are refused.
- Registry (#567, #583): built-in < user (`~/.config/rich/micro/`) < trusted
  project (`.rich/micro/`, loaded only when the caller trusts the project) <
  inline; aliases within a layer; collisions and rejected packages reported
  in file-name order; `explain(name)` through `rich_ext::cli_doc::Precedence`.
  The built-in layer is present and empty until the built-in library lands.
- Markup and API (#569, #570): `:micro:name:`, `\:micro:` to escape, unknown
  names left literal with a diagnostic. `render_markup` / `markup_text` /
  `PreparedMarkup` expand tokens before core's markup and emoji passes (so
  `:micro:x::fire:` gets both); `expand`, `MicroTransform`, `MicroPlugin`
  and `MicroExt` (`append_micro`, `expand_micro`) work on parsed text;
  `MicroAssetRef` is a renderable.
- Width (#578): an asset is exactly its columns of fallback text (padded with
  U+2800, which is not whitespace) under style metadata
  `rich.micro = [name, cols, rows, id]`, so measure, wrapping, cropping,
  tables and panels size it exactly and pipes and exports get plain text.
  `MicroView` offers each complete placement to `MicroRenderer`s, the seam
  terminal graphics plug into (see the rendering section below).
- Release tooling knows the crate: `release.py`, the package checks, the
  release workflow's tags, the CI test matrix and the readiness version
  pattern (which now also matches `rs-rich-record` and `rs-rich-interact`
  tags).

### Explorers and utilities: `rich explore`, OSC 52, tree filtering, reload, a theme picker

Versions: `rs-rich-ext` 0.0.11 → 0.0.12 (its source changed and 0.0.11 is
published), and `rs-rich-record` 0.0.1 → 0.0.2 for its new requirement on
it (manifest only). `rs-rich-interact` 0.0.2 and `rs-rich-cli` 0.0.14 were
already bumped for this release. Nothing in `rs-rich` changes.

0.0.14 workstream 3 ([plan](docs/plans/0.0.14.md), #465, #464, #428, #488,
#434, #485, #460), built on workstream 1's kit:

- **`rich explore [FILE]`** (#465): explore a JSON, YAML, TOML, XML, INI or
  .env document, from a file or stdin (`--format` as for `--inspect`), as a
  tree. Left and Right fold, typing searches keys and values, the line
  under the prompt shows the focused node's path (`$ › server › port`), a
  preview draws its subtree, Ctrl+Y copies its JSONPath and Alt+Y its
  value, and Enter prints the JSONPath `--select` takes (`--print value`
  prints the value). It paints on stderr, so `$(rich explore f)` works;
  without a terminal it prints the tree. PTY tests run it on every format
  `--inspect` reads; a tape records it.
- **`rich_interact::DataExplorer`** (new `data` feature) is that explorer
  as a component, with `components::json_path`.
- **OSC 52 clipboard** (#488): `rich_ext::clipboard` detects, with
  provenance, whether the terminal takes OSC 52: never when the output is
  not a terminal, on `TERM=dumb` or inside tmux or screen; yes for kitty,
  iTerm2, WezTerm, Windows Terminal, ghostty, Alacritty, foot, contour, rio
  and VS Code; `RICH_CLIPBOARD=0|1` overrides. Copies over 74,994 bytes are
  refused. Components call `rich_interact::clipboard::copy`; the event loop
  writes the sequence through a new `Backend::clipboard`/`Backend::copy`
  pair (the headless driver records copies in `Record::copies`).
- **Copy from a table** (#434): `TableSelect` copies the focused row
  (Ctrl+Y) or cell (Alt+Y) as text, CSV or JSON (`CopyFormat`, cycled with
  Alt+F); Ctrl+Left/Right move the focused column. Keys in context `table`.
- **Trees** (#464, #428): `TreeSelect` filters as a tree. Each match keeps
  its ancestors, dimmed, the guides join what is listed, and the cursor goes
  to the best match. `breadcrumbs(true)` shows the path line (drawn by a
  private helper until workstream 2's breadcrumbs component lands),
  Ctrl+Y copies the node's path, and `crumbs`, `paths`, `fold_below` and
  `set_collapsed` are new. The rule is public: `FilterState::set_tree`,
  `is_context`, `best`, and `kit::keep_ancestors` and `kit::ancestors`.
- **Reload** (#485): `Select::reload` (and `MultiSelect`'s) replaces the
  items while keeping the query, the focused item and the marks;
  `reload_on(key, source)` reloads on a key (action `select.reload`) and
  `reload_from(receiver)` takes lists sent from another thread.
  `Select::set_status` shows a line in place of the footer until the next
  key.
- **`ThemePicker`** (#460): pick a theme while a sample renders in it;
  `from_registry` offers plugins' theme packs
  (`ExtensionRegistry::theme_names`, new).
- Also in ext: `data::display_value` and `data::copy_text`.

Existing behaviour that changes: typing in a `TreeSelect` now lists the
matches' ancestors too, in tree order, instead of a flat ranking.

### Micro assets on a terminal: Kitty, iTerm2, Sixel, fallback and animation

Versions: `rs-rich-art` 0.0.11 → 0.0.12 and `rs-rich-ext` 0.0.11 → 0.0.12
(their source changed and 0.0.11 is published), `rs-rich-mermaid` 0.0.2 →
0.0.3 and `rs-rich-record` 0.0.1 → 0.0.2 for their new requirements on them.
`rs-rich-micro` 0.0.1, `rs-rich-interact` 0.0.2 and `rs-rich-cli` 0.0.14 are
unpublished and keep their versions. Nothing in `rs-rich` changes.

0.0.14 workstream 6 ([plan](docs/plans/0.0.14.md), #572–#577, #584):
micro assets are drawn with the terminal's own graphics where it has them.

- **Choosing (#573).** `rich_micro::select` picks Kitty, then iTerm2, then
  Sixel (when the cell size in pixels is known), then half-blocks, then the
  emoji or text fallback. `RICH_MICRO=kitty|iterm|sixel|blocks|text`
  overrides it on a terminal; output that is not a terminal always gets the
  text fallback. `rich doctor` reports the choice, its reason, the cell size
  and whether assets animate (`micro` in `--report json`).
- **Cell size and graphics beside core (ext).**
  `rich_ext::graphics::GraphicsEnvironment` carries the graphics protocol
  and the cell size in pixels (`RICH_CELL_PIXELS=WxH`, then `TIOCGWINSZ`'s
  pixel fields through `rustix`, then a `CSI 16 t` query made only when
  stdin and stdout are a terminal), leaving core's `TargetCapabilities` as
  upstream has it. `Environment` gains `cell_pixels` (defaulted) and
  `MapEnvironment` a `cell_pixels` field and builder; `Field::new` is public.
- **Kitty (#574), iTerm2 (#575) and inline Sixel (#576) in `rs-rich-art`.**
  New `kitty` and `iterm` features: Kitty's graphics protocol with Unicode
  placeholders (U+10EEEE and the row/column diacritics), chunked quiet
  transmission, frame animation and delete by id; iTerm2 inline images at an
  exact size in cells. `SixelArt::encode_cells` is the Sixel encoder at an
  exact cell size without a trailing line break. The `graphics` module fits
  images to an exact number of cells, decodes and resamples animations under
  a memory budget, encodes PNG and GIF, and draws an image over reserved
  cells with the cursor saved and restored (`graphics::overlay`).
- **Drawing (`rs-rich-micro`).** `MicroGraphics` holds a terminal's drawing
  state. Its renderer draws every asset in a `MicroView`: Kitty placeholders
  (each image transmitted once per session and reused by id), iTerm2 and
  Sixel images over blank cells, or block cells, and checks at render time
  that the console is a terminal, so pipes, logs and exports stay byte for
  byte the fallback. Kitty placeholders are only ever produced after the
  markup pre-pass, and drawn output fed back to it is reported, not reused.
- **Fallback (#577).** Where no protocol draws: the emoji or text, then a
  half-block rendering at exactly the asset's cells (on a colour terminal),
  then the alt text.
- **A graphics side channel (ext, interact).** `Frame`s carry `Placement`s
  beside their cells (`frame::graphics`: `Graphic`, `PlacementSource`,
  `plan`). `LiveCoordinator::with_graphics`, `Painter::set_graphics` and
  `EventLoop::graphics` draw them after the cell diff, redraw them when the
  cells under them are repainted, never draw them off a terminal, and
  release what the terminal holds when the view closes, so no Kitty id
  leaks.
- **Animation (#572).** Frames are fitted to the cells, deduplicated and
  rate-limited under the package limits. Kitty and iTerm2 play them
  natively; Sixel and blocks show the frame the clock is on at each redraw,
  and the event loop wakes for the next one. `RICH_A11Y=reduced-motion`,
  `RICH_ANIMATION=0` and non-interactive output show the still image.
- **Cache (#584).** Decoded images are cached in memory (keyed by source
  hash, size in cells and cell size; 32 MiB by default) and, for
  `MicroGraphics::detect`, as fitted PNGs under the user cache directory.
- **Tests.** A PTY test per protocol plays the output through a real
  pseudo-terminal into an emulator and checks bytes and cursor position
  through wrapping, a table, a scroll and a live redraw; fallback output is
  byte-identical in pipes and exports; animation stops with reduced motion;
  no Kitty id is left after a view closes.
- **Not yet:** WebP animations and frame-sequence animations draw their
  still image; images are one row high.

### Interact 0.0.2: composition, a public kit and a keymap registry

Versions: `rs-rich-interact` 0.0.1 → 0.0.2 (its source changed and 0.0.1 is
published), and `rs-rich-cli` 0.0.13 → 0.0.14 for its new requirement on it.

0.0.14 workstream 1 ([plan](docs/plans/0.0.14.md), #478, #479, #480): you can build your own components
from the built-ins' pieces and compose them with the built-ins. Nothing in
`rs-rich` changes.

- **Containers are components** (`rich_interact::compose`). `Column`, `Row`
  and `Stack` lay children out with `Size::Fixed`, `Size::Flex` and
  `Size::Auto`. `Split` puts two panes side by side or stacked. Its border
  drags with the mouse, the #476 preview border generalised as
  `kit::Divider`, and moves with Alt+H/L or Alt+K/J, within minimum sizes.
  `Tabs` keeps every tab's state and switches with Alt+Left/Right,
  Ctrl+PageUp/PageDown, Alt+1–9 or a click. `Layers` hosts
  `Layer::modal` and `Layer::popover`, with a backdrop, a focus trap,
  Escape to dismiss, and a `LayerHandle` to open layers from anywhere.
  `ComponentExt::map` adapts a child's answer to the container's output,
  and `Label` and `Painted` show without taking focus.
- **Focus, routing and bubbling.** Each child gets a `Rect`. Keys go to
  the focused child, and bubble to its containers when unused. Mouse events
  arrive in the child's coordinates, and a press holds its child until the
  release. Tab and Shift+Tab move focus through nested containers and wrap
  at the top. Containers take their own bindings with `on` (after the
  child) and `shortcut` (before it), and `rebind` them.
- **A keymap registry** (`rich_interact::keymap`). A component declares
  `Binding`s (keys, action, description, context) in a `Keymap`, and
  `Component::keymap` lists the bindings active now, the focused child's
  first. Keys rebind per component (`Select::rebind`, `Input::rebind`, and
  `rebind` on each container) or process-wide with `keymap::install` of
  `Overrides`, parsed from `context.action = keys` lines or a
  configuration table. Every built-in declares its keys. `Select` (and the
  views built on it), `Input` and the containers dispatch through their
  keymaps.
- **A public kit** (`rich_interact::kit`). It holds the line helpers
  (`text`, `plain`, `width`, `fit`, `pad`, `highlight`, `question`,
  `pasted`, `shown`, plus `slice`, `overlay` and `restyle`) and the state
  types `ListState`, `ScrollState`, `FilterState`, `TextBuffer`, `Divider`
  and `ActionMenu`.
- **Built-ins rebuilt on the kit.** `Select` is a `FilterState`, a
  `ListState`, a `Divider` and an `ActionMenu`. Its composing hooks
  (`event`, `replace_items`, `focus_item`, `set_values`, `set_kind`,
  `set_heading`, `set_prefixes`, `set_hidden`, `refilter`, and more) are
  public. `Input` edits a `TextBuffer`, and its form hooks (`field`,
  `resolve`, `suggestion_rows`, and more) are public. `Viewport` scrolls a
  `ScrollState`. Every 0.0.13 component test and tape passes unchanged.
- **A guide and an example.**
  [Building your own components](docs/guide/interact/custom-components.md)
  and `examples/custom_component.rs` build a checklist from kit pieces and
  compose it with `Select`, `Input` and `Confirm` in a split, tabs and
  modals. `tests/compose.rs` tests it headless, and `tests/pty_compose.rs`
  in a real PTY.
- **Migration (breaking, minimal).** `Flow` has a new variant,
  `Flow::Ignored` ("not mine, let it bubble"), so an exhaustive `match` on
  `Flow` needs an arm for it. The event loop treats it as `Continue`.
  `Component` gains `keymap`, `focusable`, `focus_step` and `focus_enter`,
  all with defaults, and `Box<C>` is now a `Component`. Built-ins now return
  `Ignored`, not `Continue`, for keys they do not use. This makes no
  difference at the top of an event loop. `Input` leaves Tab to its
  container when no suggestion is showing.
- **Deferred.** `TextArea` still edits its own lines, not a `TextBuffer`.
  `Confirm`, `Pager`, `TextArea`, `ColorPicker`, `Form` and `FilePicker`
  declare their keymaps but still match keys inline, so rebinding does not
  reach them yet.

### Interact 0.0.2: overlays and chrome

0.0.14 workstream 2 ([plan](docs/plans/0.0.14.md), #453, #473, #474, #475,
#481, #482): overlays that read the keymap, and chrome round a view, built
on workstream 1's layers, keymap and kit. Each is a component of its own
and works alone, in a layer or container of yours, or through `Overlays`.
Nothing in `rs-rich` changes. `rs-rich-interact` is already at 0.0.2 and
unpublished, so no version moves.

- **`Overlays`** (`rich_interact::overlay`, or `ComponentExt::with_overlays`)
  wraps any component. Ctrl+O opens the command palette, F1 the help
  overlay, and `?` or F2 the shortcut sheet. Each opens only when the
  component does not use the key, and each reads the component's keymap
  when it opens. The keys rebind with `rebind` or under context
  `overlays`. The overlays are modal layers of a `Layers` host.
- **Command palette (#453).** `Palette` lists `Command`s by category, each
  with its shortcut, and finds them by fuzzy search (`FilterState`). It
  builds from a keymap (`from_keymap`, `Command::from_binding`). `hints`
  reads each command's current keys from the keymap, and `contexts` lists
  only the active contexts. Under `Overlays`, a command of yours runs its
  handler, and a binding runs by sending the component its key.
- **Help overlay (#473) and shortcut sheet (#475).** `Help` groups every
  binding by context and searches keys, descriptions and contexts as you
  type. `Shortcuts` shows every binding's first key in columns, and any key
  closes it.
- **The action menu is a modal (#474).** Ctrl+K in `Select` (and in
  `TableSelect`, `TreeSelect`, `FilePicker` and `AssetPicker`) opens the
  menu in a box over the list, titled with its target, over a dimmed
  backdrop. Its keys and mouse behaviour are unchanged. One test asserted
  the menu's old inline rows and now asserts the boxed ones. Actions can
  also target a region: `TargetKind::Region` (`region` for plugins), which
  `Overlays::region`, `actions` and `on_action` open in a `Menu` when the
  component leaves Ctrl+K unused.
- **Status bar (#481)** (`rich_interact::chrome`). `StatusBar` holds
  `StatusItem`s on the left and right: text (markup), key hints read from
  the keymap, spinners (core's frames, picked by time, with a replaceable
  clock), and badges. A `StatusHandle` changes items while the bar shows.
  Under `Overlays::status_bar`, the hints follow whatever has the keys.
- **Breadcrumbs (#482).** `Breadcrumbs` shows a path, drops crumbs off the
  left behind `…` when it does not fit, and goes back to a clicked crumb.
  A `Crumbs` handle changes the path.
- **Kit.** `kit::frame` boxes lines with a titled border, and `kit::place`
  draws a box over a view with an optional backdrop. `Layers` and the
  action menu both draw with them. `ActionMenu::size` gives the menu's
  size. `chrome::key_hint` and `keys_hint` write keys short (`↑`, `esc`,
  `pgdn`) for hints.
- **Fix.** `Form` now returns `Flow::Ignored` for keys its field does not
  use, like the other built-ins, so containers and overlays get them.
- **Tests, example, guide and tapes.** `tests/overlays.rs` opens every
  overlay over all twelve built-ins and over workstream 1's custom
  component, and runs palette commands on them. `tests/pty_overlays.rs`
  runs `examples/overlays.rs` in a real PTY. The guide page is
  [Overlays and chrome](docs/guide/interact/overlays.md), and three new
  tapes record the palette, the help overlay and the status bar
  (`docs/tapes/palette.tape`, `help.tape` and `statusbar.tape`).

### Release workflow: a new crate's first version

- crates.io offers Trusted Publishing only on a crate that already exists, so
  a new crate's first version used to need a hand upload. `release.py
  preflight` now also reports whether a selected crate exists at all. For a
  brand-new crate, `release.yml` skips the OIDC exchange and uploads with the
  `crates-io` environment's `CARGO_REGISTRY_TOKEN`, failing if it is missing.
  Existing crates publish through Trusted Publishing only, as before, and a
  test pins that only the new-crate step can read the secret. A selection
  that mixes new and existing crates (a coordinated `vX.Y.Z` tag) is refused
  before anything uploads. This is how `rs-rich-interact` and `rs-rich-record`
  0.0.1 go out.

## 0.0.13 cohort — published 2026-09-29

Core 0.0.9 / plugin API 0.0.2 / macros 0.0.3 / art 0.0.11 / Mermaid 0.0.2 /
lumis 0.0.2 / ext 0.0.11 / interact 0.0.1 (new crate) / record 0.0.1 (new
crate) / CLI 0.0.13 / Python 0.0.2, all published from `main` at `edbf73f`;
see [the release notes](docs/releases/0.0.13.md#publication).

### Core 0.0.9 / ext 0.0.11: `syntax` and `markdown` are optional

- `rs-rich` has two new default features: `syntax` (`Syntax` and
  `SyntectHighlighter`, through syntect) and `markdown` (`Markdown`, through
  pulldown-cmark; implies `syntax`). A default build is unchanged. With
  `default-features = false` the dependency tree loses syntect, its
  unmaintained bincode 1.x (RUSTSEC-2025-0141), the second `fancy-regex` and
  pulldown-cmark. `onig` and `syntax-cache` now imply `syntax`.
- `rs-rich-ext` forwards the same two default features. Without them it leaves
  out `rst`, `cli_doc::markdown_view`, the `syntect` registry entry and
  `testing::conformance`; `source_view`, `SourceDiff` and `PatchView` still
  render, unhighlighted. The workspace depends on core with `default-features = false`,
  so each crate asks only for what it uses: `rs-rich-plugin-api`,
  `rs-rich-macros` and `rs-rich-art` no longer pull syntect in.
- CI builds and tests core without default features, and fails if a lean
  `rs-rich` or `rs-rich-ext` depends on syntect, bincode or pulldown-cmark.

### CI: macOS, and interactive commands on macOS

- A macOS job runs the whole workspace's tests, plus core without default
  features, and is part of `ci-ok`.
- `rs-rich-interact` reads terminal events with crossterm's `use-dev-tty`
  source (poll/select) instead of mio on every Unix. On macOS, kqueue cannot
  watch `/dev/tty`, so the interactive commands exited as soon as stdin was a
  pipe (`seq 1 99 | rich pager`, `ls | rich filter`, `echo draft | rich
  write`) or the command ran in the background, and keys typed while a
  session was suspended were lost after `fg`. Linux behaves as before.

### Core 0.0.9: string table cells are markup (docs)

- A string cell is console markup, as upstream's is, and the compiler cannot
  tell data from markup: `add_row(&[name])` or `Cell::from(name)` with a model
  or column name containing `[` restyles or hides the rest of the cell. The
  `From` conversions and `add_row` now carry doc warnings pointing at the
  literal form core already has, a `Text` cell, as upstream's
  `add_row(Text(name))`: `add_row_text(vec![Text::new(name)])` or
  `Cell::from(Text::new(name))`. No API or rendering change. The tables guide
  and the 0.0.13 migration notes carry a security note.

### Dependencies

- `rs-rich-macros` builds on `syn` 3, and `rs-rich-record` on `color_quant`
  2, with no code change. `log` 0.4.34 and `thiserror` 2.0.21 in the lock
  file. The PyPI release workflow uses `actions/upload-artifact` v7 and
  `actions/download-artifact` v8.

### Interact 0.0.1, CLI 0.0.13 and Python 0.0.2: suspend, in-process runs, `--panel`

- **Ctrl+Z suspends an interactive command.** Raw mode turns off the
  terminal's own Ctrl+Z, so it reached components as a key and did nothing;
  a SIGTSTP from outside stopped the process with raw mode, the alternate
  screen and mouse reporting still on. Both now give the terminal back,
  stop the process as the shell expects, and on `fg` turn the modes back on
  and repaint the whole view. `Backend` gains `can_suspend`, `suspend` and
  `take_resumed` (all defaulted); the headless driver can suspend when asked
  (`Headless::suspendable`) and counts it (`Record::suspends`). `SIGSTOP`
  still cannot be caught.
- **Python threads running the CLI no longer lose each other's plugins.**
  The wheel releases the GIL around the command line, and every run starts
  by clearing the process-wide plugin slot, so a run on one thread cleared
  another's plugins mid-render (23 runs in 100 lost theirs in the test).
  `rich_cli::run_embedded` now runs one command line at a time.
- **`--panel` takes every box style in rich's `box` module** (`heavy_head`,
  `minimal`, `simple`, `markdown`, …), not only rich-cli's six, so every name
  `rich asset --kind box` offers works with it. The same names are valid
  for `panel` in config; an unknown name lists them all.

### 0.0.13 release test, round 2: plugins, recordings and the new commands

Three independent audits covered the scope added before release (#618, #619,
#620). Each finding was reproduced, then fixed with a regression test that
failed before the fix.

- **Plugin API 0.0.2, ext 0.0.11, CLI 0.0.13 (plugins):**
  - A WASM plugin's table was unbounded, so `(table 400000000 funcref)` cost
    about 1.6 GB outside the 64 MiB memory cap. Tables are now capped at
    10,000 elements (`WasmLimits::table_elements`).
  - `[link=…]` markup in `fence-markup` output, and `link` in a highlighter
    span, printed a hyperlink, so a plugin could show one URL and link to
    another. Links and click meta are now removed from all plugin output.
  - A linked plugin whose id clashed with a built-in (`mermaid`) was dropped
    silently, with every linked plugin sorted after it. The clash is now
    reported before anything runs.
  - A native plugin whose vtable held a null `call` or `free` was accepted,
    then crashed rich on first use. `PluginVTable`'s fields are now
    `Option`s (the same layout), and `read_descriptor` refuses a null one,
    returning the checked `PluginFunctions`.
  - A library that failed to open said only "dlopen failed", as an input
    error (exit 3). The system loader's reason is now included, and it is
    "not a rich plugin" (exit 2), as a bad WASM module is.
  - `rich plugins info` cut a long path with "…", and showed a WASM
    plugin's path as typed. Paths now fold, and WASM paths are canonical.
- **Record 0.0.1 and ext 0.0.11 (recordings and frame export):**
  - SVG screenshots put text and backgrounds in the wrong columns after a
    character rich and the terminal emulator measure differently (a
    skin-tone modifier, a Devanagari vowel sign). They now follow the grid,
    as the PNG and text screenshots already did.
  - Frame SVG and HTML export was quadratic in the number of distinct
    styles: 45 s for a 500x200 screenshot with a colour per cell in a debug
    build. It is now linear.
  - A link URL holding control characters made the SVG malformed XML; they
    are now percent-encoded.
  - Link schemes are not filtered, as in upstream's `export_html`: the docs
    now say so, and how to drop untrusted links before exporting.
  - The `write`, `file`, `color` and `asset` tapes are re-recorded with the
    current renderer, and gain their HTML pages.
  - The HTML page's player stopped silently at five minutes, where GIF and
    MP4 refuse a longer recording. It is refused the same way now, and
    `--no-video` skips it too.
  - A tape whose `Output` shares no format with `--format` wrote only its
    text screenshots without a word; it now warns. "ffmpeg not found:
    skipped …" names the MP4 file the tape's `Output` chose.
- **Interact 0.0.1 and CLI 0.0.13 (`write`, `file`, `color`, `asset`,
  mouse):**
  - `--height` had no upper bound: `rich write --height 100000000` (and
    `color`, `file`) hung and used gigabytes. Every component now draws at
    most the rows that fit on screen.
  - `rich file` read its preview on the UI thread, so a file whose read
    blocks (`/proc/kmsg`, a stalled network mount) froze the picker, Esc and
    Ctrl+C included. Previews now open without blocking, read only regular
    files (checked once open), and read on a thread of their own: after
    250 ms the preview says the read has not finished, and fills in when it
    does, without a key press: the picker ticks until the result is drawn.
  - The line-prompt fallback (`CI=1`, `TERM=dumb`, no full terminal) wrote
    the header and prompt raw, so escape sequences in `--header` reached the
    terminal. Every component's line prompt now shows controls as text, as
    the full-screen view already did.
  - `rich write` kept a piped `\r\n`'s `\r` as an extra line and turned tabs
    into spaces. It now takes `\r\n` as one line ending and keeps tabs.
  - `--char-limit` counted code points and could cut inside an emoji or
    before a combining accent; it counts grapheme clusters
    (`TextArea::truncate`).
  - `rich write --value TEXT` without a terminal answered with nothing; it
    answers `TEXT`.
  - `rich file --extension tar.gz` never matched: the whole suffix is now
    compared.
  - `rich asset --selected NAME` with a name it does not offer is a usage
    error (exit 2), not a misleading "no default value".
  - With `--mouse`, one click on the row that already had focus picked it
    at once; it now focuses, and a second click picks, in `choose`, `file`
    and `color` alike. A click before a directory opened does not count as
    the first of a pair inside it.
  - The help and docs said `rich asset --kind box` names were "for `--box`",
    an option rich does not have; they now say which names `--panel` takes.
  - Debug builds: crossterm's parser panicked on a mouse report at column or
    row 0; the workspace's dev profile turns off its overflow checks.

### Render tree and recordings (0.0.13 stream 11: #226, #598, #600)

- **Core 0.0.9: semantic regions, an opt-in seam (#226).**
  `rich::protocol::RegionSink`, installed on a console with
  `ConsoleRegions::set_region_sink`, is told when a `Panel`, a `Table` and each
  of its header, body and footer cells, a `Rule`, and a Markdown heading or
  code block starts and ends, and those renderables tag the segments they drew
  with a `rich.region` key in the style's metadata. Without a sink (the
  default) nothing is called or tagged, so every golden is unchanged; with one,
  the terminal bytes are still the same (`crates/rich/tests/regions.rs`). See
  DIVERGENCES §36. Core moves from 0.0.8 (published) to **0.0.9** for the new
  public API; the plan had it unchanged.
- **Plugin API 0.0.2, macros 0.0.3, art 0.0.11, Mermaid 0.0.2, lumis 0.0.2:**
  no code change; each depends on `rs-rich` 0.0.9 now. Cargo's `0.0.x`
  requirements are exact, so every published crate that depends on core has to
  move with it.
- **Ext 0.0.11: regions in frames.** `frame::RegionRecorder` records what the
  renderables report; `Frame::from_segments` reads the tags off the styles
  (styles that differ only by a tag still intern as one), and
  `Frame::with_regions` turns them into `Region`s: a role, label, link,
  parent, depth and cell spans per row, with `bounds()`. `Frame::regions()`
  adds a `Link` region per stretch of OSC 8-linked text. `frame::render_frame`
  and `RenderTarget::frame_with_regions` render with a recorder installed.
- **Ext 0.0.11: HTML and SVG from a frame.** `Frame::to_html`,
  `Frame::to_html_with` and `Frame::to_svg` use core's templates and themes:
  without regions they write what `export_html` and `export_svg` write (tested
  on tables, panels, trees, rules, Markdown and upstream's SVG fixture), and
  they keep links (`<a href>`; in SVG an `<a>` around the text). Regions
  become nested `<span class="rich-region …">` wrappers with ARIA roles
  (`heading` with its level, `separator`, `code`, `group` for panels, tables
  and cells), closed at the end of every row so the grid is intact. SVG can
  leave out the window and add a cursor and a caption. The differences from
  core's exporters are DIVERGENCES §37. Golden fixtures:
  `crates/rich-ext/tests/fixtures/frame_export/`.
- **Ext 0.0.11: snapshot schema 3.** `RenderSnapshot::capture_regions` adds
  the frame's regions; `diff` compares them when both snapshots have them, and
  schema 1 and 2 still load. Migration: `RenderSnapshot` has a new `regions`
  field, so a struct literal must add `regions: None`.
- **Record 0.0.1: HTML output (#600).** `rich record --format html` (and in
  `all`) writes `<tape>.html`: the screenshots as selectable HTML text from
  the frame exporter, and a small inline player that swaps the recorded
  screens (each distinct row stored once) with the key overlay. It fetches
  nothing and plays only when asked.
- **Record 0.0.1: presentation.** `Set WindowFrame on|off`,
  `Set Caption "…"` and `Set KeyOverlay on|off` in a tape, and
  `--window-frame`, `--caption` and `--key-overlay` on the command line
  (`record::Presentation`): the window frame around PNG, SVG, GIF and MP4
  frames, a caption bar under them (and under the page's player), and the keys
  in video and the player.
- **Record 0.0.1: `Output`.** `Output gif png` chooses the formats a tape
  writes (intersected with `--format`, so a check run can still skip video),
  and `Output demo.html` names the file of a per-tape format, as in VHS.
- **Record 0.0.1: SVG screenshots through rich-ext.** They are drawn by
  `Frame::to_svg` from the emulator's screen as a frame, replacing the
  recorder's own SVG writer, so they match rich's other SVG output. Every
  tape's SVGs were regenerated and each tape gained its page; the PNGs, GIFs,
  casts and text grids are unchanged, and `rich record --check` passes.
- **CLI 0.0.13: `rich input --required`** refuses an empty answer with an
  error under the line (without a terminal, exit 3).
- **Tapes and docs (#598).** `docs/tapes/input.tape` records `rich input` with
  a placeholder, the `--required` error and an answer. The recordings page
  opens with a gallery: a card per tape whose still plays its GIF while the
  card is hovered or focused, and stays still under reduced motion. The frames
  guide covers regions and export; the render-tree design note marks phase 2
  and the frame export done.

### CLI 0.0.13: rich-cli 1.8.1's rendering options (#542)

- **Source:** `-h/--head LINES` and `-t/--tail LINES` for source files, CSV
  rows and notebook code cells; `-n/--line-numbers`; `-g/--guides`;
  `--lexer LEXER`; `--no-wrap`. Automatic mode renders as syntax when any of
  these is given, as rich-cli renders anything it does not recognise.
  `--tail N` keeps rich-cli's arithmetic and shows the last `N - 1` source
  lines. Given both, `--head` and `--tail` are an error, except for CSV,
  where the head wins, as upstream.
- **Text and output:** `--emoji`, `--soft` (byte-identical to rich 15.0.0's
  `print(soft_wrap=True)`), `-W/--max-width`, `-L/-C/-R/-F` (the `--text-*`
  justifies), `--rule-style`, `--rule-char` and `--force-terminal`.
  `--soft` and `-W` are refused with `--jsonl` and `--log`, which stream
  records without the final print they shape.
- **Short aliases:** `-J` (`--json`), `-u` (`--rule`), `-d` (`--padding`),
  `-a` (`--panel`), `-l/-c/-r` (`--left/--center/--right`) and `-v`
  (`--version`). `-j` and `-x` keep this CLI's meanings (DIVERGENCES §34).
- **Breaking:**
  - `-h` is `--head`, as in rich-cli, so `rich -h 20 file` works as it does
    upstream. Help is `--help` only, in every subcommand.
  - `:emoji:` codes are left as typed unless `--emoji` is given, matching
    rich-cli's `Console(emoji=False)`.
- **Ext:** `cli_doc::ArgSpec` gains `short_aliases` and `short_alias()`, for
  a second short name; help, man pages and completions list it.
- **`--rst`:** renders reStructuredText as rich-cli does (`rich-rst` 1.3.2,
  now pinned in UPSTREAM.toml). A `.rst` file and a `text/x-rst` response are
  detected, and `--lexer` names the lexer for code blocks that name none.
  The port covers the common subset of reStructuredText (DIVERGENCES §35).
- **Ext:** `rich_ext::rst`, a port of `rich-rst`: `RestructuredText` parses
  the document and renders it byte for byte as `rich-rst` does on rich
  15.0.0, checked against fixtures from `scripts/capture_rst_golden.py`.
- **`--rst` fixes (release audit):**
  - Nested lists (`- - - … x`, any marker) overflowed the stack and aborted
    the process. Nesting now stops at 200 levels, past docutils' own limit,
    and deep indentation is bounded by a budget linear in the document's
    size (500 nested quotes took 6.7 s in a debug build, now about 1 s).
  - Unclosed inline runs (`a-a-a-…`, `:a:a…`, `[1 [1 …`) rescanned the
    rest of the paragraph at each word, 19 to 61 s for 64 KB; each is now
    scanned once (under 0.2 s).
  - Grid tables trace their cells as docutils' `GridTableParser` does, so a
    cell spanning columns or rows keeps its text, and a table docutils cannot
    trace renders nothing, as upstream. A simple table with text in a column
    margin renders nothing too.
  - Section titles parse their inline markup (`Title *emph*` shows
    `Title emph`), a block quote keeps what follows its attribution as a new
    quote, and `:role:` before an inline literal is text.
- **Hardening (release audit):**
  - An argument that is not valid UTF-8 was known by its lossy spelling,
    which a real `\u{fffd}` name shares, so `rich $'\xff.txt'
    --export-html \u{fffd}.txt` wrote the export over its input. Such an
    argument is now spelled as batch paths are (`\xNN` after a NUL no
    argument can hold); `--print` text, `--title`, `--caption` and
    `--rule-char` still show it lossily. `--theme-file` reads such a path by
    its bytes, and a batch glob matches file names by their bytes, so
    `a\xff*` no longer matches `a\xfe.txt`.
  - `--dry-run` shows planned paths as errors show them: terminal controls
    visible, and an escaped name as its `\xNN` spelling without the marker.
  - `--sanitize` also covers `--rule-char`.
- **Fixes (release audit):** `-J` and `-u` override a config file's default
  mode as `--json` and `--rule` do, where they reported "only one render
  mode". A panel fits an emoji `--title` without `--emoji`, as upstream
  draws its emoji anyway. `--head`/`--tail` count lines as Python's
  `splitlines` does (a form feed or `\r` ends one).

### CLI 0.0.13: interactive commands (#493, #494)

- **`rich choose`, `rich filter`, `rich input`, `rich confirm` and
  `rich pager`** run the rs-rich-interact components from a shell script.
  The answer goes to stdout and the component paints on stderr, so
  `choice=$(rich choose a b c)` works, and keys come from the terminal even
  when stdin is the list (`ls | rich filter`). Exit codes: 0 answered (or
  yes), 1 cancelled (or no), 130 Ctrl+C, 2 usage, 3 no answer without a
  terminal.
- **`--preview COMMAND`** (#494) for `choose` and `filter` shows a command's
  output beside the list, with `{}` the focused item: `rich {}` renders it.
- Without a terminal they degrade: line prompts on stderr, `--selected` or
  `--default` answers, and `filter` prints the lines matching `--value`.
- Item labels are painted with terminal controls shown as text; the global
  `--no-color`, `--report json` and `--machine-json` apply to every
  interactive command; a `--multi` pick from stdin with nothing marked exits
  3 without a terminal instead of answering with nothing; non-interactive
  `filter` keeps blank lines.
- Behind the new default `interact` feature, which the `rs_rich` wheel's
  `python -m rs_rich` also builds. PTY tests cover the captured answer, keys
  with a piped list, cancel, Ctrl+C, the pager and previews.
- Release-test fixes:
  - a `--preview` command runs on its own thread, so a slow one no longer
    freezes the picker or delays Ctrl+C; it is killed after 5 s ("preview
    timed out") and at most 1 MiB of its output is kept;
  - on Windows, `{}` is no longer escaped for the C runtime, which
    `cmd.exe` ignores (`x" & calc & "` ran `calc`): the command goes to
    `cmd /S /C` as typed, and an item `cmd.exe` cannot quote is not
    previewed;
  - without a terminal, when input ends, `--default` and `--selected`
    answer (`rich confirm --default yes </dev/null` exits 0); with neither
    the command exits 3, not 1, and an empty line in `choose` takes
    `--selected`;
  - invalid UTF-8 on stdin is replaced instead of failing the command, and
    `pager` without a terminal writes a FILE or stdin through byte for
    byte;
  - stdin is bounded: `choose` and `filter` read at most 64 MiB and
    1,000,000 lines, `pager` pages at most 64 MiB, and more is an input
    error (exit 3) instead of `yes | rich choose` exhausting memory;
  - Ctrl+C at `input --password` read without echo exits 130, not 1.
  - a `--preview` command that starts a pipeline or a background process
    (`yes | head`, `sleep 30 & wait`) no longer leaves it running when the
    preview times out or the picker ends: on Unix the command runs in a
    process group of its own and the whole group is killed; on Windows its
    process tree is (`taskkill /T /F`).

### CLI 0.0.13: `rich write`, `file`, `color` and `asset`, and `--mouse` (#493, #476, #491)

- **`rich write`** reads several lines (Enter starts a line, Ctrl+D
  submits) with `--header`, `--placeholder`, `--value`, `--height`,
  `--char-limit` and `--show-line-numbers`. Piped stdin is the text to
  start from; without a terminal it is the answer (at most 64 MiB).
- **`rich file [DIR]`** browses from DIR with a fuzzy filter and a preview,
  and prints the path picked: `--all`, `--directory`, `--file`,
  `--extension`, `--selected` (the answer without a terminal; without it,
  exit 3), `--value`, `--height`. A path that is not UTF-8 is printed byte
  for byte. An action a plugin registered prints what its `run` returns.
- **`rich color`** picks a named colour, a palette colour or one typed as
  hex or RGB, with a live swatch, and prints it with `--format hex|name|rgb`.
- **`rich asset`** picks an emoji, a box style or a spinner
  (`--kind emoji|box|spinner`) with a preview.
- **`--mouse`** for `choose`, `filter`, `confirm`, `file`, `color` and
  `asset`: click to focus and pick, click buttons, drag the border beside a
  preview. Off by default; with it the command paints on the alternate
  screen.
- `color` and `asset` ask line by line without a terminal, like `input`.
  The exit codes are those of the other interactive commands. PTY tests
  cover each command and a mouse click; the CLI reference is regenerated,
  and tapes record each command.

### Interact 0.0.1: text area, file, colour and asset pickers, mouse and actions (#493, #476, #491)

- **New components:** `TextArea` (multi-line; wraps and scrolls; a
  character limit, a placeholder, a configurable submit key), `FilePicker`
  (fuzzy browsing with previews, `..`, hidden files, `FileMode`, extensions,
  a root jail that never follows a link out of the root, names that are not
  UTF-8 shown as `\xNN`, FIFOs never opened for a preview), `ColorPicker`
  (`ColorFormat` hex, name or RGB; the 256 palette as a grid) and
  `AssetPicker` (emoji through core's public `emoji::replace`, box styles,
  spinners). `TableSelect` and `TreeSelect` pick rows and nodes.
- **Mouse (#476), opt-in:** `Component::mouse` (each component's
  `with_mouse`). The event loop gives mouse events in the component's own
  coordinates and turns a click on an OSC 8 hyperlink into `Event::Link`;
  `run` enables mouse reporting for a component that asks. `Select` rows
  click, the border beside a preview drags (at least 12 columns a pane),
  `Confirm` choices and `Form` Submit/Cancel are buttons, and `Pager`
  records clicked links. `headless::Script` gains `click`, `drag`, `scroll`
  and `mouse`.
- **Actions (#491):** `Action::key` is optional (`Action::menu`); `Actions`
  offers actions on every target of a view, filtered by an `ActionTarget`
  (`TargetKind::Item`, `Row`, `Node` or `File`); Ctrl+K opens an action
  menu; `Actions::from_registry` brings in plugins' actions.
- Migration: `Action::key` is now `Option<Key>` (the crate is unpublished).

### Plugin API (unreleased) and Ext 0.0.11: actions for interactive views (#491)

- Additive: `CustomAction` and `PluginRegistrar::action(name, action)`,
  which has a default body, so existing hosts and plugins build unchanged;
  `Capability::Action`. `ExtensionRegistry` hosts actions like the other
  named capabilities and lists them with `actions()`.

### Python 0.0.2: `TextArea`, `FilePicker`, `ColorPicker` and `AssetPicker`

- `rs_rich.interact` exposes the four new components (stubs and docs
  included); `FilePicker` answers with a `pathlib.Path`. `Script` gains
  `click`, `drag` and `scroll`, and a Python component's `Event` has the
  mouse's `column` and `row` and a `"link"` kind. `python -m rs_rich` runs
  `write`, `file`, `color` and `asset`.

### Interact 0.0.1: painting on standard error

- `SessionOptions::output` (`Output::Stdout` or `Output::Stderr`) chooses
  where a session paints, and the terminal is restored on the same stream.
- `Policy::tty_keys` reads keys from the controlling terminal when standard
  input is a pipe, and `Policy::detect_for(output)` decides for a given
  output; `run` uses it. New `Reason`s: `StderrNotTerminal`, `NoTerminal`.

### Interact 0.0.1: release-test fixes

- **Terminal controls never reach the terminal from a view.** The painter
  shows C0 controls, DEL and C1 controls in any view's text as one visible
  character each (`␛`, `␡`, `�`), so a paged file's `ESC c` or 8-bit CSI, a
  preview command's output or a label cannot drive the terminal; styles
  still arrive as styles. Pasted text drops its controls before it reaches
  an `Input` or a picker's query.
- **SIGTERM, SIGHUP and SIGQUIT restore the terminal** (Unix): from the first
  session on, a thread restores raw mode, the alternate screen, mouse and
  bracketed paste (and a masked line read with echo off), then takes the
  signal's default action. New direct dependency: `signal-hook`, which
  crossterm already used.
- **The pager's search no longer panics** when lower-casing changes a
  character's length (`ẞ`, `Ⱥ`): it folds case one character at a time and
  maps matches back to the original, and lines holding `İ` are searched.
- **The fuzzy matcher is bounded.** A candidate that does not hold the
  pattern as a subsequence is rejected in one pass before anything is
  allocated, and a problem over 2^20 cells (a 2000-character query against
  a 300,000-character line) is matched greedily instead of allocating
  gigabytes. Scores for ordinary sizes are unchanged.
- **End of input is no answer.** The line prompts of `Input`, `Confirm`,
  `Select`, `MultiSelect` and `Form` answer with their default at the end of
  input; without one, `run` fails with `NotInteractive::NoDefault` instead
  of reporting `Cancelled`. New `NotInteractive::Ended` and
  `NotInteractive::Interrupted`; `LineIo::read_secret` returns
  `Result<Option<String>, NotInteractive>`, so Ctrl+C at a line read with
  echo off is `Outcome::Interrupted`.
- **`Input` edits by grapheme** (an accent moves and deletes with its
  letter) and scrolls a line longer than the terminal to keep the caret in
  view. `Select::repaint_every` (and `MultiSelect`'s) repaints on a timer,
  for previews that fill in by themselves.
- **The caret follows shown controls.** A view's cursor column is moved by
  the cells `paint::sanitize_line` gives the controls before it, so an `Input`
  whose prompt holds ESC or a tab (`rich input --prompt $'x\e'`) puts the
  caret after the field, not inside the prompt. New
  `paint::sanitized_column`.

### Python 0.0.2: interactive components and frames (0.0.13 workstream 5)

The `rs-rich` package on PyPI stays level with the Rust crates.

- **`rs_rich.interact`** binds rs-rich-interact: `Select` and
  `MultiSelect` over `Item`s (value, label, description, metadata,
  keywords, a markup or renderable preview, key `Action`s), `Input` (with
  `password=True`, validation, history and suggestions), `Confirm` with
  `Choice`s, `Form` (text, masked, choice and toggle fields) and `Pager`.
  An item's value is any Python object, returned as it is. Any object with
  `handle(event)` and `render(width, height)` is a component too.
- **Drivers.** `component.ask()` runs on the terminal (GIL released) and
  returns the answer; Escape raises `Cancelled` and Ctrl+C
  `KeyboardInterrupt`. `run` returns an `Outcome` (`done`, `cancelled`,
  `interrupted`) instead. Without a terminal `fallback` asks line by line,
  returns the default, or raises `NotInteractive` (with its `reason`).
  `headless(component, keys)` returns a `Record` of the painted frames and
  bytes; `degrade(component, answers)` runs the line-prompt fallback. `fuzzy`
  and `rank` are the pickers' matcher.
- **Frames.** `rs_rich.ext.frame.Frame` (`rich_ext::frame`): from segments,
  from a `Console`'s render, or `RenderTarget.frame`; rows, cells, plain
  text, `to_ansi` (the console's bytes), merged encoding, `encode_span` and
  a cell-level `diff`.
- **The new CLI commands** run through the wheel: `python -m rs_rich
  choose|filter|input|confirm|pager`, with tests for their piped
  degradation and exit codes.
- Type stubs, an [interactive components page](docs/python/interact.md),
  frames in the terminals guide, and tests that compare with the Rust
  crate's frames. Not yet from Python: component themes, `Input`'s
  background suggestion provider, and the multi-component event loop.
- **Exceptions end the run.** An exception from a validator (bar its
  `ValueError` message, and including `KeyboardInterrupt`/`SystemExit`), a
  preview or confirmation body, or a Python component now ends the run at
  once and is raised from it; before, it showed as a message and the run
  went on until the user cancelled.
- **Nested runs raise `RecursionError`** instead of crashing: a validator
  or component that starts another run nests on the native stack, and a
  run is refused while too little of the thread's stack is left (or 200
  deep).
- `Iterable` arguments take any iterable (generators, sets): `degrade`'s
  answers, `MultiSelect(marked=)`, `Input(history=)`, `Item(keywords=,
  actions=)` and `Confirm(warnings=)`. `NotInteractive(...).reason` is
  `None` when raised by hand. `run(height=0)` raises `ValueError`.
  `fuzzy` and `rank` release the GIL while they match.

### Hardening carried over from 0.0.12 (0.0.13 workstream 6)

- **CLI: non-UTF-8 file names.** A batch glob or directory walk found such a
  file under its lossy spelling, which named no file, so the batch failed,
  and an output named after it was refused. They now keep their bytes
  through globbing, walks, planned outputs (spelled with `\xNN` in messages),
  existence and collision checks, export writes and the workers' command
  lines; `bench compare` reads its runs the same way. Unix tests cover a
  glob with `--jobs 1` and `--jobs 3` and a walk. The escaped spelling
  starts with a NUL, which no path or argument can hold, so it never names
  another file (a valid name spelled `na\xFFme.txt` stays its own), and a
  glob in a non-UTF-8 directory and a `--dry-run` into one work too.
- **Python: nesting depth as Rich's.** Renderables nested past 100 raised
  `RecursionError`, and a `Pretty` in panels stopped 24 levels before Rich.
  The limit now follows Rich's Python frames: 8 per nested renderable, from
  what is left of the recursion limit where the caller prints, with a
  `Pretty` charging what Rich's takes for its data's depth, per CPython
  version (3.12 inlines more of Rich's walk). Panels around a `str`, `Text`
  or `Pretty` stop at exactly Rich's depth on Python 3.9 to 3.13, from any
  call depth (tests compare with rich 15.0.0). The depth estimate walks
  each container once, so a list holding itself costs one pass.
- **Python: no crash on a small thread stack.** Core renders each level on
  the calling thread's native stack, so 100 nested panels crashed the
  interpreter on a 1 MiB thread. Nesting now raises `RecursionError` where the
  stack left (read with `stacker`, which never switches stacks) would not
  hold the render.
- **Art: tall images** keep following `options.height` only; the image guide
  now says so and why.
- **Audit:** `.cargo/audit.toml` records the two informational advisories:
  `bincode` 1 (unmaintained; syntect 5.3.0, the latest, still uses it) and
  `ttf-parser` (unmaintained; used by rs-rich-record through fontdue).
  `cargo audit` reports nothing else.

### Third-party plugins: plugin API 0.0.2, ext 0.0.11, CLI 0.0.13 (0.0.13 workstream 9: #14, #232)

Why it works this way: [Plugin loading](docs/design/plugin-loading.md). What
a plugin may do, and the threat model: [the plugin guide](docs/PLUGINS.md).

- **Mermaid 0.0.2 and lumis 0.0.2.** Only their manifests change: they now
  require `rs-rich-plugin-api` 0.0.2. Published crates are immutable, so both
  bump and are republished after the plugin API.

- **Plugin API 0.0.2.**
  - `export_plugin!(MyPlugin)` registers a plugin for link-time collection
    (through `inventory`, a new dependency with no proc macros);
    `linked_plugins()` lists them.
  - `abi`: a versioned, text-only ABI (1.0) for runtime plugins. Native
    plugins export one C entry point with a `repr(C)` descriptor and vtable,
    written by `export_dylib_plugin!` so the plugin has no unsafe code; WASM
    plugins export four functions and a text manifest. Both decode into one
    `PluginAbi`. They may contribute transforms, highlighters, and Markdown
    fence renderers that return markup or ANSI. A host refuses another ABI
    major before reading the rest of a descriptor, and an unknown capability
    kind.
  - An example native plugin (`examples/dylib-plugin`) and WASM module
    (`examples/wasm/shout.wat`).
  - `PLUGIN_API_VERSION` stays 1: nothing existing changed.
- **Ext 0.0.11.**
  - `ExtensionRegistry::with_linked_plugins()`, `add_linked_plugins()` and
    `add_plugin_set()` add plugins sorted by id; a duplicate id is an error and
    adds nothing.
  - `plugin_loading`: `load(path)` picks the loader by extension and returns a
    `RuntimePlugin`, an ordinary `Plugin`, so `add_plugin` checks it like any
    other. Every output is sanitized: controls made visible, and only
    `fence-ansi` keeps SGR styling, through `sanitize_ansi_for_decoder`.
  - New features, both off by default: `dylib-plugins` (`libloading`; the
    library stays loaded while anything registered from it exists) and
    `wasm-plugins` (`wasmi`, a pure-Rust interpreter: no imports allowed, a
    fuel limit per call, a memory cap, a fresh instance per call).
  - `sanitize::sanitize_ansi_for_decoder`, moved from the CLI so the CLI's
    viewers and runtime plugins share one sanitizer. The CLI's behaviour is
    unchanged.
- **CLI 0.0.13.**
  - `rich plugins list` and `rich plugins info NAME` show every plugin with
    its source (`built-in`, `linked`, `native`, `wasm`), version, ABI and
    capabilities; `--report json` writes them to stdout.
  - `--plugin PATH` (repeatable) and a `plugins = [...]` config key load
    runtime plugins, in a build with the new `dylib-plugins` or `wasm-plugins`
    feature (both off by default). A project's `./rich.toml` may not list
    plugins: the list is ignored with a warning, as `mermaid_backend = "mmdc"`
    is. A failed load exits 3 (unreadable) or 2 (anything else) with a message
    naming the file, and never panics.
  - Loaded plugins draw their Markdown fences and highlight printed text, and
    linked plugins join every registry the binary builds.
  - `--transform NAME` (repeatable) applies registered text transforms, a
    plugin's or a built-in one, to text, `--print` and `--syntax`, in the
    order given, after `--filter` and before `--highlight`. An unknown name is
    a usage error (exit 2) listing the available names; a failed transform
    reads `--transform NAME failed: …` (exit 4), as `--filter` does.
    `--inspect` refuses it.
- **Release tooling.** `validate_release.py` runs the loader tests with both
  features on, and the CI MSRV check builds the CLI with them.

### Docs and tooling

- **Tapes (#598).** A first, Python runner (`scripts/tape.py`, since
  replaced by `rich record`) ran a scripted terminal session
  (`docs/tapes/*.tape`: type, press keys, wait for screen text, take
  screenshots) against the built `rich` on a real PTY. From one run it writes
  PNG, SVG and text screenshots, an asciinema cast, a GIF with a key overlay,
  and an MP4 when FFmpeg is installed. A new CI job, "docs tapes up to date",
  re-runs every tape and fails when a screenshot's text changes.
- **Terminal recordings page.** `docs/recordings.md` plays the casts with a
  vendored asciinema-player 3.17.0 (Apache-2.0), and the README opens with the
  hero recording. The first tapes cover rendering Markdown, CSV and code,
  `--watch` re-rendering on save, and `view --pager` scrolling and searching.

### Interact 0.0.1: the first components (0.0.13 workstream 3: #287, #288, #289, #291, #454, #457, #470, #472)

- **`Select` and `MultiSelect`** (#287) filter items with a new fuzzy matcher
  (`rich_interact::fuzzy`):
  - it prefers word starts and runs, uses smart case, and needs every
    space-separated term to match;
  - matched characters are highlighted;
  - `MultiSelect` marks with Tab and Ctrl+A;
  - item actions pick by key.

  A preview pane (#454) shows the focused item's preview beside the list, or
  below it on a narrow terminal.
- **`Input`** (#457, #289) edits like a shell line and offers:
  - a placeholder, a default and masking (`Input::masked`, for passwords
    and tokens). A masked input's
    default is shown only as `(default set)`, on screen and in the line
    prompt, never as its value. In the line prompt it reads with echo off
    when stdin is a terminal (`LineIo::read_secret`);
  - a validator whose message shows under the line;
  - history;
  - suggestions, fixed or from a provider. The provider runs on one
    background thread per input, one lookup at a time, always for the
    latest text; stale results are dropped.
- **`Confirm`** (#470) is a confirmation sheet: a scrollable body of any
  renderables, warnings, and any number of keyed choices.
- **`Form`** (#288, #472) has text, masked (`Form::masked`), choice and
  toggle fields. Tab and the arrows move between fields, and each validation
  error shows under its field. A text field with suggestions shows them under it, and Tab and
  the arrows pick one there. A form with no fields finishes at once. It
  returns `Answers` by field name.
- **`Pager`** (#291) pages any renderable at the terminal's width. `/`
  searches, matches are marked, and `n`/`N` jump between them. A search given
  up front opens on its first match.
- **`Component::start`** runs once before a component's first paint, with
  the terminal's size. It can finish the component without waiting for a
  key. The trait's default does nothing.
- **Finished components collapse** to a one-line answer
  (`? Open › src/main.rs`), and the painter clears what a taller view left
  below. Lines wider than the terminal are cropped instead of wrapping.
- **Tests and recordings:**
  - every component has headless tests, a PTY test and a line-based fallback;
  - `--example components` runs each one;
  - the `components` tape records them all in a fixture project
    (`docs/tapes/fixtures/project`), which the viewport tape now pages too,
    so the recordings do not change when the crate's own files do.

### Interact 0.0.1 (new crate, unreleased): interactive components (0.0.13 workstream 2: #451, #452, #489, #492, #495)

- **`rs-rich-interact`** (`rich_interact`) is the layer between printing and a
  full TUI. A `Component` is a state machine: it handles one `Event` and
  renders one `View`, and returns `Flow::Continue`, `Done`, `Cancel` or
  `Handoff(command)`.
- **Two drivers:** `run` takes the terminal, drives one component to an
  `Outcome` and gives the terminal back; `EventLoop` runs several, with ticks
  and timers. `run` is the loop with one component, so the two cannot differ.
- **Repaint on change.** Views are painted through `Frame::diff` from
  workstream 1, cell by cell: an idle loop writes nothing.
- **The terminal is always given back.** The `Session` restores raw mode, the
  alternate screen, mouse and bracketed paste on every way out: finishing,
  `?`, Ctrl+C and a panic (#489). `Flow::Handoff` lends the terminal to
  `$EDITOR` or a pager and takes it back. PTY tests check each path with
  `stty -a`.
- **`Viewport`** (#495) is a scrollable window over rendered lines, and a
  minimal pager on its own (`--example viewport`).
- **`Item<T>`** (#452) is the one item model behind every picker: label,
  description, metadata, preview, key-bound actions and search keywords.
- **The degradation policy** (#492). With no terminal on stdin or stdout,
  under `CI`, with `TERM=dumb`, or when the caller asks, no session starts.
  The component then asks line by line, returns its default, or fails, as the
  caller chooses; nothing blocks on a pipe.
- **The headless driver** (scripted events, a virtual clock, every paint
  recorded as bytes and text) tests all of it without a terminal.
- A new guide page, "Interactive (rs-rich-interact)". Release tooling, the
  release workflow's tag list, the CI feature matrix and the version tables
  include the crate; it is uploaded by hand as 0.0.1 after `rs-rich-ext`
  0.0.11.

### Record 0.0.1 (new crate, unreleased) and CLI 0.0.13: `rich record` (#599)

- **`rs-rich-record`** (`rich_record`) records tapes from Rust: `tape::parse`,
  `record::record` (bash on a PTY through `portable-pty`, followed by the
  `vt100` emulator), then `record::write` or `record::check`. Screenshots go
  through `rich_ext::frame::Frame` for their text grids. PNG and GIF are drawn
  with an embedded DejaVu Sans Mono (Bitstream Vera licence, shipped as
  `fonts/LICENSE-DejaVu`). Emoji are drawn in colour from an embedded Twemoji
  (Twemoji Mozilla 0.7.0, CC BY 4.0, shipped as `fonts/LICENSE-Twemoji`),
  with ZWJ sequences, skin tones, flags and keycaps through the font's
  ligatures. The emulator gives each grapheme cluster one cell as wide as
  `rich::cells::cell_len` measures it: vt100 alone split 👩‍👧 over four
  cells, left ❤️ and 1️⃣ one cell wide and a flag's regional indicators
  apart, and shifted the rest of the row. Box-drawing characters are drawn as lines, so borders
  join between rows, and block elements, quadrants and braille as shapes, so
  half-block and quadrant images have no seams. `Set Shell` runs a tape in
  `bash` (the default), `zsh`, `fish` or `sh`, each without the user's
  profile or rc files and with the same prompt, and `rich record` warns when
  the `bash` on `PATH` is older than 4 (macOS ships 3.2), whose line editing
  differs from CI's. The recordings page says why a tape should not
  line-edit across a joined emoji: shells take widths from the C library,
  which counts 👩‍👧 as four cells. Linux and macOS are supported; Windows builds
  through ConPTY and is experimental.
- **`rich record TAPE…`** in the CLI (the new default `record` feature):
  `--check`, `--output`, `--format png,svg,cast,gif,mp4`, `--no-video`,
  `--bin-dir` and `--font`. It reproduced every screenshot the Python runner
  had committed, so the "docs tapes up to date" CI job now runs
  `rich record --check`, and `scripts/tape.py` is removed.
- **Every terminal recording is now made in Rust.** The guided tour is a tape
  (`docs/tapes/tour.tape`), and the README and demos page show its recording.
  The Python capture scripts are removed: `capture_demo_tour.py`,
  `capture_release_demos.py`, `capture_v8_demos.py`, `capture_v9_demos.py`,
  `capture_expanded_v9.py`, `capture_image_modes_010.py`,
  `build_docs_media.py` and the `terminal_shots` kit. The media they made is
  kept, with pointers to where they last existed. `capture_golden.py` stays: it
  runs upstream Python `rich` for the parity goldens.
- **Faster GIFs.** One palette per recording, frames rendered in parallel, and
  fast paths for fills. Two tapes take 0.63 s of CPU in release (was 1.83 s)
  and 2.1 s in debug (was about 66 s). The recorder's image crates are
  optimised in dev builds too.
- **`Mask /regex/ "text"`** hides output that varies between runs (temporary
  paths, timings) from the text grids `--check` compares; images keep what was
  shown. **`Wait`** now also matches text shown since the previous step, so
  fast output that scrolls past between polls is not missed.
- **Hardening (tapes are still trusted code: `Exec` and the shell run what
  the tape says).**
  - A tape whose name would leave `--output` (`...tape` names `..`) is refused
    before any tape runs, by `rich record` and by `record::record` and
    `record::write` (`record::stem_allowed`). `record::write` now removes only
    the stale screenshots its last write listed in `provenance.json` (its new
    `screenshots` field), never other files in the directory; `--check`
    reports orphans the same way, so a directory written before this change
    reports none until it is written again.
  - Tape limits, as parse errors: `Sleep`, `Wait` and `Timeout` at most 1 hour
    (`1e300s` panicked); `Set Size` and `Resize` from 2x2 to 500x200 (0x0
    and 65535x65535 were accepted); `Write` paths relative and inside the
    workspace. Images that would pass 100 million pixels (a large `--font`)
    are refused before anything is written.
  - Output is kept at most 12 times a second (the video's frame rate): a burst
    is one event and one frame, and a burst over 32 KiB is recorded as a
    repaint of the screen. `seq 1 300000` no longer keeps a frame per read.
    Video covers the first 5 minutes; asking for a GIF or MP4 of a longer
    recording is an error that says to use `--no-video`. The screens kept for
    `Wait` are capped at 16 MiB.
  - GIF and MP4 are drawn a batch at a time and streamed: the GIF's palette
    is chosen on a first pass and frames are encoded on a second, so memory
    no longer grows with the recording's length. A GIF wider or taller than
    65535 pixels is an error, not a truncated size. FFmpeg is killed and
    reaped when writing to it fails, and when a `Session` is dropped on an
    error path its shell is killed and reaped.
  - A panic in the `vt100` emulator fails the tape with an error instead of
    poisoning the session; a flood of combining marks takes linear time; an
    escape with intermediate bytes (`ESC ( B`) no longer prints its final byte.
  - `Exec` reads its error output as it runs (a command that wrote more than a
    pipe holds hung until the timeout), keeps the first 64 KiB for the report,
    and is reaped after a timeout.
  - Output still queued when a `Resize` runs is recorded, with its frame, at
    the old size before the resize event, instead of being drawn on the
    resized screen after it.
  - SVG text replaces characters XML forbids (control characters in a title
    or on screen) with U+FFFD, so the file stays well-formed.
  - Migration: `Timeline` has a new `truncated` field; `Terminal::process` and
    `set_size` return `io::Result`; `video::sample` returns references into
    the timeline (`VideoFrame::snapshot`), `video::render` takes the timeline,
    and `raster::gif` returns `io::Result`.

### Ext (0.0.11, unreleased)

- **Frames (#226).** `rich_ext::frame::Frame` stores a render as rows of styled
  runs with interned styles. `Frame::to_ansi` writes the same bytes as
  `Console::segments_to_string` for the same control-free segments; this is
  tested on random segment streams, core renderables in every colour system,
  and every golden fixture. `to_ansi_merged` is a smaller, opt-in encoding.
  `cells`, `diff` and `encode_span` give a cell-level view and diff.
  `RenderTarget::frame` renders straight to a frame.
- **`LiveCoordinator` repaints changed cells**, not whole rows, when that
  writes fewer bytes. A one-cell change in a table row wrote 45 bytes before
  and 27 now. What ends up on screen is unchanged.
  A row holding characters terminals may draw at a different width than rich
  measures (emoji and text presentation selectors, ZWJ sequences, flags,
  skin tones, keycaps, tag sequences, East Asian ambiguous-width characters
  other than box drawing and blocks) is still repainted whole, since a column
  move could land in the wrong cell. Adds a `unicode-width` dependency.
- **Snapshot schema 2.** `RenderSnapshot::capture_frame` stores rows of merged
  runs, so output that is only split differently compares equal. `diff`
  compares schema 1 and schema 2 snapshots by what shows, and `upgrade`
  converts schema 1. `capture` and schema 1 JSON are unchanged.
- Migration: `RenderSnapshot` has a new `rows` field, so code that builds one
  with a struct literal must add `rows: None`.

## Core 0.0.5 / ext 0.0.7 / art 0.0.7 / CLI 0.0.9 — published 2026-09-22

- Core 0.0.5: optional immutable rendering-environment extension seam; unchanged
  default parity and public ConsoleOptions/Renderable requirements.
- Ext 0.0.7: explicit targets/capabilities, deterministic optional snapshots,
  constrained layouts/overflow, typed events/diagnostics, optional log/tracing
  adapters and single-writer coordinated Live regions.
- Art 0.0.7: still-image rotation, flips, grayscale and ordered Bayer dithering;
  exact Braille/half-block edge regressions and same-source output examples.
- CLI 0.0.9: directory-preserving/template batch names, config/worker routing,
  typed log presentation, still-image exports, updated guided demo and real media.
- Migration: exhaustive matches must include `Dither::Bayer4x4` and the added
  unsigned/128-bit `Value` variants. New batch naming modes take output directories.
- Fixed before merge: Live `print` no longer drops ordinary writes at interactive
  widths 0/1, and diagnostic snippets strip CRLF carriage returns.
- Release test passed on 2026-09-22 (full validation, golden parity, per-tag plans,
  packaged consumer install, installed-binary screenshots); see
  [expanded notes](docs/releases/0.0.9-expanded.md#release-test-2026-09-22).
- Published from `main` at `c645220` in dependency order by the protected release
  workflow: [`rs-rich-v0.0.5`](https://github.com/buchochelliq-labs/rs-rich-cli/actions/runs/35789282880), [`rs-rich-ext-v0.0.7`](https://github.com/buchochelliq-labs/rs-rich-cli/actions/runs/35792283558), [`rs-rich-art-v0.0.7`](https://github.com/buchochelliq-labs/rs-rich-cli/actions/runs/35792299240), [`rs-rich-cli-v0.0.9`](https://github.com/buchochelliq-labs/rs-rich-cli/actions/runs/35792320939). The core
  upload first failed because crates.io required Trusted Publishing; it succeeded
  after the crate setting was adjusted. The workflow still uses a stored token
  (tracked for 0.0.10).

## [0.0.1] — first release

The first published version of all four crates: `rs-rich`, `rs-rich-ext`,
`rs-rich-cli`, `rs-rich-art`.

**Read the version number literally.** `0.0.1` is not modesty — the API takes
breaking changes regularly (three in the week before this release), and the port
is deliberately incomplete. What is implemented is byte-parity tested against
real Python `rich` 15.0.0; what isn't is listed in the README and
`docs/PORTING.md`, and what deliberately differs is in `docs/DIVERGENCES.md`.

Two things worth knowing up front:

- **Package names carry an `rs-` prefix** because `rich` is taken on crates.io.
  The library targets keep the short names, so you write `use rich::…`, and the
  CLI's binary is still `rich`.
- **Crate versions are independent SemVer and do not mirror the upstream release.**
  Which upstream version is tracked lives in `UPSTREAM.toml`. An earlier policy
  mirrored the number; it was dropped before release because publishing a young
  API as `15.0.0` would have been a lie, and the first breaking change would have
  collided with upstream's next major.

Entries below record subsequent releases and development.

## Core 0.0.8 / plugin API 0.0.1 / macros 0.0.2 / ext 0.0.10 / art 0.0.10 / Mermaid 0.0.1 / lumis 0.0.1 / CLI 0.0.12 / Python 0.0.1 — published 2026-09-26

Cohort versions for 0.0.12: core 0.0.8, plugin API 0.0.1 (new), macros 0.0.2,
ext 0.0.10, art 0.0.10, Mermaid 0.0.1 (new), lumis 0.0.1 (new), CLI 0.0.12, and
the first `rs-rich` Python package, 0.0.1, on PyPI. Core changes below, so
every dependent moves with it. See the [0.0.12 plan](docs/plans/0.0.12.md) and
the [release notes](docs/releases/0.0.12.md).

### Release test: second audit round (Python package and new core)

Four more audits covered the Python package end to end (console and threads,
live displays, renderables by differential fuzzing against Rich 15.0.0, and
the ext/art/plugin/CLI surface). Every finding has a regression test:
`crates/rich-py/tests/test_robustness_*.py`, `crates/rich/tests/robustness_audit.rs`
and the goldens `audit_edges.tsv` (10 cases) and `audit2.tsv` (12 cases),
captured from rich 15.0.0.

Core (`rs-rich`):
- Text renders by one sweep over span events, as upstream: printing
  `list(range(20000))` is linear (it took minutes). Output is unchanged.
- `Segment::crop_lines` ports `split_and_crop_lines`: a newline inside a
  segment (a print `end` like `"!!\n"`) survives the crop, and zero-width
  characters past the edge are dropped.
- Markdown paragraphs and images inherit overflow, no-wrap and justify; an
  invalid GFM delimiter row is a paragraph; text with many brackets parses in
  linear time.
- `Console::capture` is per thread, as upstream, and ends when the captured
  code panics.
- Layout `size=0` is flexible; `ratio_resolve` uses exact integer arithmetic
  (no dropped rows); huge sizes and screen coordinates no longer overflow.
- Syntax accepts any `i64` line numbers and ranges without panicking.
- Colour downgrade to `windows` uses the Windows palette; bare `grey`/`gray`
  are not colour names.
- `Tree("")`, `Syntax("")` and an unpadded empty `Text` print their blank line;
  measuring splits on Python's line and word separators (U+2028 and friends).
- Markup error positions count characters of the original string; the console
  replaces emoji codes only between tags (`markup::render_emoji`).
- New `Text::set_justify_option`/`get_justify_option`: an explicit
  `justify="default"` is not overridden by a table column.
- `render_lines` pads with no style; `Panel.width(0)` draws Rich's empty box;
  SVG export rejects a non-finite `font_aspect_ratio` and formats numbers as
  Python does; Json accepts lone surrogates (DIVERGENCES §30); `Columns` with
  zero width and padding renders nothing instead of panicking (§32).

Ext, art and CLI:
- rs-rich-art: image backends cap their grid at `MAX_CELLS` (262,144 cells)
  before resampling, and derived rows respect a fixed `options.height`: a tiny,
  very tall image no longer hangs or aborts. **Breaking:** new
  `ImageArtError::TooLarge` (and `check_cell_budget`) for an explicit size over
  the budget (CLI exit 3).
- rs-rich-ext: `CancelToken` shares its ancestors, so deep task trees build in
  linear time; `TaskTree` state, elapsed time and rendering no longer recurse (a
  10,000-deep tree renders).
- rs-rich-cli: arguments that are not valid Unicode no longer panic; files named
  by them open by their original bytes, as upstream rich-cli.

Python package (`rs-rich` on PyPI):
- No lock is held while Python code runs (getters, replacing `file` or
  `highlighter`, the plugin table), so a `__del__` that prints cannot deadlock.
- A print re-entered from what it prints (`__str__`, a highlighter, a fence, a
  hook) raises `RecursionError` instead of crashing; deeply nested objects
  pretty-print as in Rich, with Rich's `<repr-error ...>` where the recursion
  limit runs out.
- Leaving a `Live`, `Progress`, `track` or status just before the interpreter
  exits no longer crashes it: refresh threads are joined.
- Render hook items print on any thread (those from `log` or a justified
  `print` on the collecting thread only); Ctrl-C works while waiting for
  another thread's print; output under a `Live` goes past its file proxy, and a
  console with no file writes to a null file, as Rich.
- `markup=False` applies to strings inside every container; bad markup raises
  `MarkupError` when printed (Table cells and titles, Panel titles included),
  with Rich's message and position.
- Huge sizes (console height and `tab_size`, `ConsoleOptions`, `Text.pad` and
  friends, `Syntax`, `Panel`/`Align` height, `Table(leading=)`, ...) raise
  `MemoryError`/`OverflowError` instead of aborting or eating all memory.
- Progress columns handle integers past 2**63 and huge floats as Rich;
  `Task.percentage` clamps -0 and NaN to 0; `RichHandler` builds its path link
  as Rich (no link for a path Rich cannot parse).
- `Table` and `Panel` expose Rich's attributes (`table.show_header = False`).
- `Console.rule` takes a `Text` title and rejects empty `characters`; a hook
  returning a non-renderable raises `NotRenderableError`; `print(width=-1)`
  prints nothing; `Text.end` is kept inside `Group`, `Styled` and `Constrain`;
  `Spinner` text expands emoji; `Columns(width=0, padding=0)` raises
  `ZeroDivisionError`.
- `Text.stylize`, `tokenize`, the diffs and the `find` helpers no longer do
  work proportional to the text length per call; `DataNode.from_python` caps
  embedded nodes at 512 levels.

### Release test: fixes from five independent audits

Five audits covered the whole 0.0.12 delta: core, the plugin API and ext host,
Mermaid, art and CLI flags, and the Python bindings. Every finding below was
reproduced by a test that failed first.

- **Core (parity).**
  - `Syntax` turns a lone `\r` into a line break, as Pygments does for every
    lexer. It used to join the two lines, and `Panel.fit` then cropped the
    second one away. Markdown fences behave the same.
  - A `Panel` in no width renders nothing, as upstream does, instead of two
    empty lines.
  - Table ratio arithmetic runs in `i128`, so a huge column `ratio` no longer
    overflows. The output matches rich 15.0.0's.
  - With a console-wide default code highlighter, inline code in Markdown
    uses it too, not just code blocks.
  - `Text::base_style()` exposes the base style (upstream's `Text.style`).
- **Plugin API and host.**
  - A plugin that panics in `metadata` or `register` is refused with
    `PluginError::Failed`, and the registry is left unchanged. It used to take
    the host down.
  - Plugin and capability names must start with a letter or digit, so `-x`,
    `--help`, `.` and `..` are refused. This narrows the contract before the
    crate's first release.
  - `provided_by(&Capability::Highlighter)` names the plugin that registered
    the highlighter, `"(direct)"` for one registered without a plugin, and
    `None` when there is none.
- **Transforms.** `HighlightMatches` styles whole matches only: a named group
  is no longer used as a style name (`(?P<blink>…)` added blink). It also
  reports the regex engine's backtrack limit as an error, as `KeepLines` does.
  The pattern is compiled once.
- **lumis.** Pygments names that lumis lacks map to the language that
  highlights them: `shell`, `sh`, `zsh`, `console`, `python3`, `golang`,
  `patch`, `jsonc` and more. These used to come out as plain text.
- **Mermaid.**
  - A link's extra dashes count for at most 10 ranks (`MAX_LINK_LENGTH`).
  - A flowchart whose layout needs more than 5,000 points is refused at once.
    An input under 1 KiB used to take four minutes. Crossings are now counted
    in O(E log V), with unchanged output.
  - `mmdc` stays in the caller's process group, so Ctrl-C stops it and
    Chromium. A timeout sends SIGTERM, then SIGKILL after 2 s. On Unix the
    temporary directory is removed even if `rich` is killed.
  - `MmdcOptions::max_output` (16 MiB) caps the image read into memory
    (`MmdcError::OutputTooLarge`). Only the first 4 KiB of the log is read.
- **CLI.**
  - `rich doctor` reports an unknown configured highlighter or code theme
    instead of failing. It is where the error message sends you.
  - `rich config validate` rejects an unknown highlighter or code theme,
    which every render would reject.
  - `--filter` and `--highlight` keep `--syntax`'s full-width background rows,
    so a pattern that matches nothing leaves the output byte-identical.
  - `mermaid_backend = "mmdc"` in a user config, on a build without mmdc,
    warns and draws Mermaid as text. It used to make every command exit 2.
    On the command line it is still an error.
- **Python bindings (`rs-rich` on PyPI).**
  - `Console`, `Table` and `Panel` work from any thread, including
    `rs_rich.print` from a worker thread. Concurrent prints to one console
    don't interleave. Printing from inside the console's own `file.write`
    raises `RuntimeError`.
  - `print` and `rule` flush the file, as Rich does.
  - Panels nested more than 100 deep raise `RecursionError` instead of
    crashing the interpreter.
  - Sizes are limited. `Console(width=…)` and a column's `width`,
    `min_width` and `max_width` go up to 65536, and `ratio` up to
    4294967295. A `Panel`'s padding goes up to 65536 on each side, where Rich
    has no limit. Larger values raise `ValueError` instead of aborting,
    hanging or taking minutes to print.
  - `t.append(t)` works; `Text.stylize` clamps offsets of any size; `Style`
    is hashable; reference cycles through a panel's child or `Console.file`
    are collected.
- **Workflows.**
  - `python.yml` also runs when the root `Cargo.toml` or `Cargo.lock` changes.
  - `pypi-release.yml` runs the whole test suite against the Linux wheel
    before publishing.
  - Third-party actions (`maturin-action`, `gh-action-pypi-publish`) are
    pinned by commit SHA.
- **Release material.** `rich --demo` has sections for Mermaid, code themes,
  filter and highlight, and native image size. The CLI README covers the
  0.0.12 flags and features. `make_cases.py --release 0.0.12` builds the
  screenshot set.

### Pluggable code highlighters, core (0.0.12 workstream 1: #522, #523)

- **`CodeHighlighter` (#522).** A new extension point in `rich::protocol`: an
  engine takes source, a language and a theme, and returns styled spans per
  line (`HighlightedCode`, `HighlightedLine`, `HighlightSpan`, `HighlightError`).
  `Syntax::highlighter` and `Markdown::highlighter` take any engine; Markdown
  passes it to code blocks and to lexer-highlighted inline code.
  - Core validates what an engine returns against the source. It drops
    out-of-range, overlapping or misaligned spans and hyperlinks in span styles,
    renders missing lines unstyled, and falls back to the default theme for an
    unknown theme and to plain text if the engine fails. The rendered characters
    always come from the source, so an engine cannot add text or control
    sequences.
- **`SyntectHighlighter` (#523)** is the default and holds the whole `syntect`
  dependency. Output is byte-identical to 0.0.11: all 558 source, docs and data
  files in the repository render the same through `rich FILE` (default and
  `onig` builds, and with `syntax-cache`), and 150 through `rich view`.
- **ANSI themes (#523).** `ansi_dark` and `ansi_light` use upstream's
  `ANSI_DARK`/`ANSI_LIGHT` styles with the terminal's 16 colours and no
  background. Pygments token types map to TextMate scopes (DIVERGENCES #18).
- **Migration.** `Syntax::theme` names are now the highlighter's own; with the
  default nothing changes, and `ansi_dark`/`ansi_light` are new.

### Plugin API crate and ext host (0.0.12 workstream 2)

- **New crate `rs-rich-plugin-api` 0.0.1** (`rich_plugin_api`, also
  `rich_ext::plugin`). It defines the plugin contract and depends on core only,
  so a plugin never depends on `rs-rich-ext`. A plugin implements `Plugin`
  (`metadata()` plus `register()`) and adds regex highlighters, named code
  highlighters, themes, box styles and source renderers through a
  `PluginRegistrar`. `PLUGIN_API_VERSION` is 1, and every breaking change bumps it.
- **Ext host.** `ExtensionRegistry::add_plugin` checks the plugin's API version
  and names, rejects duplicate ids and conflicting capability names, and adds
  everything or nothing. Look-ups: `plugins()`, `code_highlighter()`,
  `code_highlighter_names()`, `theme()`, `box_style()`, `renderer()` and
  `provided_by()`. `with_defaults()` registers the built-in `rich-ext` plugin
  (the number highlighter and the `syntect` code highlighter). The existing
  `register_highlighter`, `install` and `install_defaults` are unchanged.
- **CLI.** `rich doctor` lists the registered plugins and the plugin API version
  in its text report and as `plugins` in `--json`.
- **Release tooling.** `rs-rich-plugin-api-v*` tags, the CI feature matrix,
  release readiness, and the release scripts and tests know the new crate. Its
  first version is uploaded by hand after core 0.0.8 and before ext 0.0.10
  (BRANCHING, "Registry authentication").

### Python bindings, first slice (0.0.12 workstream 7: #197)

- **`rs-rich` on PyPI, `import rs_rich`** (`crates/rich-py`, PyO3 and
  maturin, version 0.0.1). It provides Rich's API for:
  - `Console`: `print`, `rule`, `export_text`, `record=` and `file=`;
  - `Text`, `Style` and markup (`escape`, `MarkupError`);
  - `Table`, `Panel` and the `box` constants;
  - `rs_rich.print`.

  A Rich program moves over by changing its imports.
- **All rendering is Rust.** The Python modules only re-export the compiled
  module's classes. Anything outside the slice raised instead of rendering
  differently; workstream 8 (below) fills in the rest.
- **Byte-compared with Python rich 15.0.0.** The tests cover markup and
  highlighting, `Text`, styles, the README table, table options, panels,
  rules, justification and `export_text`, in truecolor and plain. Links match
  except for upstream's random `id=`.
- **Packaging.**
  - One abi3 wheel per platform for CPython 3.9 and later, on Linux x86-64
    and arm64, macOS arm64 and x86-64, and Windows x86-64.
  - The new `python` workflow builds each wheel and smoke-tests it on Python
    3.9 and 3.13.
  - `pypi-release.yml` publishes from `python-v…` tags with PyPI Trusted
    Publishing (the `pypi` environment).
- The crate is outside the Cargo workspace, which is unchanged: its build
  needs a Python interpreter.
- Documentation: a [Python API reference](docs/python/index.md), one page per
  class (Console, Text, Style, Table, Panel, boxes/markup/errors, and
  compatibility). `tests/test_docs.py` runs every example and checks its
  printed output. `BRANCHING.md` covers the Python release.
- Tests: a pytest suite per class (`tests/test_*.py`); its expected output was
  checked against Rich 15.0.0. `test_compat.py` still compares whole programs
  byte for byte. Type stubs (`_native.pyi`) cover every class, and a test
  keeps them matched with the runtime names.
- `MarkupError` and `StyleSyntaxError` derive from `ConsoleError`, as in Rich.
  `Table.add_column(header_style=...)` now styles the whole header cell.
- Core parity fix: a table with `show_header=False` draws head-styled boxes
  plain, like upstream's `Box.get_plain_headed_box` (for example `HEAVY_HEAD` as
  `SQUARE`). The fix has new `table_headless_*` goldens.

### Python bindings: full parity (0.0.12 workstream 8)

- **Its own docs site**, published under `/python/` beside the main one: a
  getting started guide, a page per area, and an API reference for every module
  generated from the type stubs (`mkdocs-python.yml`,
  `scripts/gen_python_api.py`).

`rs_rich` now covers all of Rich 15.0.0's API and exposes every Rust crate.
Output is byte-compared with Rich (and, for the port's own crates, with the
Rust crates); 850+ tests and every documentation example run in CI.

- **Console.** `Console(highlighter=...)`, `tab_size` and `emoji_variant`;
  `log` of any renderable and `log_locals=True`; every `print_json` option;
  `export_html`/`export_svg` with `code_format` and `font_aspect_ratio`;
  `set_window_title`; containers, dataclasses and `__rich_repr__` objects
  pretty-print; an `Emoji` (and a titled `Rule(end="")`) leaves its line open.
- **Render hooks.** `push_render_hook`/`pop_render_hook`, `set_live`/
  `clear_live`: `Live`, `Status` and `Progress` redraw through the console
  instead of wrapping its file, so recorded output includes redrawn frames.
- **Text, Style and colour.** `Text` complete (`end` is a real field; meta
  data on spans: `apply_meta`, `on`, `assemble(meta=)`); `Style` complete
  (meta, `render`, `get_html_style`, `StyleStack`); `rs_rich.color`;
  `Theme.from_file`/`read`/`ThemeStack`; `markup.render`/`Tag`; `Emoji`;
  Rich's error messages and reprs (`Segment` included).
- **Renderables.** `Rule`, `Padding`, `Align`, `VerticalCenter`, `Constrain`,
  `Styled`; `Columns`; `Group`/`group`, `Renderables`, `measure_renderables`;
  `Tree` (no stack overflow on deep trees); `Layout` with splitters and
  regions; `Bar`; `Spinner` and `SPINNERS` (core's table; an unknown name is a
  `KeyError`). `Table` gains `Table.grid`, padding, `collapse_padding`,
  `pad_edge`, `style`, `highlight` and per-column `vertical`/`highlight`, and a
  cell's own `vertical` (`Align`) is honoured; `Panel` gains `style`, `height`,
  `highlight`, `safe_box` and `Text` titles.
- **Code and data.** `Markdown` and `Syntax` with every option, a code
  highlighter by name or plugin, and `Markdown(fences=)`; `Pretty`, `pprint`,
  `install`, `JSON`, `inspect`, subclassable highlighters; `Traceback`,
  `print_exception` and `install`.
- **Live and interactive.** `Live`, `LiveRender`, `Status`, `Screen`,
  `Pager`; `Progress` with every column, `track`, `wrap_file` and `open`;
  prompts; `RichHandler`.
- **The port's crates.** `rs_rich.ext` (38 modules, including `testing` and
  `qa`: snapshots, assertions with rendered diffs, stress, lint, explain,
  profile, fuzz, matrix, screenshots, benchmarks); `rs_rich.art` (images with
  every option, Pillow optional, byte-identical Sixel, FIGlet, GIFs, image
  diffs); `rs_rich.mermaid` (parse, draw, fences, `mmdc` in `mmdc` builds);
  `rs_rich.plugins` (Python classes as all seven capability kinds, checked by
  the Rust host; `ExtensionRegistry.install(console)`).
- **The command line.** The wheel ships the `rich` command as
  `python -m rs_rich` and the `rich-rs` script; `rs_rich.cli.main(argv)`
  returns the exit code. `rs-rich-cli` gains a library target (`run`,
  `run_embedded`); `--batch` workers and `--watch` restart through the host's
  command. In the `lumis` build the command line has the lumis highlighter too.
- **Rendered by core.** `Rule`, `Tree`, `Columns`, `Syntax` (every option,
  and `Syntax.highlight`), `Text.from_ansi`/`with_indent_guides` and the
  spinner table now render through core's, replacing the bindings' ports;
  output is unchanged against Rich. Code highlighters gain an optional
  `token_style(theme, token)` for line numbers and indent guides.
- **Core, additively, for the bindings.** Style meta data, a public spinner
  table (`spinner_names`, `spinner_frames`), `Console` `Clone + Sync`,
  `render_str_with`, `Json::with_options`, `export_*_with`,
  `set_window_title`, console `tab_size` and `emoji_variant`.
- **The last API gaps.** `Table` takes every Rich option: `width`,
  `min_width`, `show_footer` with column `footer`/`footer_style`, `leading`,
  `row_styles`, `header_style`/`footer_style`/`title_style`/`caption_style`,
  `title_justify`/`caption_justify`, `safe_box`, `Text` titles and captions,
  headers and footers of any renderable, `add_row(style=, end_section=)` and
  `add_section()`; a row with more cells than columns adds columns, as in
  Rich. `Panel` takes a `Text` subtitle, and `safe_box` is honoured.
  `Syntax` negative `start_line`, `line_range` and `stylize_range` positions
  render through core. `Layout.refresh_screen`, `Console.update_screen` and
  `update_screen_lines` redraw part of the alternate screen. None of these
  raises `NotImplementedError` any more; each is byte-compared with Rich.
- **Known differences** are listed in
  [Compatibility](docs/python/compatibility.md): Pygments versus syntect
  colours, no Jupyter output.
- **Docs and CI.** A page per module under `docs/python/` (and `ext/`), each
  example run by `test_docs.py`; the Rust oracle for the art and Mermaid tests
  lives in `crates/rich-py/oracles`; `python.yml` builds the `rich` binary for
  the CLI comparison, installs Pillow, and lints the `mmdc` build.

### Core: the last Rich API gaps (0.0.12 workstream 8)

Faithful ports of rich 15.0.0, each proved by goldens captured from real
Rich (`API_GAP_CASES` in `scripts/capture_golden.py`, `golden/api_gaps.tsv`,
`tests/golden_api_gaps.rs`).

- **`Table`**: `width` (implies `expand`) and `min_width`, in rendering and
  in `__rich_measure__`; `show_footer` with column `footer` and
  `footer_style` (`column_footer`, `column_footer_fill`) and a table
  `footer_style`; `leading` (the box's new `RowLevel::Mid` row, repeated on
  one line as upstream does); alternating `row_styles`, per-row `style`
  (whitespace dividers take its background) and sections (`add_row_with`,
  `add_section`, `end_section`); `header_style` as a theme-resolved
  `StyleType`; `title_style`, `caption_style`, `title_justify`,
  `caption_justify`, `Text` titles and captions (`title_text`,
  `caption_text`); `safe_box`; headers of any `Cell` (`add_column_cell`);
  the first and last rows drawn take the box's head and foot glyphs; a row
  with more cells than columns adds columns; `row_count`, `get_row_style`.
- **`Panel`**: `subtitle_as_text` and `safe_box`.
- **`Box::substitute`** keeps every ASCII box (`ASCII2`, `ASCII_DOUBLE_HEAD`,
  `MARKDOWN`) on an ASCII-only console, as upstream's `ascii` flag does.
- **`Syntax`**: `start_line`, `line_range`, `highlight_lines` and
  `stylize_range` positions are signed (`i64`), with upstream's Python
  semantics (a start of 0 or less is the first line, a negative end counts
  back, negative positions index from the end); `highlight_range` ports
  `Syntax.highlight(code, line_range)`.
- **`Layout::refresh_screen`**, with `Console::set_alt_screen`,
  `is_alt_screen`, `update_screen`, `update_screen_lines`, `ScreenUpdate` and
  `RichError::NoAltScreen`. `Console::control` now records into a capture,
  as upstream buffers control codes.

### Native image sizing (0.0.12 workstream 6: #519)

- **`rich_art::ImageFit::Native`** renders an image at its own pixel size in
  whole cells. The density is per backend: 1×2 pixels a cell for ASCII and
  half-blocks, 2×4 for quadrants and Braille, and 8×16 for Sixel. Pixels
  land unscaled, and the last partial cell takes the background.
  - The console width, `width`/`height` and `max_width`/`max_height` only
    shrink it, keeping the aspect ratio. It is never enlarged to fill the width.
  - `ImageArt::native_grid(mode, available)` reports the grid.
  - Adding the variant is a breaking change for exhaustive `match`es on
    `ImageFit` (rs-rich-art is 0.0.x).
- **CLI: `--image-fit native`**, and `image_fit = "native"` in `rich.toml`.
  Unlike the other fits, it needs no `--height`.
- The art guide has a "Native size" section with a rendered example.
  `docs/cli.md` and the CLI reference are updated too.

### Render tree spike (0.0.12 workstream 5: #226)

- **Design note, no code change:**
  [docs/design/render-tree.md](docs/design/render-tree.md). It covers what an
  intermediate frame would give export, snapshots, accessibility and live
  repaint, and a migration that leaves core alone.
- **Measurements** come from a standalone prototype
  (`docs/design/render-tree/prototype`, not a workspace member). Rows of
  styled runs with interned styles retain 2–7× less heap than `Vec<Segment>`,
  encode no slower, and reproduce today's bytes exactly. A per-grapheme cell
  grid costs more than segments on text-heavy output.
- **Recommendation:** styled-run frames in `rich-ext`, proposed as milestone
  0.0.13 "Frames".

### Composable transforms (0.0.12 workstream 4: #216)

- **`rich_ext::transform`.** `Transform<T>` rewrites a value; `Pipeline<T>`
  runs named stages in order and reports which one failed (`PipelineError`).
  `from_fn` makes a stage from a closure, and a pipeline is itself a stage.
  - Text: `KeepLines` (lines matching a regular expression, or not) and
    `HighlightMatches`.
  - Data (`rich_ext::data::transform`, `jsonpath` feature): a `Document`
    (tree, label, highlighted paths) with `Redact`, `Select`, `Filter` and
    `Highlight`. `Explorer::highlight(path, style)` draws highlighted lines.
  - Tables (`table::transform`): `Sort` and `Group`. Patches
    (`diff::transform`): `KeepFiles`.
- **Plugins can contribute text transforms.** The plugin API gains
  `TextTransform`, `PluginRegistrar::transform(name, transform)` and
  `Capability::Transform`. `ExtensionRegistry` hosts them: `transform(name)`,
  `transform_names()`, `register_transform` and `text_pipeline(names)`.
  `rich doctor` lists them with each plugin's capabilities. The plugin API is
  unpublished, so `PLUGIN_API_VERSION` stays 1.
- **CLI: `--filter` and `--highlight`.** With `--inspect` they take a JSONPath:
  `--filter` keeps what it selects and the containers above it, and
  `--highlight` shows those tree lines in reverse video. For text, `--print`
  and `--syntax` they take a regular expression, matched per line.
  - One fixed order, whatever order the flags come in: `--redact`, `--select`,
    `--filter`, `--highlight` for `--inspect`; `--filter`, `--highlight` for
    text. It is written down in [docs/cli.md](docs/cli.md#filter-and-highlight)
    and pinned by a test.
  - `--redact` and `--select` now run as stages of that pipeline. Their output
    is unchanged.
  - Other modes refuse both flags, and `--compare`, `--find`, `--flatten` and
    `--table` refuse the combinations they cannot show.
- **Guide: "Transforms"** ([docs/guide/ext/transforms.md](docs/guide/ext/transforms.md)),
  from the compiled `guide_transforms` example, including a plugin transform.

### Highlighter conformance kit, guide and benchmarks (0.0.12 workstream 1: #526)

- **`rich_ext::testing::conformance`** (the `testing` feature).
  `conformance::check(highlighter)` runs one contract against any
  `CodeHighlighter`, and reports every failure with the input that caused it:
  - listed themes highlight, and an unknown theme is `UnknownTheme`;
  - line counts match `split('\n')` for empty input, a missing final newline,
    CRLF, tabs and multi-byte text;
  - spans are sorted, disjoint, in range and on character boundaries;
  - an unknown language renders as no language;
  - rendered output has no control characters beyond styling;
  - 10,000 lines cost at most 40× what 1,000 lines do (a relative budget, not
    a wall-clock one).
  `check_with(…, Options { all_themes, scaling })` tunes it.
  - syntect passes it with every theme, and lumis passes it in its own tests.
  - Eight deliberately broken adapters are each rejected by the check they
    break.
- **`ExtensionRegistry::register_code_highlighter(name, engine)`** registers an
  engine without a plugin, with the same name and conflict rules.
- **Guide: "Code highlighters"**
  ([docs/guide/ext/code-highlighters.md](docs/guide/ext/code-highlighters.md)):
  choosing between syntect and lumis (the trade-offs, and the tree-sitter
  linking caveat), a keyword-only adapter in about 40 lines that passes the
  kit, and the ANSI themes. The example (`guide_highlighters`) is compiled, and
  its conformance test runs with `cargo test`.
- **Benchmarks:** syntect, syntect with `onig`, and lumis on the review files
  ([docs/benchmarks.md](docs/benchmarks.md#0012-code-highlighters)).
  - lumis has a fixed cost per language (0.07–0.12 s), then scales best: 1.87 s
    for 29,160 lines of Python, against 3.66 s with `onig`.
  - The CLI binary with the `lumis` feature is 169 MB, against 15.7 MB without
    it.

### Choosing the code highlighter (0.0.12 workstream 1: #525)

- **Core: a console-wide default.** `protocol::CodeHighlighting` (an engine and
  an optional theme) and `ConsoleCodeHighlighting`, implemented by `Console`.
  - A `Syntax` without a highlighter of its own uses the console's when it
    renders. Markdown code blocks do too. The console's theme applies unless
    the `Syntax` names one.
  - `Syntax::highlight_for(console)` is `highlight()` with the console's
    default.
  - A console without a default highlights exactly as before.
- **Ext.**
  - `ExtensionRegistry::set_default_code_highlighter(name, theme)` chooses a
    registered engine and theme. `install()` puts it on the console, and
    `code_highlighting()` returns it.
  - An unknown name or theme is a `HighlighterChoiceError` that lists the
    choices.
  - `source_view` and the text and patch diff views highlight with the
    console's default.
  - `RenderTarget::with_code_highlighting` passes one to a target's consoles.
- **CLI.**
  - `--highlighter NAME` and `--code-theme NAME`, with the `highlighter` and
    `code_theme` config keys. A working-directory `rich.toml` may set them:
    they are a plain choice among compiled-in engines.
  - `syntect` is always available. `lumis` needs the new off-by-default
    `lumis` feature (and Rust 1.91).
  - An unknown highlighter or theme is a usage error listing the choices, and
    says when lumis needs the feature.
  - They apply to `rich FILE`, `--syntax`, `rich view`, `rich diff`, Markdown
    and batch workers.
  - `rich doctor` reports the compiled-in highlighters with their themes and
    the active one and its theme, in its text and `--json` output.
  - With neither option, nothing changes.
  - Upstream's `--theme` (a syntax theme) is this CLI's config-theme option,
    so the code theme is `--code-theme`.

### The lumis highlighter (0.0.12 workstream 1: #524)

- **New crate `rs-rich-lumis` 0.0.1** (`rich_lumis`): a `CodeHighlighter` over
  [lumis](https://lumis.sh), with tree-sitter grammars and Neovim themes.
  - Use it with `Syntax::highlighter` or `Markdown::highlighter`, or register
    `LumisPlugin` to choose it by the name `"lumis"`.
  - **Themes** are lumis's own names (more than 250). The default is
    `monokai`, upstream rich's default. Themed code gets the theme's background
    and foreground.
  - **`ansi_dark`/`ansi_light`** use upstream rich's ANSI tables, mapped from
    tree-sitter capture names (`keyword.operator`, `function.builtin`, …) to
    Pygments token types, as the syntect adapter does.
  - **Languages:** all of lumis's (116) by default. The `bundle-web`,
    `bundle-web-extra`, `bundle-system` and `bundle-backend` features give a much
    smaller build (the web bundle's binary is 9 MB against 156 MB).
  - An unknown language highlights as plain text. An unknown theme is
    reported, and `Syntax` falls back to the default theme.
- **Its own crate and minimum Rust.** lumis links tree-sitter's C runtime, so
  only builds that ask for it pay for that. It needs Rust 1.91 (lumis's
  minimum). The workspace stays at 1.90, and CI checks this crate on 1.91
  separately.
- **Release tooling and CI.** `rs-rich-lumis-v*` tags, the feature matrix,
  release readiness, and the release scripts and tests. The first upload is
  manual, after the plugin API and before CLI 0.0.12 (BRANCHING).

### Markdown fences and Mermaid diagrams (0.0.12 workstream 3: #222)

- **Core: `FenceRenderer`.** A new extension point in `rich::protocol`.
  `Markdown::fence_renderer` asks each renderer, in the order added, whether it
  draws a ```` ```lang ```` fence; if none does, the fence is highlighted as
  before. Indented code never reaches a renderer. With none added, Markdown is
  byte-identical: goldens are unchanged.
- **Plugin API and ext.** `PluginRegistrar::fence_renderer(language, …)` and
  `Capability::FenceRenderer`. `ExtensionRegistry::fence_renderer(language)`
  looks one up, and `fences()` combines them into one for Markdown, routed by
  language; two plugins cannot claim the same language.
- **New crate `rs-rich-mermaid` 0.0.1** (`rich_mermaid`), built on the plugin
  API alone (`MermaidPlugin`, `MermaidFences`, `Mermaid`).
  - **Flowcharts as text:** `graph`/`flowchart` in `TD`, `TB`, `BT`, `LR` and
    `RL`; the common shapes; solid, thick, dotted and invisible edges with
    arrow, circle and cross heads, labels, longer links, chains and `&`. A
    layered layout gives every edge its own port and track, so lines never
    run together. ASCII where the console is ASCII-only; cropped with a note
    when wider than the terminal. Styling statements are ignored; subgraphs
    are drawn flat with a note.
  - **Every diagram type through `mmdc`** behind the off-by-default `mmdc`
    feature: Mermaid's own CLI renders a PNG, shown as Sixel where the terminal
    supports it, otherwise quadrant blocks with a note. Without real pixels,
    flowcharts still prefer text. The source goes through a private temporary
    file, never a shell; there is a 20 s timeout (Chromium is stopped with its
    process group) and a 64 KiB cap.
  - **Never an error on screen:** whatever cannot be drawn shows its source
    in a code block under a one-line note saying why (unsupported type, parse
    error with its line, `mmdc` missing, timed out or failed). Labels and
    shown source lose control characters, so a diagram cannot write escape
    sequences.
- **CLI.**
  - `rich mermaid FILE` (alias `mmd`), with `.mmd` and `.mermaid` files
    detected.
  - ```` ```mermaid ```` fences draw in `rich --markdown`.
  - `--mermaid-backend text|mmdc|off` and the `mermaid_backend` config key.
    `off` keeps upstream's plain fences.
  - `rich mermaid` tries `mmdc` first in a build with it.
  - A working-directory `rich.toml` cannot choose `mmdc`, since it starts a
    browser: the setting is dropped with a warning and a `config explain` note.
  - The `mermaid` feature is on by default and `mmdc` is off.
  - `rich doctor` lists the Mermaid plugin, and both features.
- **Release tooling and CI.** `rs-rich-mermaid-v*` tags, the feature matrix
  (with and without `mmdc`), release readiness, release scripts, and a
  `Mermaid CLI backend` job that installs `@mermaid-js/mermaid-cli` and renders
  a flowchart, sequence, class and state diagram. The crate's first version is
  a manual upload after art 0.0.10 and before CLI 0.0.12 (BRANCHING).

### Packaging

- The CLI README's "prepared, not yet published" line now names 0.0.12. It
  shipped inside the published `rs-rich-cli` 0.0.11 package, so it could only
  change with a new CLI version.

## Core 0.0.7 / macros 0.0.1 / ext 0.0.9 / art 0.0.9 / CLI 0.0.11 — published 2026-09-24

Cohort versions for 0.0.11: core 0.0.7, macros 0.0.1 (new), ext 0.0.9, art
0.0.9, CLI 0.0.11. Core changes below, so every dependent moves with it.

- Published from `main` at `433c1b1`, one tag at a time in dependency order:
  [`rs-rich-v0.0.7`](https://github.com/buchochelliq-labs/rs-rich-cli/actions/runs/36063309820),
  then `rs-rich-macros` 0.0.1 uploaded by hand with a maintainer token (a new
  crate cannot start with Trusted Publishing), then
  [`rs-rich-ext-v0.0.9`](https://github.com/buchochelliq-labs/rs-rich-cli/actions/runs/36065371117),
  [`rs-rich-art-v0.0.9`](https://github.com/buchochelliq-labs/rs-rich-cli/actions/runs/36065405590)
  and [`rs-rich-cli-v0.0.11`](https://github.com/buchochelliq-labs/rs-rich-cli/actions/runs/36065429562)
  through Trusted Publishing. See the
  [release notes](docs/releases/0.0.11.md#publication).

### Optional Oniguruma highlighting (`onig`)

- **Core, CLI.** A new `onig` Cargo feature, off by default, runs syntect on
  Oniguruma instead of pure-Rust `fancy-regex` (DIVERGENCES #26). Highlighting
  is 2–4× faster: a full `rich` run on a 1,784-line Rust file went from 0.43 s
  to 0.16 s. Output is unchanged: 558 repository files rendered byte-identically
  under both engines. It needs a C compiler at build time and adds about 330 KB.
  `rich doctor` lists it under build features.

### Release test: security, robustness and parity fixes

Six independent audits of the whole 0.0.11 delta found the issues below. Each was
confirmed by reproduction, and each fix came with a regression test that failed
first. See the [0.0.11 release notes](docs/releases/0.0.11.md#what-the-release-test-found-and-fixed).

- **Dependencies.** `rustls` 0.23.45 for RUSTSEC-2026-0285 (reaches only the
  CLI's `fetch` feature). Core loads only syntect's bundled dumps, which drops
  the unmaintained `yaml-rust` and `plist` from the tree.
- **Core: Markdown links (security and parity).** Link and image destinations
  now go through a port of markdown-it's `normalizeLink`: mdurl
  percent-encoding, plus punycode written by hand, so no new dependency. They are
  also checked with markdown-it's `validateLink`. An escape sequence in a link can
  no longer reach the terminal, `é.com/ü` becomes `xn--9ca.com/%C3%BC`, and
  `javascript:`, `vbscript:`, `file:` and non-image `data:` links print as
  literal text, as upstream does. `~~` inside an autolink is no longer
  strikethrough. A refused reference definition still prints nothing
  (DIVERGENCES #24).
- **Core: parity.**
  - A printed `Text`'s base style no longer colours justify padding.
  - Zero-width table cells get no vertical padding.
  - Byte columns and speeds keep negative values (`-8/100 bytes`).
  - `TextColumn` format specs follow CPython's `format()`: 3,836 golden cases,
    up from 509 that diverged.
- **Core: robustness.**
  - A `LiveProgress::refresh()` inside `with()` no longer deadlocks; a nested
    `with()` panics with a clear message.
  - A render panic on the live thread restores the cursor. `AutoLive::try_stop`
    reports the panic.
  - `ProgressBar` no longer overflows on huge totals.
  - `Tree` renders, measures and drops without recursion, tested at 20,000
    levels.
  - Theme files parse in linear time.
  - JSON deeper than 10,000 levels is an error rather than an unbounded render
    (DIVERGENCES #25).
- **Ext: terminal-control injection.**
  - OSC 8 links could be broken out of in three places, each letting a crafted
    path or URL set the window title:
    - control characters in `LiveCoordinator` link URLs;
    - control characters in `Hyperlinker` file and editor URLs;
    - control characters in diff `TemplateLinks`.
  - File URLs are now percent-encoded outside the RFC 3986 path set, including
    non-ASCII.
  - Stack-trace labels, patch paths decoded from git's octal quoting, and
    JUnit/libtest names and output decoded from `&#x1b;` or `\u001b` now render
    inert.
  - Bidi controls (Trojan Source) are shown as escapes in `rich unicode` and in
    env views.
- **Ext: secrets.** `rich env` / `EnvView` mask secret names by whole segment
  (`*_KEY`, `DB_PASS`, `MYSQL_PWD`, `*_DSN`, `*_COOKIE`, `JWT`, …). They also mask
  credentials inside any value (URL passwords, token prefixes, JWTs, AWS key ids)
  through `redact_value`. Masking is best effort.
- **Ext: crashes and hangs.**
  - Values that panicked `richf!` at run time can no longer escape the template's
    markup.
  - Stack-trace cause chains are capped at 64, and render and drop without
    recursion.
  - Hunk headers with impossible numbers and huge hex line widths are now errors
    or clamped instead of overflowing.
  - A CRLF `Suggestion` span no longer panics.
  - libtest reports and wrapped `SourceView` lines (minified JSON in `rich view`)
    are no longer quadratic.
- **Ext: correctness.**
  - `RichHandler::live` no longer drops lines containing control characters.
  - Non-interactive `LiveCoordinator` output keeps the whole final snapshot.
  - A truncated JUnit file is an error, not a pass.
  - Tracing span fields are replaced, not duplicated.
  - An empty `FORCE_COLOR` is ignored.
  - QA screenshot names can't leave their directory.
- **Ext: structured data (`rich inspect`) and CLI authoring.**
  - YAML aliases are charged by the bytes they copy (64 MiB), and only aliased
    anchors are cloned, which stops alias and nested-anchor memory bombs.
  - JSONPath slices with huge steps no longer overflow; filters nest at most 128
    levels, and a selection past 1,000,000 nodes is an error.
  - YAML anchor names, C1 controls and DEL in keys and values, parse-error
    messages and diagnostic snippets are all escaped.
  - `--redact` masks dash spellings (`api-key`), everything under a secret key,
    and XML element text.
  - Generated completion scripts and man pages can no longer be injected into:
    - zsh/fish command names and fish subcommand words;
    - man page requests from multi-line fields.
  - Man pages escape non-ASCII for groff.
  - Completions offer each subcommand's own and global options.
  - Minified (single-line) XML and TOML parse in linear time: 200,000 elements
    took 35 s and now take 0.3 s.
  - JSON `-0` reads as `0`.
  - YAML duplicate keys, an INI key shadowed by a section, and content after the
    XML root are errors.
- **Core: speed.** `Text::divide` visits only the lines each span touches, as
  upstream does. Rendering 40,000 highlighted lines in `rich view` went from
  132 s to 11 s.
- **CLI: terminal controls.**
  - `rich view` and text `rich diff` neutralise terminal controls by default
    (`--no-sanitize` opts out). Both are new commands with no upstream behaviour
    to mirror, and `view` could otherwise pass clipboard-writing OSC 52 to the
    terminal.
  - `rich diff --sanitize` now covers patches, file names and C1 codes.
  - `rich capture` always cleans its title, and with `--sanitize` also C1 codes
    and the cast.
  - File names in error messages show control characters as symbols.
- **CLI: resources.**
  - `hex` reads only the window it shows.
  - `view`, `hex`, `unicode`, `inspect` and `capture` have documented size
    limits (docs/cli.md "Limits").
  - `capture` stops reading 1 s after the command exits, even while a background
    child still holds the pipe.
- **CLI: correctness.**
  - `capture` gives the command the panel's real width.
  - `config` and `doctor` no longer panic on a closed pipe.
  - `rich <command> --help` shows that command's help.
  - `bench compare` reports success in JSON, and a 0 ns baseline is a change.
  - `hex --bytes-per-line` must be 1–4096.
  - A bad `--select` is a usage error.
- **CLI: config trust.** A working-directory `rich.toml` can no longer set
  `theme_file`, choose export files (`export_html`, `export_svg`; ignored with a
  warning) or turn off the new `view`/`diff` sanitising, the same rule as
  `NO_COLOR`. Theme files must be regular files of at most 1 MiB, so a FIFO no
  longer hangs `rich`.
- **Tests.** Two intermittent failures were traced to the tests:
  - `test_batch_v9.py` stopped a worker before it exec'd, which suspended the
    parent in `posix_spawn`'s vfork. It now stops only exec'd workers.
  - The XML linearity test compared against a wall-clock limit. It now compares
    two sizes, which is how the quadratic XML parse above was found.
- **Art.**
  - GIF decoding is capped at 512 MiB (a 35-byte GIF used to abort on a 17 GB
    allocation).
  - Sixel rasters are capped at 16 megapixels before resizing, and repeat runs are
    split at 65535.
  - `TerminalDefault` keeps the colour of pixels at or above half opacity in every
    palette, and in ASCII and Braille.
  - Reduced-palette Sixel is 4–16× faster, with byte-identical output.
  - Sixel errors now tell "not a terminal" from "terminal not known to support
    Sixel", and `RICH_GRAPHICS=sixel|none` works as documented.
- **Release docs.** A new crate's first version cannot use Trusted Publishing, so
  `rs-rich-macros` 0.0.1 needs one token-authenticated upload before the ext tag
  (`docs/BRANCHING.md`).
- **Demo.** The tour shows `inspect`, `diff`, `view`, the `hex`, `unicode` and
  `ansi explain` inspectors, a theme file, a redacted `capture`, Atkinson with
  OKLab, and both alpha backgrounds.
- **Migration.**
  - `StackTrace` has a new public `omitted_causes` field.
  - New enum variants: `unicode_inspect::Kind::Bidi`,
    `ImageArtError::{SixelTooLarge, SixelNotSupported}`.
  - `richf!` prints a value's `\[` literally; it used to unescape it.
  - New ext items: `ArgSpec::global`, `env_inspect::redact_value`,
    `hex::MAX_BYTES_PER_LINE`, and the `sanitize` helpers
    `sanitize_terminal_and_bidi_controls` and `sanitize_single_line`.
  - `junit::parse` rejects a truncated file.
  - Core additions: `LivePanic`, `AutoLive::try_stop`,
    `filesize::{decimal_signed, pick_unit_and_suffix_signed}`, `json::MAX_DEPTH`.

### Workflow renderables (0.0.11 workstream 10)

All in `rs-rich-ext`, with no new dependencies and no change to core. Every
renderable has a plain fallback that carries its meaning without colour and
follows the accessibility policy (ASCII symbols, reduced motion).

- **Commands, task trees and summaries (`workflow`; #388, #389, #393).**
  - `CommandRecord` holds a process's argv, working directory, interleaved
    stdout/stderr, exit status and duration. Running a process
    (`CommandRunner`, which a `CancelToken` can kill) is separate from showing
    one (`CommandView`: output folded to its tail, stderr marked `!`, a spinner
    while running, a diagnostic after a failure).
  - `TaskTree`: nested tasks whose parents aggregate their children's status,
    with per-task cancellation, progress, notes and time from an injected
    `Clock`. It can collapse finished subtrees.
  - `CompletionSummary`: overall status, counts, duration, the items that need
    attention and next steps, built by hand or from a finished `TaskTree`.
- **Transfers, retries and notifications (#390, #391, #392).**
  - `transfer`: `Transfer`/`Transfers` with a moving-window rate, ETA, retry,
    pause and cancel states; `transfer_columns()` and `Transfer::task_update()`
    drive the core `Progress`; `TransferReader`/`TransferWriter` count bytes and
    stop on a `CancelToken`.
  - `countdown`: `Backoff` (exponential, capped, attempt limit, seeded jitter),
    `RetryStatus`, `RateLimit`, `CountdownBar`, and `CountdownWait`, a
    cancellable wait that redraws in a live region or prints one line per
    attempt when motion is off or the output is not interactive.
  - `notify`: `Notification` toasts (one line or a panel) and a `Notifications`
    stack with expiry, shown below live regions or printed once each when the
    output is not interactive.
- **Tables (`table`; #259, #429).**
  - `StreamingTable`: keyed rows for append and update workloads (upsert keeps a
    row's place), head and tail windows, and capacity eviction. Only changed
    rows are laid out again unless a column's width changes, and the output is
    byte-identical to the same core `Table` built from scratch.
  - `TableData`: stable multi-column sorting (numbers and digit runs compare by
    value, empty cells last, `▲`/`▼` or `^`/`v` in headers), grouping with
    count, sum, min, max, mean or custom aggregates, and totals, rendered as a
    core `Table`.
- **Badges, size bars and formatters (#426, #427, #425).**
  - `Badge`/`Badges`: status, label, link (OSC 8) and metadata chips that read
    `[OK build]` or `[docs <url>]` without styles, and wrap between badges.
  - `SizeBar`: a size against a total or limit, with going over shown by glyph,
    percentage and "over by" text.
  - `format`: sizes (decimal and binary), rates, durations, clock times,
    relative times, UTC timestamps, percentages and numbers.
- **Redaction (`redact`; #224), experimental.** Best effort, not a guarantee:
  check redacted output before sharing it, and report anything that gets
  through as a bug. `Redactor` masks secrets in strings, ANSI text
  and rendered segments, keeping styles and cell widths, so output can be
  redacted between recording and export. Built-in detectors cover secret-named
  keys, bearer tokens, common token prefixes, AWS access keys, JWTs and URL
  passwords; custom patterns can mask a named `secret` group. `SECRET_KEYS`
  moves to `redact`, and `data::SECRET_KEYS` still re-exports it.
- **CLI: `rich capture --redact` and `--redact-pattern REGEX`** (experimental,
  with the same caveat) mask the command
  line and its output before the capture is shown, exported or recorded.
- **Shared:** `cancel::CancelToken` (with child tokens) and
  `theme::STYLE_TABLES`, the one list of style tables `extended_theme()`
  layers, which now includes the new `workflow.*`, `transfer.*`,
  `countdown.*`, `notify.*`, `table.*`, `badge.*` and `size_bar.*` keys.
- **Fixed before merge:** a command that had already exited was reported
  cancelled when its token fired while its pipes drained.

### Art and CLI carry-over: colour modes, alpha backgrounds and theme files (0.0.11 workstream 12)

- **Art: Atkinson dithering and OKLab distance (#498).**
  - `Dither::Atkinson` (`--image-dither atkinson`) spreads six eighths of the
    error over six neighbours.
  - `ColorDistance::Oklab` (`--image-color-distance oklab`, config
    `image_color_distance`) picks the nearest palette colour perceptually. The
    default, `Rgb`, leaves every existing output byte-identical.
- **Art: colour modes for Sixel and GIF (#498).**
  - Sixel accepts `--image-color ansi256|ansi16|grayscale`. It quantizes and
    dithers like the text backends, then encodes exactly the palette's colours.
    Truecolor Sixel is unchanged.
  - GIF frames (`AnimatedArt::color_mode`/`dither`/`color_distance`, and the
    same flags with `--gif`) take a reduced palette.
  - Braille is monochrome and still rejects a reduced palette, now with an
    error that says so.
- **Art: alpha backgrounds (#126).** `ImageBackground` chooses what transparent
  pixels become. `Color` is what `ImageArt::background` did.
  - `TerminalDefault` (`--image-background default`) leaves them unpainted, so
    the terminal's background shows through, including in contain padding.
  - `Checkerboard` (`--image-background checkerboard`) previews them on gray
    squares.
  - Opaque images and every existing option render as before.
- **CLI: `--theme-file PATH` (#499)** (config `theme_file`, relative to the
  config file) loads an upstream `[styles]` theme file.
  - It layers above the built-in theme and below config themes and
    `--theme-style`.
  - A bad file is a usage error naming the file and line.
- **Migration:** exhaustive matches on `Dither` must include `Atkinson`. The
  internal `ImageArt` background field is now an `ImageBackground`, but the
  `background([r, g, b])` builder is unchanged.

### CLI viewers: `view`, `hex`, `unicode`, `env` and `capture` (0.0.11 workstream 11)

- **`rich view FILE` (#401)** shows a file the way it should be read:
  - Markdown, CSV, notebooks, JSON Lines, images, GIFs and patches go to their
    renderers.
  - Source code and structured data (JSON, YAML, TOML, XML, INI) are
    highlighted with line numbers.
  - Binary input becomes a hex dump.
  - It detects the format from the extension, else from the content (stdin
    included, read once), and pages by default.
  - `--search TEXT` highlights case-insensitive matches, marks their line
    numbers and prints a summary to stderr; `--no-line-numbers` drops the
    gutter.
- **`rich hex` (#410):** a `hexdump -C`-style dump with offsets, byte groups, an
  ASCII panel, colour by byte class and `*` for repeated lines. `--offset`,
  `--length`, `--bytes-per-line`, `--group`, and `--search BYTES` (hex or
  quoted text) shape and search it.
- **`rich unicode` (#411):** one row per grapheme (code points, UTF-8 bytes,
  width, a Rust escape and a kind), invalid UTF-8 as rows of its own, and a
  summary; `--limit N`.
- **`rich env` (#412):**
  - Lists environment variables, masking secret-looking names unless
    `--show-secrets` (`AUTH` only as a whole word, so `AUTHOR` shows).
  - Arguments filter names.
  - `rich env PATH` checks each entry: missing, duplicate of #N, empty, not a
    directory.
  - Control characters in names and values are shown inert.
- **`rich capture -- CMD` (#424):** runs a command with colour forced and
  `COLUMNS` set, stdout and stderr merged in order through one pipe, and shows
  the output in a panel with its exit status and duration. `--export-svg`,
  `--export-html` and `--cast FILE` (asciicast v2) record it. It exits with
  the command's status (128 + the signal for a killed command) once the panel
  and exports are written; PNG export is not provided.
- **Ext:** new `source_view::SourceView`, `hex::HexView`,
  `unicode_inspect::UnicodeView` and `env_inspect::{EnvView, PathView}`, all
  width-aware and ASCII-safe, with no new dependencies.

### Ext: tracing spans, span trees, editor links and Live-safe logging (0.0.11 workstream 7)

- **Spans (#286):** `EventLayer` now tracks spans in the subscriber's registry.
  Every event carries the spans it happened in, outermost first, with their
  fields as last recorded (`StructuredEvent::span_context`). With
  `span_open(true)` and `span_close(true)` it also reports spans opening and
  closing, with how long they were open (`span_marker`).
- **`RichHandler` span views:** `SpanView::Inline` (the default) prints
  `outer{a=1}:inner: message`. `SpanView::Tree` indents events under their
  spans with `│ ` guides and draws `┌ name` and `└ name 1.20ms` branches, in
  ASCII on ASCII consoles. `SpanView::Hidden` shows the message alone.
- **Editor links:** `RichHandler::hyperlinker` links the path column through a
  `Hyperlinker`, so its editor template and a base directory for `tracing`'s
  relative paths apply. Core's `LogRender` is unchanged.
- **Live-safe output:** `RichHandler::live` prints through a `LiveCoordinator`,
  above its regions, so logging never tears a live display.
- **Breaking:** `EventLayer` now requires `S: LookupSpan`, as every
  `tracing_subscriber` registry-based subscriber is. Events logged inside spans
  now show the span chain before the message; `span_view(SpanView::Hidden)`
  restores the old output.
- `rich_eprintln!` and `rich_trace!` (#285) shipped with the macros in
  workstream 5; they print through the themed console and are unchanged here.
- The logging guide covers spans, both views, editor links and Live output,
  with two new screenshots. Its example compiles every snippet.

### CLI: a project's `rich.toml` cannot undo `NO_COLOR`

- A `no_color = false` in a `rich.toml` found in the working directory used to
  turn colour back on even with `NO_COLOR` set, so running `rich` inside a
  repository could colour the output of someone who had asked for none. That
  file now can turn colour off but not on while `NO_COLOR` is set, in
  `[defaults]` or a profile. Your own `~/.config/rich/config.toml`, a file named
  with `--config`, and `--color` still override `NO_COLOR`, as the NO_COLOR
  convention allows for user configuration.
- `config show` reports the effective setting, and `config explain` notes when
  the file's value was ignored. The config reference and `docs/cli.md` describe
  the rule.

### Core: table vertical padding follows the row's position

- A table applied its top and bottom padding to every row alike. Upstream
  (`_get_cells`' `get_padding`) picks them per row. With `collapse_padding`,
  every row but the last keeps only `max(0, top - bottom)` below it. With
  `pad_edge` off, the first row (the header, when shown) loses its top padding
  and the last row its bottom. So `Table::grid().padding(0, 2, 1, 0)` no longer
  leaves a blank line after each row, and a grid of tables stacks as it does
  upstream.
- Checked against rich 15.0.0: 240 combinations of padding, `collapse_padding`,
  `pad_edge`, header, row count, boxed and grid match byte for byte (98
  differed before). Goldens: `table_vpad_{grid,boxed,collapse,no_edge,
  header_only,no_header}`.
- The guide's box gallery is regenerated. `box::NONE`'s doc comment no longer
  calls it a port: upstream has no `box.NONE` and uses `box=None`, which draws
  no border lines at all.

### Documentation: the user guide

- **`docs/guide/`** walks through every component: an overview of the crates,
  how they fit together and how one render flows; the core (console, text and
  style, tables, layout, trees, progress and live displays, code and data,
  logging, prompts, export); every rs-rich-ext module (extensions and links,
  diagnostics, logging, live regions and layouts, macros, structured data, CLI
  authoring, diffs and test reports, QA tooling, capabilities, accessibility,
  ANSI explain); rs-rich-art (banners, images, GIFs, image diffs); and the `rich`
  CLI (overview, a hands-on walkthrough of every command, the smoke test).
- **Every code sample is compiled.** Pages include sections of the
  `guide_*` example programs in each crate's `examples/`, and every screenshot
  is that example's real output exported to SVG. `scripts/capture_guide.py`
  regenerates them (`--check` fails on a stale image).
- **`scripts/smoke_cli.py`** runs every CLI command and mode end to end against
  generated fixtures (86 cases, no network or terminal needed), checks exit codes
  and output, and regenerates the CLI screenshots with `--screenshots`.
- **Fixes to existing docs:** the tutorial's `Live` example used an API that does
  not exist (it now includes the compiled one); a markup error exits 4, not 1;
  getting started pins the current release; the VS Code link template in
  `Hyperlinker::editor`'s docs produced a double slash; rich-art's README, crate
  docs and description now cover images, GIFs, Sixel and the current colour and
  dithering rules; docs.rs builds rs-rich-ext and rs-rich-art with all features,
  so feature-gated APIs are documented there.

### Core: no_color, spinner time and prompt output match rich

Three gaps found while writing the user guide, each verified against rich
15.0.0 and pinned by new golden fixtures (`no_color.tsv`, `prompt_ask.tsv`,
three `clock_*` rows in `live_status.tsv`).

- **No-colour mode keeps attributes.** `no_color` (or a non-empty `NO_COLOR`)
  used to drop the colour system, so bold, italic and underline went too.
  Upstream keeps the colour system and removes only colours when output is
  written (`Segment.remove_color` in `Console._render_buffer`).
  - **Behaviour change:** `Console::color_system()` now reports the colour
    system whatever `no_color` says, as upstream's `Console.color_system` does.
    Code asking "will colour reach the terminal?" must also check
    `Console::no_color()`. The CLI's render target, `rich doctor`,
    `rich_ext::qa::explain_console` and rich-art image and GIF rendering now
    do.
  - Capture and printing strip colours. HTML, SVG and plain-text exports read
    the recording, so they keep their colours, as upstream's do.
  - In a terminal, `NO_COLOR` now keeps bold and underline, as it does in
    upstream rich-cli (which honours it through `Console`); our `--no-color`,
    documented as equivalent, does the same. Piped output is still plain.
  - A `TERM` of `dumb` or `unknown` now gets no colour system, as upstream's
    `is_dumb_terminal` does. `NO_COLOR` had hidden this gap.
  - New `Style::without_color` and `Segment::remove_color`.
- **Spinners animate by themselves.** A `Spinner` or `Status` rendered as a
  renderable now shows the frame for the console's clock
  (`Spinner.__rich_console__` → `render(console.get_time())`), so one placed in
  a `Live` display moves. Before, it always drew frame 0.
  - New `Console::get_time` and `ConsoleBuilder::get_time`, ports of
    `Console(get_time=…)`, default monotonic. This is the same clock as
    `Progress`: `progress::GetTime` is now `console::GetTime`, re-exported.
  - `Spinner` measures as `render(0)`, as upstream's `__rich_measure__` does,
    which also fixes the start time at 0.
- **Prompt questions go through the console.** `Prompt`, `Confirm`,
  `IntPrompt` and `FloatPrompt` wrote the question with `print!`, so capture and
  export missed it. New `Console::input` / `input_from` port `Console.input`:
  the prompt is printed through the console with `end=""`, then a line is read.

### Fixes: print macro captures, flag suggestions, diagnostic locations, SVG titles

- **Print macros capture locals.** `rich_println!("[bold]{x}[/]")`,
  `rich_eprintln!` and `rich_trace!` failed with "cannot find value `x`":
  `richf!` gave implicit captures the call-site span, which inside the print
  macros' `macro_rules!` wrapper is the wrapper's hygiene. Captures now take
  the template literal's span, as `format!` does.
- **Like-for-like flag suggestions.** `cli_doc::suggest` compares a `--long`
  typo only with long flags, a `-s` typo only with short flags and a bare word
  only with bare words, so `--paralel` no longer also suggests `-r`.
- **One location line.** A diagnostic with a location and a snippet of the same
  file no longer repeats `--> file` above the snippet; `DataError` parse errors
  show only `  --> file:line:col`. Snippet-only diagnostics are unchanged.
- **SVG titles.** `rich print '[b]Hi[/] there' --export-svg …` and `--rule` with
  markup titled the SVG with a fragment split on `/`; literal print text and
  rule titles now get the default title `rich`. Path, URL and stdin titles are
  unchanged.

### Core: Columns and Tree fit narrow widths

Both were found by the ext fuzzer and checked against rich 15.0.0.

- **Columns:** an item wider than the width sized its column to the whole item
  and showed only its first line, so `Columns(["supercalifragilistic"])` at 8
  cells printed 20. Item widths are now capped by `Measurement.get`, and the
  items are laid out in upstream's `Table.grid`, which wraps them or ends them
  with `…` (`superca…`). `Columns::equal` and `Columns::expand` are ported too.
- **Tree:** guides overflowed a narrow width, so a nested tree printed 8-cell
  lines at 3 cells. A label with no width left after its guides is now not
  rendered, guides included, as upstream renders nothing there.
- Goldens: `columns_long_*`, `columns_wrap_w13`, `columns_mixed_*`,
  `columns_equal*`, `columns_expand_w20`, `tree_deep_w{3,4,6,10,20}` and
  `tree_multiline_w{6,12}`.

### Core: markup in cells, labels and columns; measurement parity

Both were found while writing the user guide and checked against rich 15.0.0.

- **Markup in plain strings.** Upstream renders a `str` through
  `Console.render_str`, so `add_row("[b]x")`, `add_column("[b]Name")`,
  `Tree("[b]root")`, `tree.add(...)` and `Columns([...])` items parse console
  markup and emoji codes. The port printed `[b]x[/b]` literally. Strings are now
  a new `Cell::Markup` (also what `From<&str>`/`From<String>` produce), rendered
  through the new `Console::render_str`. A `Text` is never re-parsed, so
  `add_row_text`, `add_column_text` and `Cell::Text` stay literal.
- **Highlighting follows upstream:** off in table cells unless
  `Table::highlight(true)` or `column_highlight(true)`, off in tree labels
  unless `Tree::highlight(true)`, and the console default for `Columns` items.
- **Measurement.** `Table`, `Tree`, `Panel`, `Padding`, `Constrain`, `Align`,
  `Rule` (`1, 1`) and `Bar` now port their `__rich_measure__`. Before, they
  measured as the full width, so a nested table stretched its column and
  `Align` could not move a table. `Styled` and `Align` measure through
  `Measurement::get`. `Table` measures every cell, header included, with
  `Measurement.get` inside its padding, and uses upstream's `_get_padding_width`
  for fixed, clamped and ratio columns.
- **Panel:** `expand(false)`, `Panel::fit` and `width` are ported, so a fitted
  panel shrinks to its measured content and title.
- **Columns** take any cell: `Columns::from_cells` accepts markup strings,
  literal `Text` and renderables, with `equal` constraining renderables.
- **Table text cells** render their `Text` unstyled and apply the cell style
  afterwards, as `render_lines(style=...)` does. A span that matches the cell
  style now keeps its own segment (`[b]Name` in a bold header).
- **Migration:** a plain-string header, cell or tree label that must stay
  literal should be passed as `Text` or escaped with `markup::escape`.
  `Tree::new`/`add` take `impl Into<Cell>`. Ext's `derive` headers, precedence
  layer names, `rich_table!` and `rich_tree!` now pass `Text` and stay literal.
  CLI CSV headers and cells now parse markup and emoji codes, as rich-cli 1.8.1
  does (`table.add_row(*row)` with `str` cells). Malformed markup prints
  literally rather than raising (DIVERGENCES §2).
  The 0.0.11 ext views that show data (ANSI explain, capability, contrast,
  test and matrix reports) pass `Text` too, so `[info]` or `test_x[a]` stays
  literal.
- **QA measure checks** follow upstream's contract now that containers
  measure. An expanding `Panel` or `Table` takes the whole width whatever it
  measures (and an expanding table can give a column more than its
  `max_width`), as upstream's do. So when a render is wider than its measured
  maximum, fuzz, stress and lint render it again at that maximum and report a
  mismatch if it panics, overflows, or wraps onto more lines while it does
  not widen with more room.
- Goldens: `table_markup_*`, `tree_markup`, `tree_highlight`, `columns_markup`,
  `table_nested`, `table_renderable_cells*`, `table_tree_cell`,
  `columns_panels*`, `columns_tables`, `panel_fit_*`, `panel_width`,
  `align_table`, `align_panel_fit`, plus the new `highlight_renderables.tsv`
  (5 cases, highlighting console) and `measure_renderables.tsv` (18
  container measurements).


### Capabilities and accessibility (0.0.11 workstream 9)

- **Capabilities (#209).** `rich_ext::capabilities` detects colour depth,
  Unicode, OSC 8 hyperlinks, graphics protocol (kitty, iTerm, Sixel), size,
  interactivity and whether animation suits the output. It reads an
  `Environment` (the real one, or a deterministic map for tests), records where
  every value came from, and honours `RICH_COLOR`, `RICH_UNICODE`,
  `RICH_HYPERLINKS`, `RICH_GRAPHICS`, `RICH_SIXEL`, `RICH_ANIMATION`,
  `RICH_WIDTH` and `RICH_HEIGHT` overrides. It converts to the
  `TargetCapabilities` that `RenderTarget` uses. `rich doctor` now reports
  through it: a capability table with sources, and `capabilities` in
  `--report json`.
- **Graceful degradation (#218).** `Fidelity` runs Animated, Rich, Styled,
  Plain, Ascii and is selected from capabilities and a policy. `Degradable` and
  `Adaptive` let a renderable offer levels, and `Degrade` strips colour, styles
  or non-ASCII glyphs from any renderable while keeping cell widths.
- **Semantic text (#229).** `AccessibleText` gives screen readers and logs the
  content of a `Table`, `Tree`, `Panel`, `Rule`, `Text` or `Diagnostic` in
  reading order, without decoration.
- **Policies (#422).** `AccessibilityPolicy` (compact, screen reader, no
  animation, reduced motion, high contrast, monochrome) from `RICH_A11Y` and
  `NO_COLOR`, with themes, fidelity ceilings and status symbols that do not
  rely on colour (`✔ ok`, `[OK]`, `ok:`).
- **Theme checks (#307).** `check_theme` reports WCAG contrast failures
  against light and dark backgrounds, styles that become identical without
  colour, and pairs confusable under protanopia, deuteranopia or tritanopia
  (Viénot/Brettel simulation, CIEDE2000), with suggested colours and a JSON
  form behind the `serde` feature.
- **ANSI explain (#219).** `rich_ext::ansi_explain` decodes SGR, CSI, OSC
  (including OSC 8 links), DCS, ESC and control characters with their meanings
  and the visible text. `rich ansi explain [FILE|-]` prints it.

### Diff engine and `rich diff` for text (0.0.11 workstream 8)

- **Engine (#208).** `rich_ext::diff`: a linear-space Myers diff whose unified
  output matches GNU `diff -u` hunk for hunk, with word-level emphasis.
  `DiffView` renders text, ANSI captures (style-only changes shown with `~`) and
  render snapshots, unified or side by side, readable with colour off.
- **Source diffs (#223).** `SourceDiff` highlights both sides with `Syntax`,
  emphasises changed tokens, numbers lines and can link them to an editor.
- **Patches (#236).** `git::parse_unified` reads `git diff` output, including
  renames, copies, binary files and mode changes. `PatchView` shows a file tree
  with counts, highlighted hunks, inline annotations and links from a pluggable
  `LinkProvider`.
- **Test results (#237).** Behind `test-report`: JUnit XML (Surefire, pytest,
  jest-junit) and libtest JSON into one model, and `TestReport` renders failures
  first with a diff of expected against actual.
- **Assertions (#217).** With `testing`: `assert_rich_eq!`,
  `assert_rich_json_eq!`, `assert_render_eq!` and `assert_snapshot_eq!` panic
  with a rendered diff (side by side with `RICH_ASSERT_LAYOUT=side-by-side`).
  `RenderSnapshot::diff` now returns a unified diff.
- **QA tooling (#306, #308–#313, #225, #230).** `rich_ext::qa`, with `testing`:
  - `screenshot`: renders a fixture across widths, colour depths and Unicode
    modes and checks it against approved files (`RICH_APPROVE=1` accepts
    changes; `.new` files hold what changed).
  - `stress`: many widths and heights, reporting overflow, clipped content,
    unstable wrapping, panics and measure mismatches.
  - `lint`: clipped text, unknown style names in markup, broken links, status
    shown only by colour, and colours, glyphs or links the target cannot show.
  - `explain`: why a render wrapped, truncated, lost colour (`#ff8700 → 208 →
    …`), fell back to ASCII or dropped links.
  - `profile`: measure, render and frame times, with an opt-in counting
    allocator for allocation counts.
  - `fuzz`: seeded, reproducible random renderables checked for panics, width,
    determinism and measure bounds, with shrinking and a Rust reproduction.
  - `matrix`: the same fixtures across 16 capability profiles (colour depths,
    Unicode, links, dumb, CI, Windows Terminal, screen reader).
  - `bench`: a benchmark harness, a JSON run format (also read from criterion
    output) and `compare`, with sparklines. `rich bench compare BASE CAND`
    prints it and exits 5 on a regression.
  - The fuzzer found two core bugs, confirmed against rich 15.0.0: `Columns`
    overflowed with items wider than the width, and `Tree` guides overflowed
    below about 8 columns. Both are fixed below, and the fuzz test now covers
    every core renderable at every width.
- **CLI.** `rich diff` compares anything that is not an image pair as text, or
  renders one patch (`git diff | rich diff -`), with `--side-by-side`,
  `--context` and `--language`. `--threshold` counts changed lines and exits 5
  above it. Image pairs are compared perceptually as before.

### Ext: CLI authoring (0.0.11 workstream 6)

- **One description, many outputs.** `rich_ext::cli_doc::CommandSpec` describes a
  command line without depending on any parser: options with value hints,
  choices, defaults, environment variables and config keys, headings, examples
  and extra sections.
- **Help and errors (#38, #407).** `HelpView` renders grouped options with their
  hints, examples and sections, in two columns from 60 columns wide and stacked
  below that. `CliError` renders a diagnostic with "a similar argument exists"
  suggestions (Jaro-Winkler, as clap does).
- **Completions (#403).** `completion::generate` writes Bash, Zsh, Fish and
  PowerShell scripts with descriptions and groups. `CompletionCatalog` exposes
  the same metadata to other tools.
- **Markdown and man pages (#408).** `docs::to_markdown` and `docs::to_man` /
  `to_man_pages`; the man pages pass `mandoc -T lint` and `groff -ww`.
- **Configuration (#409, #413).** `ConfigReference` renders keys, types,
  defaults, environment variables and flags, with its sources in precedence
  order. `Precedence` shows every layer's value for each key, marks the winner
  and explains a single key's chain.
- **clap (#38).** The optional `clap` feature maps a `clap::Command` onto the
  model (`CommandSpec::from_clap`), maps `clap::Error` onto `CliError`, and
  offers parse helpers that render help, version and errors through rich.
- **CLI.** `rich --help` is now rendered from a `CommandSpec` of the whole CLI:
  usage first, options under headings (Render mode, Input, Layout, Image,
  Inspect, Export, Paging, Watch, Batch, Config & theme, …) with their
  defaults, choices, environment variables and config keys, wrapped to the
  terminal. Metavars read `<N>`. It is plain when piped and colour-free with
  `--no-color`.
  - `rich completions bash|zsh|fish|powershell` prints a completion script.
  - `rich docs markdown`, `rich docs man [--output DIR]` and `rich docs config`
    print reference pages; `docs/cli-reference.md` is generated from them.
  - `rich config reference` lists every config key. `rich config explain [KEY]`
    shows each key's value in every layer, in the order the binary applies
    them (defaults, `NO_COLOR`, config file, profile, command line), and marks
    the one that wins.
  - Drift tests keep the help and the parser in step, in both directions.
  - `completions` and `docs` are command words when they come first, like
    `config` and `doctor`; `rich -p docs` still prints the word.

### Structured data and `rich inspect` (0.0.11 workstream 4)

- **Ext: `rich_ext::data` (#37, #211, #384).** One insertion-ordered document
  tree for JSON, YAML, TOML, XML, INI and dotenv, built from text or from any
  `Serialize` value. Features: `data` (JSON, INI, dotenv and every view),
  `yaml` (saphyr-parser; anchors and aliases are kept), `toml`, `xml` and
  `jsonpath`. Parse errors carry a line and column and convert to a
  `Diagnostic`.
  - `Explorer` draws a tree or table with depth, length and string limits,
    folding and optional paths. `print_json`, `print_table` and `print_tree`
    render serde values with no setup, and `TableOptions` overrides columns,
    headers, justification and row limits.
  - YAML (#394), TOML (#395) and XML (#396) keep their structure: anchors,
    datetimes, and attributes as `@name` keys. INI and dotenv (#397) render as a
    key table with comments; `Redaction::secrets()` and the `Redactor` trait
    mask secret-looking values.
  - Selection (#398): the `Selector` / `SelectorBackend` traits and a
    `Selectors` registry, with an optional JSONPath backend (`$`, keys, indices,
    slices, wildcards, recursive descent and filters).
  - Flatten and unflatten (#399), with errors for ambiguous input; key, path
    and value search with highlighted matches and context (#400); and `diff`.
  - `Format::detect` is conservative: prose, Markdown, CSV and single lines are
    not mistaken for YAML, TOML, INI or dotenv.
- **CLI: `rich inspect` (#211).** Explores a file, URL or stdin as a tree; INI and
  dotenv as a key table. `--select`, `--find`, `--flatten`, `--table`,
  `--max-depth`, `--max-length`, `--show-paths`, `--redact` and `--compare`
  change the view. A parse error reports `file:line:column` and exits 4.
- **CLI: `--format` (#402).** `--format auto` detects piped or extensionless
  input and routes it: JSON to the JSON renderer, other formats to highlighting,
  anything else to plain text as before. A named format overrides the
  extension. Detection is opt-in, so upstream's default is unchanged. `format`
  is also a config key, and is ignored when another mode is chosen explicitly.

### Progress: pulse, format columns and live display (#6)

- **Pulse bar.** `ProgressBar` pulses when `pulse` is set or the total is
  unknown: `ProgressBar::indeterminate`, `.pulse`, `.animation_time`. It uses
  upstream's cosine fade, including the quirk that 256-colour consoles get the
  two-tone fallback. It also has the ASCII (legacy/`ascii_only`) glyphs and
  no-colour fallbacks. The bar column pulses for unstarted and indeterminate
  tasks.
- **Determinate bar fixes.** A zero total draws a full finished bar, and the
  background is left out without colour, as upstream does.
- **Text and renderable columns.** `TextColumn` formats strings against the
  task (`{task.completed:>6.1f}`, `{task.fields[name]}`) through the new
  `rich::pyformat`, a port of Python's format mini-language. It also takes
  style, justify and markup. Per-task fields come through
  `Progress::add_task_with` and `TaskUpdate::field`. `ProgressColumn::Renderable`
  shows any renderable, and rows grow to fit multi-line cells.
- **Live display.** `Progress::start(console, writer, refresh_per_second)`
  returns a `LiveProgress`. It redraws on the `Live` thread and refreshes
  where upstream does: `add_task`, `reset`, and `update(refresh=True)`.
  `stop()` commits the final frame.
- **`track()`.** `LiveProgress::track` and the module-level `rich::track`
  advance a task per item, after the loop body, as upstream does.
- **Live changes.** `Live::spawn` waits for the first frame. The new
  `AutoLive::refresh_wait` redraws synchronously. The final newline is written
  only after a non-empty render.
- **API changes.** `TaskUpdate` gains `fields` and `refresh` (struct literals
  need `..TaskUpdate::default()`). `ProgressColumn` gains `TextFormat` and
  `Renderable`. New `Console::no_color()`.
- **Goldens.** New `progress_bar.tsv` (15 cases) and `progress_live.tsv`
  (3 cases), plus 4 more `progress_time.tsv` cases: pulse, format fields and
  renderable columns. All match rich 15.0.0.

### Progress: the grid, expand, column options, transient and file reading (#6)

- **Rendered as upstream's grid.** `Progress::make_tasks_table` builds the
  `Table::grid` that upstream's `make_tasks_table` does, and the display renders
  it. Columns wrap at narrow widths instead of cropping.
- **`expand`**, **`transient`** and **`disable`** on `Progress`.
- **Column options.** `ProgressColumn::with_table_column(ColumnOptions)` is
  upstream's `table_column=Column(...)`: width, min/max width, ratio, justify,
  `no_wrap`, overflow and style. `ProgressColumn::BarWith(BarColumn)` sets the
  bar width (`None` fills the column) and its styles.
- **Reading files.** `LiveProgress::wrap_read` and `LiveProgress::open` port
  `wrap_file` and `open`: the returned `ProgressReader` advances its task by
  the bytes read.
- **Table.** Cells may be any renderable (`Cell`, `Table::add_row_cells`), and
  `Table::add_column_with(header, ColumnOptions)` sets every column option.
  `ProgressBar` measures as upstream's does and gains style setters.
- **Live.** `Live::transient` and `Live::spawn_with(…, transient)`. A
  non-terminal stop no longer adds a newline; `LiveProgress::stop` writes it,
  as `Progress.stop` does.
- **Migration.** Exhaustive matches on `ProgressColumn` need the `BarWith` and
  `WithTableColumn` variants.
- **Goldens.** 6 new `progress_time.tsv` cases (expand, flexible bar, column
  options, crop and style, narrow wrapping) and 4 new `progress_live.tsv` cases
  (transient, disabled, non-terminal) match rich 15.0.0.

### New crate `rs-rich-macros` 0.0.1 and ext macros (0.0.11 workstream 5)

- **`rs-rich-macros`** is a proc-macro crate, used through `rs-rich-ext`'s new
  `macros` feature. It checks markup and styles with the core's own parsers.
  - **Checked markup (#36, #282, #283).** `richf!` builds a `Text` from
    `format!`-style markup and escapes the values. Unbalanced, mismatched or
    unclosed tags, unknown style names and unknown theme keys are compile
    errors, as are malformed or unused placeholders. `style!`, `theme_key!` and
    `markup!` check literals.
  - **Derive (#280, #281).** `#[derive(Rich)]` implements `RichRecord` and
    `Renderable`: labelled fields, a panel, or a table row, with field options
    `skip`, `label`, `style` (checked), `display`, `format`, `justify` and
    `order`. `rich_ext::derive::table` renders many records as rows.
- **Print and debug macros (#284, #285).** `rich_dbg!` is `dbg!` rendered through
  `Pretty`. With `macros`, `rich_println!`, `rich_eprintln!` and `rich_trace!`
  print checked markup.
- **Convenience macros (#385).** `rich_table!`, `rich_panel!`, `rich_tree!` and
  `rich_progress!` build the ordinary core types.
- **Tests.** `trybuild` compile-fail cases pin the error messages.
- **Release tooling.** The tooling knows the fifth crate: `release.py`, the
  release tag triggers, the CI feature matrix and the release tests.
  Publication order is core, macros, ext and art, then CLI.

### Ext: diagnostics, hyperlinks, stack traces and a dashboard (0.0.11 workstream 3)

- **Diagnostics (#210).** `Diagnostic` gains a `Level` and code
  (`error[E0308]: …`, the code optionally linked), a `Location` row, labelled
  primary and secondary spans on `SourceSnippet`, `Suggestion`s that show the
  edited line, and an attached `StackTrace`. Without them it renders exactly as
  before.
- **Error adapters (#386, #387).** `DiagnosticInfo` lets any error type, such as a
  `thiserror` enum, supply its level, code, help, notes and location through
  `Diagnostic::from_info` / `to_diagnostic`. The new `anyhow` feature adds
  `Diagnostic::from_anyhow`, which maps the context chain and a captured
  backtrace.
- **Hyperlinks (#212).** `hyperlink::Hyperlinker` links URLs, paths,
  `path:line:column` and `#123` / `owner/repo#123` references, as `file://` URLs
  or through an editor template. It is a `Highlighter`, so `RichHandler` can use
  it. Non-terminals keep the plain text.
- **Stack traces (#239).** `stacktrace::parse` normalises Rust, Python, Java and
  JavaScript traces, with chained causes, into one shape. Parsers are pluggable.
  Rendering dims and collapses library frames and links locations.
  `stacktrace::capture` and `panic_hook` cover Rust panics. The tests parse traces
  captured from real Python 3.11, Node 22, OpenJDK and Rust runs.
- **Dashboard (#329).** `dashboard::DiagnosticsDashboard` summarises counts by
  level, lists the most frequent codes, and groups diagnostics by file in
  location order.
- `rs-rich-ext` now depends on `fancy-regex` directly. It was already in the tree
  through core.

### Logging: `LogRender` port and `RichHandler` (#10)

- **`LogRender` is now a port of `_log_render.py`.** It lays a record out as a
  `Table::grid` row: time (blanked when it repeats the previous record's),
  level, message (ratio 1, folding) and `path:line` linked to the file. It
  remembers the last time across calls, as upstream's instance does.
  `rich::level_text` builds upstream's padded `logging.level.<name>` column, and
  `LogLevel` spells `WARN` as Python's `WARNING`.
- **Migration.** The old one-line `LogRender::new(level, message).time(..).path(..)`
  is now `LogRecord`, with the same builders plus `.line(n)`. `LogRender::new()`
  takes no arguments; call `.render(console, message, time, level, path, line,
  link_path)` for each record.
- **Table.** `Table::grid()` now has upstream's defaults: no padding and
  `collapse_padding`. New `Table::without_box()` (upstream `box=None`),
  `Table::padding(top, right, bottom, left)` and `column_overflow`; a
  column's overflow applies to cells that set none.
- **JSON floats** are written with Python's `float.__repr__` (`1e+20`, `1e-07`,
  `10000000000.0`), as `json.dumps` does. This resolves DIVERGENCES §8.
- **Ext: `RichHandler`** (`rich_ext::RichHandler`) is the counterpart of
  upstream's `logging.RichHandler`. It prints events with the level column,
  `ReprHighlighter`, HTTP-method keywords in `logging.keyword`, optional
  markup, the file name linked to its full path, and structured fields as
  `key=value`. With the `log` or `tracing` feature it is an `EventSink`, so
  `LogAdapter` and `EventLayer` print through it. See the `rich_handler` example.
- **Goldens.** New `log_render.tsv` (4 cases) and `json_python_floats` match
  rich 15.0.0. The differential fuzzer now generates `box=None` tables.

### Markdown: styled table cells and constructor options (#9)

- `Table` headers and cells can be styled `Text`, via the new
  `Table::add_column_text` and `Table::add_row_text`. A cell's own `justify`,
  `overflow` and `no_wrap` override the column's, as `Text.__rich_console__`
  prefers them.
- Markdown table cells now carry their strong, emphasis, code, strike and link
  runs, as upstream's `TableDataElement` appends them under the current style.
  Headers keep `markdown.table.header` over their inline spans.
- A single `~` inside a table cell is appended to the cell, like other literal text.
- `Markdown` gains upstream's constructor options: `justify`, `style`,
  `code_theme`, `inline_code_lexer` and `inline_code_theme`. `style` is the root
  of the style stack, as upstream's context uses it: under every text run,
  under list-item content and padding, and under the block-quote colour. Code
  themes are `syntect` names (DIVERGENCES #18).
- New `Syntax::highlight` returns the highlighted code as `Text` (upstream
  `Syntax.highlight`); inline code uses it when a lexer is set.
- Fix: a wrapped list item's continuation rows are padded in the marker's own
  style (bold bullet, cyan number), as upstream's `render_bullet` and
  `render_number` do.
- New goldens: `markdown_table_inline.tsv` (8 cases) and
  `markdown_options.tsv` (8 cases: justify, style, and wrapped items) match
  rich 15.0.0.

### Core parity fixes (0.0.11 workstream 1)

This fixes every divergence family the 0.0.10 differential fuzzer found, each
verified against rich 15.0.0. The known-divergence queue is now empty, and every
repro is in the pull-request corpus.

- **Padding (#442):** empty content keeps its line, and the padding style sits
  under the content, as upstream's `render_lines(style=…)` does. The same fix
  gives `Panel("")` its blank body row.
- **Align (#443):** the rendered block is aligned as a whole
  (`Constrain` + `Segment.set_shape`), not line by line.
- **Rule (#444):** a right-aligned title with a multi-cell fill is dropped, as
  upstream drops it.
- **Table (#445):**
  - header cells are bottom-aligned, and row shaping follows upstream's
    `align_cell` + `set_shape`
  - cells render through `Text` wrap, justify and truncate
  - `_measure_column` is ported, including columns with no cells, the
    `maximum or 1` floor, the re-measure after collapsing, and no expand once a
    table has collapsed
- **Printed `Text` (#446):** `Console::print` renders at the full width and
  applies upstream's `Text.join` semantics, so a printed `Text`'s own `justify`,
  `overflow` and `no_wrap` defer to the print options. The top-level
  shrink-to-measurement that stood in for this is gone.
  - `Renderable::fit_to_measurement` now defaults to `false`.
  - New `Console::print_with` and `render_export_with` take explicit options, the
    equivalent of `console.print(…, overflow=…, no_wrap=…)`.
- **Tabs (#447):** `Text` measures the raw string, as upstream does. A tab
  measures zero cells until render.
- **Emoji (#448):** scanning resumes after every `:…:` match, including unknown
  codes and `::`, as upstream's single `re.sub` does.
- **Zero-width content (#449):** a renderable given less than one cell renders
  nothing (upstream `Console.render`), so a squeezed `Panel` has no body rows.
- **API:** `Console::render_lines_styled` ports `render_lines(style=…)`.

Before the fix, three seeds of 2,000 generated cases had 1,193 mismatches.
After it, 40,000 generated cases across 20 seeds match.

## Core 0.0.6 / ext 0.0.8 / art 0.0.8 / CLI 0.0.10 — published 2026-09-23

Every workstream in the [0.0.10 plan](docs/plans/0.0.10.md) is merged to `main`, and the
release test passed on the integrated tree; see the
[0.0.10 release notes](docs/releases/0.0.10.md). All four crates were published through
Trusted Publishing, each with a passing exact-version registry consumer:
[core](https://github.com/buchochelliq-labs/rs-rich-cli/actions/runs/35818053978), [ext](https://github.com/buchochelliq-labs/rs-rich-cli/actions/runs/35818015980), [art](https://github.com/buchochelliq-labs/rs-rich-cli/actions/runs/35818038760) and
[CLI](https://github.com/buchochelliq-labs/rs-rich-cli/actions/runs/35818067984). The tags were first pushed out of order; the release notes
record the recovery. Each version was uploaded once.

- Release tooling: `check_packages.py` drops Cargo's cached copies of staged-only
  versions before verifying. Cargo reused a core 0.0.6 unpacked and built before
  `fit_to_measurement` existed, which failed a good tree and could pass a bad one.
- CLI: a multi-file `--watch` with `--watch-exit-on-error` no longer promises a retry
  in the failing region.
- Demo: the tour shows a pushed theme, `~~~` strikethrough and a two-file watch that
  ends through `--watch-exit-on-error`; it is re-recorded from the 0.0.10 build.

- Release: crates.io Trusted Publishing replaces the stored registry token; the
  OIDC exchange runs only after preflight and dry run (`docs/BRANCHING.md`).
- CI: GitHub Actions moved to Node 24 majors (`checkout` v7, `github-script` v9,
  `upload-pages-artifact` v5, `deploy-pages` v5).
- Art: `icy_sixel` 0.7 for the optional Sixel backend.
- CLI: `toml` 1.1 for configuration parsing; strict-config behaviour unchanged.
- Core parity (#15): `Spinner` follows upstream's animation state. The first
  `render` fixes the start time, `Spinner::update` changes text, style or speed
  (a new speed continues from the current frame), the text is console markup,
  and the frame style may be a theme name. `Status` keeps one stateful spinner,
  parses its message as markup, defaults to the `status.spinner` theme style and
  gains `update`; `Status::renderable` now returns `&Spinner`.
  `ProgressColumn::Spinner` delegates its start time to the spinner, as
  upstream's `SpinnerColumn` does. The new golden `live_status.tsv` has 8 step
  programs covering spinner and status frames and LiveRender control sequences;
  PORTING gains parity cells for Live and progress.
- Core: Progress time, rate and spinner columns and upstream's task model (#6):
  an injectable clock, `add_task` returning a `TaskId`, `update`/`advance`/
  `reset`/`start_task`/`stop_task`/`remove_task`, the 30 s speed window, and
  `TimeElapsed`, `TimeRemaining`, `TransferSpeed`, `FileSize`, `TotalFileSize`,
  `Spinner`, `TaskProgress` and binary `Download` columns. Golden
  `progress_time.tsv` replays identical step programs against rich 15.0.0.
  `Progress::new()` now uses upstream's default columns (adds time remaining);
  `add_task` takes `impl Into<Option<f64>>` totals and returns a `TaskId`.
  Progress cell styles now resolve against the console theme.
- CLI: the capability demo's progress section shows speed, ETA, elapsed and a
  spinner from a simulated clock.
- Ext: `Diagnostic::from_error` no longer reports an ordinary error chain as a
  `[cycle]` when a wrapper stores its source as its first field (same address);
  cause identity now compares address *and* type (#146, #151). New regressions
  cover a three-level chain, and checked-in `RenderSnapshot` fixtures cover
  multiline, chained and source-context diagnostics. A nested-panel layout
  regression evidences #134.
- Ext (behaviour change): `LayoutNode` leaves now receive their region's height,
  as upstream `Layout` passes it, so height-aware renderables such as `Panel`
  fill their region instead of rendering at natural height above blank rows.
  The nested-panel regression is byte-identical to rich 15.0.0's `Layout`.
  Wrap a leaf in `.content_height()` to keep a panel at its natural height.
- Core: `Syntax` and `Json` port upstream `__rich_measure__` (Syntax measures its
  raw source plus padding; JSON measures as its `Text`), with `Measurement::get`,
  `normalize` and `with_maximum`, parity-tested by the new `measure.tsv` golden.
  A printed `Syntax` still renders at the full console width, as upstream does:
  the new `Renderable::fit_to_measurement` (default `true`) opts it out of the
  top-level shrink that stands in for upstream's `str`/`Text` joining.
- Ext: `layout::Overflowing` applies one explicit `OverflowPolicy` (wrap, fold,
  crop, ellipsis, visible) to Syntax, JSON or Text lines; fitting output is
  unchanged byte for byte, and padded Syntax rows keep their background (#149).
- Art 0.0.8 image modes (#125, #126, #199) and CLI routing (#144):
  - `ImageColorMode::Ansi16` (rich's standard palette) and `Grayscale` (neutral
    ANSI256 entries by luma). Floyd–Steinberg and Bayer 4×4 now work with every
    quantized mode.
  - `ImageMode::Quadrants` / `QuadrantArt`: 2×2 pixels per cell, choosing the
    cheapest two-colour split. Also available for `--diff` heatmaps.
  - `ImageFit::Stretch`, `ImageArt::max_width`/`max_height`, and brightness,
    contrast and gamma in `ImageTransforms`, applied in a documented fixed order.
  - CLI flags `--image-color ansi16|grayscale`, `--image-mode quadrants`,
    `--image-fit stretch`, `--image-max-width`, `--image-max-height`,
    `--image-brightness`, `--image-contrast` and `--image-gamma`, plus matching
    config keys. Invalid values and unsupported combinations are usage errors.
  - Unset options leave output byte-identical: 149 pre-existing mode, colour,
    dither, fit and transform invocations compared equal against the previous
    binary, stdout plus HTML and SVG exports.
  - Migration: exhaustive matches need `ImageMode::Quadrants`,
    `ImageFit::Stretch`, `ImageColorMode::{Ansi16, Grayscale}` and
    `ImageArtError::InvalidAdjustment`. `ImageTransforms` gained three `f32`
    fields, so it is no longer `Eq`, and struct literals need `..Default::default()`.
  - The guided demo's art section and a same-source comparison image
    (`docs/media/cli-010-image-modes.png`) show the new modes from actual output.
- CLI: `--watch` accepts several local files; a change re-renders only that
  file, in its own `rich-ext` Live region, with errors shown per file until it
  recovers. File events come from `notify` (parent-directory watches, so atomic
  rename-over saves and delete-and-recreate are seen), debounced by
  `--watch-debounce` (default 0.1 s); `--watch-poll` and watcher failures use
  the polling loop. `--watch-exit-on-error` ends the watch non-zero on a failed
  render. New config keys: `watch_debounce`, `watch_poll`,
  `watch_exit_on_error`. Redirected output and URL watching are unchanged (#139).
- Parity tooling (#34): `scripts/diff_rich.py` generates Table, Rule, Padding
  and Align cases alongside markup, text and panels. The Python oracle renders
  each colour system in its own interpreter, because rich memoises a Style's
  escape codes and a shared process misreported colours. Markup compares the
  strict parser on both sides. The shrinker keeps the failure kind and reduces
  rows, cells, columns and options. A nightly workflow runs 20,000 generated
  cases on `main`. First findings are filed as #442–#449, with repros in
  `scripts/fixtures/diff_rich_known.jsonl`; triage steps are in `docs/parity.md`.
- Core: Markdown strikethrough pairs tilde runs as upstream's markdown-it does, so
  runs of three or more (`a ~~~x~~~ b` → `a ~` + struck `x` + `~ b`) and uneven
  runs match rich 15.0.0. Golden `markdown_strike.tsv` (24 cases); DIVERGENCES §21
  narrowed to tilde pairs crossing a later emphasis span (#9).
- Core: upstream's theme stack — `Console::push_theme`, `pop_theme` and a
  `use_theme` guard (`ThemeContext`) that pops on drop — plus `Theme::from_styles`,
  `config`, `from_file` and `read` for upstream theme files. Golden
  `theme_stack.tsv` checks them against rich 15.0.0; DIVERGENCES §14 resolved (#3).
  `RichError` gains `ThemeStack` and `ThemeConfig`; exhaustive matches need them.

## CLI 0.0.9 / art 0.0.7 — published with the cohort above

- Named TOML themes, default/profile/CLI selection, explicit style overrides and
  resolved theme bindings for batch workers and exports.
- Human-report stderr-terminal batch progress and `--no-progress`; Ctrl+C stops
  scheduling, kills/reaps workers, exits 130 and preserves one machine report.
- Listable `core`, `workflows`, `art` demo sections and selectable playback.
- Read-only `rich doctor` diagnostics with JSON stdout, selected config/pager
  details and explicit inferred capability reporting.
- Public art `ImageColorMode`/`Dither` builders and CLI opt-in ANSI256/
  Floyd–Steinberg for ASCII/half-block still images. Default truecolor/no-dither
  and the `ImageOptions` struct shape remain unchanged.

Local validation and review passed; see [0.0.9 preparation notes](docs/releases/0.0.9.md).
The expanded scope above also moves core to 0.0.5 and ext to 0.0.7; all four published on 2026-09-22.

## CLI 0.0.8 / art 0.0.6 — published

- Guided `rich --demo` suite tour with adjustable pacing, real CLI workflow and
  art examples, finite redirected output and clean Ctrl+C interruption.
- Batch planning also rejects hard-linked input/output aliases before writing.

- Full TOML configuration with strict validation, default/profile/CLI precedence,
  explicit boolean overrides and JSON `config show` / `config validate`.
  Inspection lists configured settings, not every built-in default.
- Batch dry-run without output writes; bounded subprocess concurrency for file
  exports with disk-spooled output replayed in input order. Fail-fast stops new
  scheduling while in-flight workers finish; terminal batches remain serial.
- Opt-in terminal-height automatic paging and explicit no-pager controls.
- Nine public `ImageAnchor` positions and an `ImageArt::anchor` builder, exposed
  by `--image-anchor` for cover fitting. Center remains the default and contain
  behavior is unchanged.
- Registry package-content checks and staged sibling package verification;
  staged success does not establish publication readiness. Updated recipes,
  benchmark/demo work and release handoff documentation.

Independent publication succeeded for
[art 0.0.6](https://github.com/buchochelliq-labs/rs-rich-cli/actions/runs/35541438821)
and [CLI 0.0.8](https://github.com/buchochelliq-labs/rs-rich-cli/actions/runs/35542000274),
including exact-version registry consumers. Ext 0.0.6 and core 0.0.4 stayed
unchanged. Evidence is recorded in [0.0.8 notes](docs/releases/0.0.8.md).

## CLI 0.0.7 / art 0.0.5 / ext 0.0.6 — published

The art release succeeded first. The initial CLI package dry run failed because
registry ext 0.0.5 lacked the workspace `CliExtensions: Clone` implementation.
Publishing ext 0.0.6 and correcting the CLI dependency completed recovery:
[art workflow](https://github.com/buchochelliq-labs/rs-rich-cli/actions/runs/35531841822),
[ext workflow](https://github.com/buchochelliq-labs/rs-rich-cli/actions/runs/35533288812),
[CLI workflow](https://github.com/buchochelliq-labs/rs-rich-cli/actions/runs/35533304887).
Core remained 0.0.4.

### CLI finishing

- Fix watch refresh after same-size edits/atomic saves with preserved timestamps;
  read local contents using bounded memory on each poll. Repaint the terminal
  viewport instead of appending each changed frame. Redirected watch stays finite.
- Add `ImageFit::{Contain,Cover}` and `ImageArt::background` for still-image
  letterboxing, centre-cropping and alpha compositing; expose CLI fit/background
  options with positive-dimension and allocation-limit validation.
- Add real CLI workflow snapshots and terminal regressions, fresh demo capture
  tooling, workflow recipes, and truthful serial `--jobs` help.

### Release scope

CLI 0.0.7 and art 0.0.5 shipped still-image fitting, background compositing,
watch fixes, deterministic serial batch conversion and scalar config profiles.
The [0.0.7 release notes](docs/releases/0.0.7.md) record validation and dependency
recovery. Serial `--jobs` and limited TOML/boolean semantics below describe that
release; the published 0.0.8 release above replaces them.

### Added

- **Confidence tooling:** added `scripts/snapshot_cli.py`, a deterministic CLI
  snapshot runner with injectable width and terminal capability profiles,
  PTY terminal forcing for color profiles, option-terminator (`--`) positioning,
  newline normalization, and readable unified diffs. The differential corpus and
  generator now cover box renderables (`Panel`) and safe-box capability
  profiles, and `library_bench` accepts explicit `--width` and `--color-system`
  values.
- **`rs-rich-cli` 0.0.7 workstream:** deterministic `--batch` conversion for
  files, directories, and globs with deterministic serial execution, explicit collision and
  overwrite policy, fail-fast/continue-on-error modes, and aggregate JSON
  status reports. `--jobs` reserves a future concurrency limit; it does not
  enable parallel workers. Added TOML-style scalar config profiles with local/home discovery,
  explicit `--config`/`--profile`/`--no-config`, and CLI-over-config precedence.

- **`rs-rich-cli`:** added task-oriented subcommands (`rich json`,
  `rich markdown`, `rich syntax`, `rich csv`, `rich ipynb`, `rich jsonl`,
  `rich log`, `rich gif`, `rich diff`, `rich rule`) while preserving existing
  flat flags without deprecation warnings.
- **`rs-rich-cli`:** added streaming JSONL / NDJSON and structured-log rendering
  from stdin or files with bounded per-record processing and fail-fast malformed
  record handling.
- **`rs-rich-cli`:** added stable automation exit-code classes plus
  `--report json` / `--machine-json` for a common result/error envelope on
  stderr, keeping rendered stdout separate.
- **`rs-rich-cli`:** added non-busy `--watch` polling for local files and
  fetch-enabled URLs, including atomic-save/disappearance recovery, parse-error
  frames, configurable intervals, URL response caching, and deterministic
  one-shot behavior when stdout is redirected.
- **`rs-rich-art`:** added an `ImageArt` facade (`ImageMode`, `ImageOptions`,
  `RenderCapabilities`) plus a reusable Braille renderer so consumers can
  render a single still image without duplicating renderer selection logic.
- **`rs-rich-cli`:** added a first-class `--image` flag / `rich image` command
  for rendering a single still image (reusing `rich-art`'s `ImageArt` facade),
  plus a new `--height N` option; `--image-mode` now also applies to `--image`
  and accepts `braille`.

## [0.0.5] — 2026-09-13

Prepared for independent `rs-rich-ext-v0.0.5` and `rs-rich-cli-v0.0.5` tags.
`rs-rich` and `rs-rich-art` remain at 0.0.4.

### Changed

- **Parity tooling:** `scripts/capture_golden.py` now verifies that the
  installed Python `rich` version matches the exact `UPSTREAM.toml` pin before
  regenerating fixtures, preventing silent captures from the wrong oracle.

- **Differential testing:** added a replayable `scripts/diff_rich.py` corpus
  harness and Rust `diff_render` probe that compare selected markup/style/Text
  cases against pinned Python `rich`, with bounded mismatch shrinking and CI
  coverage for the deterministic corpus.

- **Benchmarking:** added a `library_bench` Rust probe for repeatable markup
  parsing, Text wrap/justify, Table layout and Console-render timings, including
  setup timing, output hashes and active feature metadata.

### Added

- **`rs-rich-ext`, `rs-rich-cli`:** added an opt-in `--sanitize` path that
  neutralizes terminal controls from decoded input, JSON/notebook strings,
  titles and captions by rendering them as visible inert text while preserving
  default upstream-compatible output.

## [0.0.4] — 2026-09-10

All four independently versioned packages published at 0.0.4 for their code and
internal dependency changes. The annotated tag is on main; the protected release
workflow passed full source checks and exact-version registry verification.

- `rs-rich`, `rs-rich-cli`: optional, off-by-default `syntax-cache` for repeated
  lines matching a small initial parser state. No per-entry capture copies;
  grammars and output remain unchanged, with workload-dependent gains (#45).

- `rs-rich`: ownership-transfer protocol for table rows, preserving existing rendering.
- `rs-rich-cli`: move parsed CSV rows into the table and compact completed rows;
  measured 100k-row peak RSS falls about 46%, with identical output (#74).

- `rs-rich`: translate successive Text wrapping breaks incrementally, removing
  repeated scans of long UTF-8 line prefixes while preserving wrapping output (#74).

- `rs-rich-art`: explicit capability-aware half-block GIF frames and mixed-renderer stages.
- `rs-rich-cli`: `--gif-mode ascii|blocks`, preserving ASCII defaults and safe
  ASCII fallback without color and first-frame output when redirected (#65).

- `rs-rich-ext`: explicit strict UTF-8/UTF-16 decoder with BOM and byte-order validation.
- `rs-rich-cli`: wire `--encoding` for text files, stdin and URLs; preserve default
  decoding, add UTF-16 hints and clean actionable image errors (#62).

## [0.0.3] — 2026-09-10

### Fixed
- **`rs-rich-cli` diff thresholds:** compare both percentages with the same
  one-decimal formatting used by the report, including fractional limits and
  rounding ties, so the displayed verdict agrees with the process exit status.
- **`rs-rich-cli`:** render diff HTML/SVG using destination color capabilities
  while preserving plain piped stdout and threshold exit codes. Apply notebook
  decorators to the whole cell group; reject ignored demo options, explain
  interactive stdin, and document environment, paging and GIF repeat defaults.
  Honor non-empty `NO_COLOR` in all CLI modes.
- **`rs-rich`:** parse Panel, Rule and Table title/caption markup with visible
  width measurement; preserve styled Unicode truncation without invalid span
  offsets. New fixtures are captured from pinned Python Rich 15.0.0.
- **`rs-rich`:** select `more.com` as the Windows pager fallback. Required Windows
  CI launches the native pager and verifies its output as well as env precedence.
- **`rs-rich`, `rs-rich-cli`:** expose table streaming through the `LineRenderable`
  protocol trait; treat an early-closing CSV consumer as successful termination.
  Regression test covers a 10,000-row producer whose pipe is closed after one byte.
- **`rs-rich`, `rs-rich-cli`:** preserve arbitrary-size JSON integers and render
  overflowing exponents as signed Infinity, matching Python. Repair #98 escape
  folding without losing suffix bytes; keep the divergence behind the off-default
  `json-escape-safe` feature. Default golden fixtures are captured from Python.
- **`rs-rich-art`, `rs-rich-cli`:** piped GIFs emit their first frame once, including
  infinite repeats, without cursor controls. Reject unsupported GIF decorators,
  paging and exports; explain image-mode downgrades on stderr (#59, #72).
- **`rs-rich`, `rs-rich-cli`:** stream undecorated CSV rows to reduce peak memory;
  preserve upstream alignment-flag priority and notebook display-data omission.
  Fix empty Markdown output, HTML/quoted-rule spacing, and adjacent images in one
  table cell (#72, #74). Empty text and CSV retain their newline.
- **`rs-rich` (and rendering through `rs-rich-cli`): Markdown image hoisting is byte-parity in nested and adjacent containers**
  (`markdown.rs`): images inside table cells are now hoisted ahead of the table
  and leave their cells empty, multiple images in one container share a row, and
  images hoisted from consecutive containers occupy adjacent rows without a blank
  row. Goldens cover a table cell, a multi-image paragraph, and a README-style
  badge table against Python `rich` 15.0.0.

### Changed
- **`rs-rich`, `rs-rich-ext`, `rs-rich-art`, `rs-rich-cli`:** set 0.0.3
  manifests and internal requirements together. Core includes the Markdown fix;
  ext needs a new package version for its dependency on core 0.0.3. Art and CLI
  retain their 0.0.3 versions. This dependency closure is specific to this release;
  independent per-crate versioning remains supported.
- **Release tooling (all four crates):** retain coordinated `v*` releases and
  independent `<crate>-v*` releases, with selection-scoped exact-version
  verification. Protect registry consumer builds with the same `crates-io`
  environment as upload, with a manual verification-only recovery mode. Reject
  lightweight tags and require the tag's exact checked-out commit on main. Align the
  release checklist and skill with both paths.
- **CI and docs (all four crates):** parse the upstream pin as TOML, check
  generated manifest versions and CLI help, and apply main-ancestry checks to
  `rc/*`, `release/*`, and `releases/*` integration branches.

## [0.0.2] — 2026-08-11

### Fixed
- **`Color::downgrade` was wrong for 8-bit and standard targets** (`color.rs`) —
  found by the new colour-system goldens, which is exactly the coverage gap that
  let it survive:
  - **8-bit:** we searched the 256 palette for the nearest colour. Upstream
    instead maps into the 6×6×6 cube *by formula* (with a greyscale-ramp branch
    under 15% saturation), which picks different entries: `#00ff00` is cube index
    46, not the exactly-matching system green 10. The axis is also non-linear
    (first step 0–95, then 40 per step), and Python's banker's rounding is
    reproduced with `round_ties_even`.
  - **standard:** we matched against the 128-based ANSI table. Upstream matches
    against a *separate* 170/85-based `STANDARD_PALETTE`, so `#ff0000` gives 1
    (maroon), not 9. The two tables are now distinct: `ANSI_BASE_PALETTE` (the
    first 16 of the 8-bit palette, and the default terminal theme's ANSI set)
    versus `STANDARD_PALETTE` (matching only).

  A unit test had been asserting the buggy values; it now asserts upstream's,
  captured from real rich 15.0.0.

### Fixed
- **Four more defects**, from a second adversarial review (of the change above):
  - **Tabs were never expanded on the render path.** `Text::expand_tabs` existed
    but nothing called it, so a tab occupied one cell in every layout calculation
    and eight on the terminal. `Text::measurement` now measures the *expanded*
    text too — upstream gets away with measuring raw (a tab counts as zero cells
    there) only because its `print` never narrows to the measurement.
  - **A bare `link` was accepted** and emitted an empty OSC 8 hyperlink; upstream
    raises `"URL expected after 'link'"`. This one was mine, from the `link`
    keyword added in the previous commit.
  - **Colour names kept their original case**, so `on #FF0000` did not normalize
    to `on #ff0000` and a mixed-case open/close tag pair raised.
  - **`Text::new` did not strip control codes** (BEL, backspace, vertical tab,
    form feed, carriage return), which upstream removes on construction.
- **Six markup and `Style::parse` defects**, all found by an adversarial review of
  the named-span change and each verified against real rich 15.0.0:
  - **The highlighter beat markup.** `[green]123[/]` rendered `repr.number` cyan
    instead of green, because markup spans were pushed first and the highlighter
    decorated that `Text` in place. Upstream highlights the plain string and
    appends the markup spans last, so an explicit tag wins.
  - **Two tags over the exact same range combined backwards**, so `[red][blue]x[/][/]`
    came out red. Upstream's `sorted(spans[::-1], key=start)` needs both the
    reversal *and* a start-only key; sorting by `(start, end desc)` makes an
    identical range compare equal and hands the win to the outer tag.
  - **`expand_tabs` kept one span** where upstream splits per tab-part, so a span
    crossing several tabs rendered as one segment instead of several — same
    colours, different bytes.
  - **Tag names weren't normalized.** `Style::normalize` is now ported (with
    `Style::definition`, the port of `Style.__str__`), so `[b]` opens what
    `[/bold]` closes and a mixed-case name reaches the theme lowercased.
  - **`[link=url]` was unusable**: parameters weren't split from the tag name, so
    `[/link]` raised a markup error. `Style::parse` also had no `link` keyword,
    which meant such a style resolved to null and dropped the hyperlink.
  - **`not BOLD` was accepted** and silently cancelled an enclosing bold;
    upstream's `not` operand is case-sensitive and raises.

### Changed
- **Style names on spans now resolve at render time, against the rendering
  console's theme** (`StyleType` in `style.rs`, `Theme::get_style`,
  `Console::get_style`; closes the main half of #14). Previously a span held a
  concrete `Style`, resolved eagerly — highlighters looked names up in a
  *process-global* default theme, so a console's own theme could not restyle
  their output at all, and an unknown markup tag was an error rather than the
  no-op upstream produces.
  - `Span.style` and `Text`'s base style are now `StyleType { Name | Style }`,
    the port of upstream's `Union[str, "Style"]`. The render path resolves them
    once per render into a vector parallel to the spans (upstream's `style_map`).
  - Two user-visible behaviour changes, both verified against real rich 15.0.0
    before shipping: highlight colours follow the console's theme, and an unknown
    markup **tag name** renders as a no-op (a genuine *syntax* error still fails).
  - `Text::highlight_regex` gains its named-group half — each group is styled
    with `{style_prefix}{name}` as a *name*, retiring the deferral noted when it
    was first ported. This is also how `RegexHighlighter` now works, so a custom
    highlighter with novel group names finally gets colours rather than just span
    boundaries.
  - `Text::stylize` takes its arguments in upstream's order
    (`stylize(style, start, end)`), and the render family takes a `&Theme`.
    `Text::render_wrapped` and `render_joined_justified` are gone (no callers,
    not upstream). `Text::render` drops its unused `ColorSystem` argument.
  - Covered by `golden/highlight.tsv` — the **only** fixture set rendered with
    `highlight=true`, which is why this path had no byte-parity net before. All
    12 pre-existing fixture sets are byte-identical.
  - Still deferred: upstream's theme *stack* (`push_theme`/`pop_theme`). See
    `docs/DIVERGENCES.md` §14 for why it needs its own design pass.

### Added
- **Prompts** (`prompt.rs`, closes the `prompt.py` half of #11): `Prompt`,
  `Confirm`, `IntPrompt`, `FloatPrompt` — a styled question, a read, and a
  re-ask until the answer validates.
  - Reading sits behind an `InputSource` trait (upstream's `stream=` argument),
    so the whole loop — including the re-ask path and the empty-answer-takes-the-
    default rule — is testable without a terminal. `ScriptedInput` is provided
    for that; `StdinInput` is the default.
  - The question line is byte-parity tested (13 fixtures in `golden/prompts.tsv`)
    across choices, defaults, `show_choices`/`show_default`, markup prompts and
    `Confirm`'s `(y)`/`(n)` rendering.
  - Case-insensitive choices return the choice as spelled in the list, not as
    typed, which upstream is explicit about.
  - Diverges in one place: exhausted input returns the default instead of
    looping forever (Python raises `EOFError`).
- **The rest of the `Text` manipulation API** (`text.rs`): `divide`, `split`,
  `pad`/`pad_left`/`pad_right`, `right_crop`, `rstrip`, `rstrip_end`,
  `expand_tabs`, `join`, `blank_copy`, `highlight_words` and `highlight_regex`.
  All carry spans correctly through the transformation — re-based and clipped by
  `divide`, shifted by `pad_left`, extended over the spaces a tab expands into.
  - `highlight_regex` is ported in its **explicit-style** form only. Upstream also
    styles each named group with the group's *name* as a style name, which needs
    a span that can hold an unresolved name; that waits on the named-span
    refactor (#14). `RegexHighlighter` already covers the named-group case by
    resolving against a theme as it goes.
  - Covered by 24 new byte-parity fixtures (`golden/text_ops.tsv`). Each case
    renders its result, so one comparison pins the plain text, the span
    boundaries and the styles together.
- **Text overflow** (`Overflow` in `console.rs`, `text.rs`, `segment.rs`). `Text`
  now carries `overflow` and `no_wrap`, as do `ConsoleOptions`, resolved with
  upstream's `self.x or options.x or DEFAULT` precedence. The wrap pipeline is
  now a full port of `Text.wrap`: split on newlines, wrap (folding an unbreakable
  word **only** under `fold`), justify, then truncate each line.
  - `Overflow::Ignore` neither wraps nor truncates, which upstream keeps
    survivable via `Console.print(crop=True)` — ported as `Segment::crop_lines`
    and applied on the print path. Without it, `ignore` would run off the side of
    the terminal rather than being cut at the edge.
  - `Text::truncate(max_width, overflow, pad)` is exposed directly.
  - Covered by 22 new byte-parity fixtures (`golden/overflow.tsv`) spanning every
    method, `no_wrap`, double-width characters straddling the cut, and the three
    positions an ellipsis can land relative to a span — the last of these caught
    a real bug, since the marker inherits the style of the run the cut *entered*,
    not the one it left.
- **Paging** (`pager.rs` + `Console::page`, `rich-cli --pager`). A `Pager` trait
  with a `SystemPager` that
  ports `pydoc.get_pager`'s selection order (`MANPAGER`, then `PAGER` — split
  into program + args — then `less -R`/`more`), including its guards: no tty on
  stdin/stdout or `TERM=dumb`/`emacs` falls back to writing straight to stdout,
  as does a pager that can't be spawned. `Console::page(styles, f)` buffers a
  closure's output and shows it, stripping styles unless `styles` is set
  (matching `Console.pager(styles=False)`); `page_with` takes a custom `Pager`,
  upstream's `Console.pager(pager=…)` seam. `rich --pager` pages *styled*
  output.
- **`rich-cli` URL fetch** (`rich-cli/main.rs`, completes the common CLI surface):
  `rich <url>` fetches an `http(s)` resource and renders it — the render mode
  comes from a flag, else the URL extension, else the response `Content-Type`
  (markdown/json/csv), else syntax-highlighting (with a lexer guessed from the
  Content-Type; upstream's behavior). Fetching uses `ureq` (rustls +
  bundled webpki roots — self-contained TLS), bounded by a 30 s timeout and a
  16 MiB **decoded**-body limit, behind a **default `fetch` feature**
  (`--no-default-features` gives a lean, network-free build). Like
  `requests.get`, a non-2xx status still renders the response body. The size
  limit is applied to the post-decompression stream (ureq's own `limit()` sits
  *under* the gzip decoder and would only bound compressed bytes, leaving a
  decompression-bomb hole); oversized and non-UTF-8 bodies get their own clear
  errors. The only deliberate divergences from upstream are the timeout and size
  bound, which upstream lacks.
- **`Live` auto-refresh thread** (`live.rs`, advances DIVERGENCES #17):
  `Live::spawn(renderable, console, writer, refresh_per_second)` returns an
  `AutoLive` handle whose background thread redraws on an interval (and on each
  `update`) — a port of upstream's `refresh_per_second`. The thread constructs and
  owns the `Live` internally (only `Send` inputs cross the boundary), so
  `Console` is now `Send` (highlighter boxes are `dyn Highlighter + Send`; the
  rich-ext registry factory matches). Deterministically tested to emit the same
  byte stream through the thread; `Drop` finalizes if `stop()` is skipped.
- **`rich-cli` `.ipynb` (Jupyter notebook) rendering** (`rich-cli/main.rs`, port
  of rich-cli's `render_ipynb`): `--ipynb` (and `.ipynb` auto-detect) renders a
  notebook — markdown cells as `Markdown`, code cells as an `In [n]:` label + a
  dim `Panel` of `Syntax`, and cell outputs (stream / error traceback /
  execute_result `text/plain`) decoded via `AnsiDecoder`, blank-line separated.
  Adds a `serde_json` dependency for notebook parsing. (Rich outputs — images,
  HTML — are deferred; text is handled.)
- **`rich-cli` panel `--title` / `--caption` / `--style` + `none` box**
  (`rich-cli/main.rs`, `box.rs`): completes the `--panel` decorator to full
  rich-cli fidelity — the panel gets a title (top border), caption (bottom
  border), and border style (e.g. `--style "bold red"`); `--panel none` uses the
  new blank `box.NONE`. Composes the already-byte-parity `Panel` builders.
- **`rich-cli` `--panel` / `--padding` decorators** (`rich-cli/main.rs`): wrap any
  render mode's output in a `Panel` (`--panel ascii|ascii2|square|rounded|heavy|
  double`) and/or `Padding` (`--padding` with 1, 2, or 4 comma-separated ints,
  unpacked like upstream's `Padding.unpack`), composing the byte-parity Panel/
  Padding renderables. Port of rich-cli's decorator flow (padding inside, panel
  outside). `Console::build_text` is now public so the `--print` markup can be
  wrapped. (`none` box + `--title`/`--caption`/`--style` are follow-ups.)
- **`rich-cli` `--export-svg`** (`rich-cli/main.rs`): exposes the new SVG export
  through the CLI (any render mode → a self-contained SVG), alongside the existing
  `--export-html`. The two are mutually exclusive; the SVG title is the resource's
  basename (else `rich`) and it uses a fixed `unique_id` (the default can't be
  byte-parity — DIVERGENCES #15). `emit` refactored to an `Export` enum.
- **`Console::export_svg`** (`svg.rs`, resolves DIVERGENCES #15): exports recorded
  output as a self-contained SVG image of a terminal window (Fira Code font-face,
  window chrome + traffic-light circles, per-line clip-paths, a generated CSS
  class table, and the styled text matrix), using `SVG_EXPORT_THEME`.
  **Byte-parity** with real rich 15.0.0 (golden `tests/golden/svg_export.svg`).
  Built on the groundwork primitives `SVG_EXPORT_THEME`, `blend_rgb`, and
  `Style::get_svg_style`, plus new `Style::attr` / `Color::is_default` accessors.
  Takes an **explicit `unique_id`** (upstream's default hashes Python `repr()`
  output, not reproducible in Rust — the auto-default is the only residual, #15).
- **`rich-cli` CSV/TSV rendering** (`rich-cli/main.rs`, advances the CLI port):
  `--csv` (and `.csv`/`.tsv` auto-detection) renders a delimited file as a table,
  a port of rich-cli's `render_csv` — blue-bordered `HEAVY_HEAD` table with the
  first row as the header and any all-numeric column right-justified + bold-green
  (body & header). Includes an RFC-4180-ish parser (double-quoted fields with
  `""` escaping, embedded delimiters/newlines, `\r\n`, leading-BOM stripping) and
  the numeric-column heuristic. **Byte-parity** with the Table real rich-cli
  builds (unit-tested). Deferred: `csv.Sniffer` dialect/has-header heuristics and
  title/caption. Demo shows a CSV table.
- **`Table` `border_style` + per-column header cell fill** (`table.rs`): a
  `border_style` builder (tints the box edges/dividers, composed over the table
  style) and `column_header_fill` (a per-column header *cell* style — content +
  padding — combined over the table `header_style`, distinct from the existing
  content-only span). Together they reproduce **rich-cli's `render_csv` styling**
  byte-parity (HEAVY_HEAD, blue border, numeric columns right-justified + bold
  green), verified by the new golden `table_csv_style`. Groundwork for the CLI's
  CSV rendering.
- **`Progress` `Download` column + `filesize::pick_unit_and_suffix`** (`progress.rs`,
  `filesize.rs`, advances DIVERGENCES #16): the `DownloadColumn` renders
  `completed`/`total` in a shared SI byte unit (`0.5/1.0 kB`), byte-parity with
  rich 15.0.0. Added `pick_unit_and_suffix` (the unit/suffix picker upstream's
  download/transfer columns share). This completes the *deterministic* Progress
  columns; only the wall-clock columns (spinner/speed/time) + the `Live` loop
  remain.
- **`Progress` custom columns** (`progress.rs`, advances DIVERGENCES #16):
  generalized from a hard-coded 3-column layout to a configurable
  `ProgressColumn` list (`Progress::columns`). Deterministic columns ported
  byte-parity — description, static text, bar, percentage, and **M-of-N**
  (`{completed}/{total}`, `progress.download` green). The inline grid matches
  upstream's `Table.grid(padding=(0,1))`. Non-deterministic columns (spinner,
  speed, time) and the download byte-unit formatting stay deferred (need the
  `Live` loop / filesize units). Default layout unchanged (byte-parity).
- **`Table` per-column `ratio` / `min_width` / `max_width`** (`table.rs`, the
  substantive half of DIVERGENCES #7): `column_ratio`, `column_min_width`, and
  `column_max_width` builders. `min_width`/`max_width` clamp the measured content
  width (over-wide cells wrap); `ratio` columns share the free width in proportion
  when the table is `expand`ed, via a faithful port of upstream's flexible/fixed
  split (`ratio_distribute` now takes per-slot minimums). Byte-parity goldens
  `table_ratio`, `table_min_width`, `table_max_width`. Only the rare width-0
  padding edge remains open on #7.
- **`ISO8601Highlighter` covers the full upstream pattern set** (`highlighter.rs`,
  resolves DIVERGENCES #13): added compact/basic calendar dates (`20230615`),
  ordinal dates, week dates, basic times, standalone timezones, and space-separated
  date-times. Upstream's one PCRE-conditional pattern is rewritten as two
  non-conditional alternatives (all-hyphen/colon + all-basic) that match the same
  strings. Byte-parity with rich 15.0.0 (unit-tested); the extended forms are
  unchanged.
- **`AnsiDecoder` decodes OSC 8 hyperlinks** (`ansi.rs`, resolves DIVERGENCES
  #10): `\x1b]8;<params>;<url>\x1b\` sequences now attach the URL to the running
  `Style` (via the new `Style::update_link`), with the empty closing sequence
  clearing it and `id=`/other params ignored — matching upstream. Re-rendering is
  byte-identical to rich except the random `id=` field we omit for determinism
  (same deviation as #20). Round-trip unit tests added.

### Fixed / clarified
- **`chop_cells` over-long-word fold is byte-parity** (`cells.rs`, resolves
  DIVERGENCES #5): dropped the `!line.is_empty()` guard so folding a character
  wider than the fold width (e.g. a 2-cell CJK char to width 1) emits upstream's
  empty leading chunk (`["", "宽", …]`), and `_wrap.divide_line` therefore yields
  the same break positions (and the same empty line) as rich. Normal text is
  unaffected (`cw <= width` never triggers the empty push). Unit-tested against
  real rich 15.0.0. Only a rare multi-codepoint-grapheme residual remains (needs a
  grapheme table).
- **Markdown trailing thematic-break blank line** (`markdown.rs`): a document that
  *ends* with a `---` rule now emits the extra trailing blank line upstream does
  (upstream's hr element yields a trailing break, observable only when the rule is
  the last block — a mid-document rule merges with the normal block separator).
  Byte-parity golden `markdown_hr_end` + unit test; the mid-document case
  (`markdown_quote_hr`) is unchanged. Closes the last Markdown-block divergence.
- **`Json` non-ASCII is byte-parity** (`json.rs`): confirmed and locked in with a
  golden (`json_unicode`) + unit test. `rich.json.JSON` defaults to
  `ensure_ascii=False`, so our UTF-8 output already matched upstream (accented
  characters/symbols render literally, not `\uXXXX`), and `serde_json`'s
  `preserve_order` keeps input key order. Corrected the stale DIVERGENCES #8,
  which wrongly claimed an escaping mismatch; the only remaining JSON caveat is
  exotic **number formatting** (exponent notation like `1e+20`/`1e-07`, and
  integers beyond i64/u64) differing from CPython's `repr`.

### Added
- **Markdown tables** (`markdown.rs`, port of upstream's `TableElement`): GFM
  tables now parse (`pulldown-cmark` `ENABLE_TABLES`) and render via [`Table`]
  built exactly as upstream — `box=SIMPLE`, `pad_edge=false`,
  `collapse_padding=true`, `markdown.table.border` (cyan) table style, and
  `markdown.table.header` (`not bold cyan`) header-content styling — with
  per-column justify read from the alignment row. **Byte-parity** with real rich
  15.0.0 (new golden `markdown_table`). Inline styling *within* a table cell is a
  documented follow-up. Enabled by a new Table capability: a per-column
  **header-content style span** (`Table::column_header_style`) that styles the
  visible header characters while the header padding keeps `header_style`.
- **Table-level `style`** (`table.rs`, port of `Table(style=…)`): a default style
  for the whole table, composed as the base of the border style
  (`border_style = style + border_style`) so it tints the box glyphs and dividers
  while cell content keeps its own styles — matching upstream exactly. Byte-parity
  golden `table_style`. With `pad_edge`/`show_edge`/`collapse_padding`/per-column
  justify already in place, the only remaining Markdown-tables step is wiring the
  `TableElement` (#9).
- **Table `collapse_padding`** (`table.rs`, port of `_get_padding_width`):
  `Table::collapse_padding(true)` merges adjacent cell padding — an interior
  column's left pad is reduced by the previous column's right pad
  (`max(0, pad_left - pad_right)`), so a default-padded grid collapses to a
  single space between columns. Byte-parity-tested (new golden `table_collapse`).
  The last per-cell Table feature Markdown tables need before wiring the element.
- **Table `pad_edge` + `show_edge`** (`table.rs`, port of upstream's flags):
  `Table::pad_edge(false)` drops the first column's left pad and the last
  column's right pad; `Table::show_edge(false)` removes the outer box edges
  (top/bottom borders + left/right glyphs), leaving only the internal dividers +
  content. `Box::get_top/get_row/get_bottom` gained an `edge` parameter.
  Byte-parity-tested (new goldens `table_pad_edge`, `table_no_edge`). These are
  two of the features Markdown tables need (`SIMPLE` box + `pad_edge=False`).
- **OSC 8 hyperlinks** (`Style::with_link`): a `Style` can now carry a link URL,
  rendered as an OSC 8 hyperlink around its styled text. **Markdown links**
  (`[text](url)`) render as the `markdown.link_url` style (underline blue) + the
  hyperlink. Byte-identical to real rich 15.0.0 **except** upstream's random `id=`
  field, which we omit for determinism (DIVERGENCES #20).

### Added (functional, non-byte-parity)
- **Markdown code blocks** (`markdown.rs`): fenced and indented code blocks now
  render via the `Syntax` renderable (language from the fence info string), so
  they're syntax-highlighted. Not byte-parity (syntect ≠ Pygments — DIVERGENCES
  #9/#18). Markdown now covers everything but links and tables.
- **`LogRender`** (`log_render.rs`, a Rust-native reimagining of `rich/_log_render.py`
  + `rich/logging.py`): formats a log record — optional time, a severity-colored
  `LogLevel`, message, optional path — into a styled line, using the same column
  styles (`log.time`, `logging.level.*`, `log.path`). Takes a `LogLevel` enum +
  strings, so the core stays dependency-light; a `log::Log` handler on top is a
  `rich-ext` follow-up (DIVERGENCES #19).
- **`Traceback`** (`traceback.rs`, a Rust-native reimagining of `rich/traceback.py`):
  `Traceback::new(&error)` walks an error's `Error::source()` chain and renders the
  message + `Caused by:` chain in a red-bordered `HEAVY` panel; `from_message` takes
  a plain string (e.g. a captured panic). No stack frames — Rust errors don't carry
  them (DIVERGENCES #19).
- **`Pretty`** (`pretty.rs`, a Rust-native reimagining of `rich/pretty.py`):
  `Pretty::new(&value)` / `Pretty::compact(&value)` format a value with its
  `Debug` impl (`{:#?}` / `{:?}`) and colorize the result with the built-in
  `ReprHighlighter` (numbers, strings, `None`/`Some`, paths, …). Rust has no
  reflection, so this replaces upstream's Python-object introspection; a few
  Rust spellings (`true`/`false`) are left unstyled (DIVERGENCES #19).
- **Syntax highlighting** (`syntax.rs`, port of `rich/syntax.py`'s renderable via
  the `syntect` crate): `Syntax::new(code, language)` highlights a code block into
  a solid colored panel (fg/bg + bold/italic/underline, each line padded to width
  with the theme background), with a configurable theme. Language is resolved by
  name or file extension; unknown languages render as plain text. **Not**
  byte-parity with upstream — `syntect` uses different grammars/themes than
  Pygments (DIVERGENCES #18); the first renderable that is functionally, not
  byte-, verified. `syntect` uses its pure-Rust `fancy-regex` backend (no C
  `onig`).

### Fixed / verified
- **Wrapping matches upstream for combining sequences** (DIVERGENCES #5 corrected):
  confirmed `cells::chop_cells` is byte-parity with real rich 15.0.0 — current
  rich folds over-long words char-by-char (not by grapheme), and 0-width combining
  marks stay attached to their base char in both. Added unit tests and a golden
  (`wrap_combining`, a decomposed `base + U+0301` fold). The old "we don't do
  grapheme wrapping" note was stale; the only residual difference is an empty
  leading chunk for a char wider than the fold width, which we suppress.

### Added
- **All built-in boxes** (`box.rs`): added the remaining upstream box constants —
  `ASCII2`, `ASCII_DOUBLE_HEAD`, `SQUARE_DOUBLE_HEAD`, `MINIMAL_HEAVY_HEAD`,
  `MINIMAL_DOUBLE_HEAD`, `SIMPLE`, `SIMPLE_HEAD`, `SIMPLE_HEAVY`, `HORIZONTALS`,
  `HEAVY_EDGE`, `DOUBLE_EDGE`, `MARKDOWN` — so `box.py` is complete. Byte-parity-
  tested (new goldens `table_simple`, `table_double_edge`). `SIMPLE`/`MARKDOWN`
  are the boxes Markdown tables will use.
- **Box substitution** (`Box::substitute`, port of `rich/box.py`'s `substitute`):
  on a legacy Windows console the fancy boxes (`ROUNDED`/`HEAVY`/`HEAVY_HEAD`) fall
  back to `SQUARE`, and on a non-UTF-8 terminal any non-ASCII box falls back to
  `ASCII`. Added the `legacy_windows`/`safe_box`/`ascii_only` `Console` flags
  (default off/on/off); `Panel` and `Table` apply the substitution. Byte-parity-
  tested against real rich 15.0.0. Resolves the mechanism half of DIVERGENCES #6
  (runtime auto-detection of legacy terminals is still deferred).
- **Table `no_wrap`** (`table.rs`, toward `rich/table.py`): `Table::column_no_wrap`
  marks a column whose cells crop to a single line with ellipsis instead of
  wrapping, and which doesn't shrink during collapse (only yielding via the
  last-resort even reduce). Byte-parity-tested (golden `table_nowrap`). Narrows
  DIVERGENCES #7 (only per-column ratio/min/max and the width-0 padding edge
  remain).
- **`Live`** (`live.rs`, port of `rich/live.py`'s manual-refresh core): drives a
  `LiveRender` over a generic `Write` sink — `start` hides the cursor and draws,
  `update`/`refresh` reposition and redraw in place, `stop` commits the final
  frame and shows the cursor. The emitted byte stream is byte-parity with real
  rich 15.0.0 (`auto_refresh=False`, `transient=False`). The background
  auto-refresh thread + transient/alt-screen modes are deferred (DIVERGENCES #17).
- **`LiveRender`** (`live_render.rs`, port of `rich/live_render.py`): wraps a
  renderable, records its rendered shape, and emits the `position_cursor` /
  `restore_cursor` control-code sequences (`\r` + erase-line + cursor-up×N) that
  redraw in place — the byte-parity-testable core of a `Live` display. Byte-parity
  vs real rich 15.0.0. (The full `Live` refresh loop + vertical-overflow cropping
  are deferred — issue #6.)
- **`Progress`** (`progress.rs`, port of `rich/progress.py`'s default display):
  `Progress::add_task(description, total, completed)` renders a static grid of
  tasks — each a description, a bar that flexes to fill the width, and a magenta
  percentage — reproducing the default `TextColumn`/`BarColumn`/`TaskProgressColumn`
  layout. Byte-parity-tested against real rich 15.0.0 (golden `progress_three`).
  Custom columns, the time/rate columns, and the `Live` refresh loop are deferred
  (DIVERGENCES #16 / issue #6).
- **`Status`** (`status.rs`, port of `rich/status.py`) + **styled spinner frames**:
  `Status::new("…")` renders a spinner (default `dots`, green) followed by a
  message; `Spinner::style` styles the frame while the trailing text stays plain.
  Byte-parity-tested against real rich 15.0.0 (static `t = 0` frame). Live-loop
  animation is still deferred (DIVERGENCES / issue #6).
- **HTML export** (`export.rs` + `terminal_theme.rs`, port of
  `Console.export_html` + `_export_format`): `Console::export_html(|c| …)`
  captures printed output and returns a self-contained HTML document with inline
  styles, resolving colors through a `TerminalTheme` (`DEFAULT_TERMINAL_THEME`).
  Added `Style::get_html_style` and `blend_rgb`. Byte-parity-tested against real
  rich 15.0.0 (full document + `get_html_style`). Also `export_html_classes`
  (the default `.r1 {…}` CSS-class stylesheet form, byte-parity). The `rich-cli`
  `--export-html` flag emits the class form (rich-cli's default). (SVG export
  deferred — DIVERGENCES #15.)
- **Generic `RegexHighlighter` base + `ISO8601Highlighter`** (`highlighter.rs`,
  toward `rich/highlighter.py`): extracted the shared regex-highlight loop into a
  public `RegexHighlighter` (base-style prefix + named-group patterns), refactored
  `ReprHighlighter` onto it, and added `ISO8601Highlighter` for standard extended
  date/time strings (`iso8601.date`/`time`/`timezone`). Highlighters now stylize
  **every** matched named group (unknown names → null style), reproducing
  upstream's per-field segment boundaries. Byte-parity-tested vs real rich 15.0.0
  (repr unchanged; ISO8601 new). See DIVERGENCES #13/#14 for the remaining gaps.
- **Table per-column width, style + ellipsis overflow** (`table.rs`, toward
  `rich/table.py`): `Table::column_width` pins a column's content width (content
  wraps to it instead of the column shrinking), `Table::column_style` styles a
  column's body cells, and table cells now wrap with **ellipsis overflow** (a
  word wider than the column is cropped with `…`), matching upstream's default.
  Added the last-resort even `ratio_reduce` when fixed columns overflow.
  Byte-parity-tested against real rich 15.0.0. (Per-column `ratio`/min/max and
  `no_wrap` still deferred — DIVERGENCES #7.)
- **Markup fidelity** (`markup.rs`, toward `rich/markup.py`): a `[…]` is now only
  a tag when it starts with `[a-z#/@]` (so `[Hello]`/`[42]` are literal text),
  unmatched closing tags return `RichError::Markup`, `@`-tags apply no visible
  style, and a public `markup::escape` implements upstream's backslash-doubling
  escape. Byte-parity-tested (literal brackets, meta tags) + negative tests for
  the error cases. Narrows DIVERGENCES #2.
- **`Styled`** (`styled.rs`, port of `rich/styled.py`) and **`Screen`**
  (`screen.rs`, port of `rich/screen.py`): `Styled` lays a `Style` under an
  entire renderable (each segment's own style still wins); `Screen` fills the
  console region (width × height) with a child, cropping/padding to an exact
  rectangle with an optional background style. Added `Segment::apply_style` and
  `Segment::set_shape` (now shared with `Layout`). `Styled` is byte-parity-tested
  against real rich 15.0.0; `Screen` has a known trailing-newline edge
  (DIVERGENCES #12).
- **`rich-cli` render modes** (`crates/rich-cli`): the binary now implements a
  slice of upstream rich-cli's flags — `-p/--print` (console markup),
  `-m/--markdown`, `-j/--json`, and `--rule` — plus `-w/--width`,
  `--left/--center/--right` justification, stdin (`-`), and file-extension
  auto-detection (`.md`/`.json`). Covered by CLI integration tests. (Syntax, CSV,
  panel, ipynb, URL fetch, and HTML export remain roadmap items.)
- **Height-aware `Panel`** (+ `Console::render_lines` height handling): a `Panel`
  now consumes `options.height`, expanding its content to fill the imposed height
  (`child_height = height − 2 − padding`) so a `Panel` used as a `Layout` leaf
  fills its region instead of being blank-padded. `render_lines` crops/pads any
  renderable to an explicit height. Byte-parity-tested against real rich 15.0.0
  (Panel-in-layout goldens). Resolves the height half of DIVERGENCES #11.
- **Layout** (`layout.rs` + `ratio.rs`, port of `rich/layout.py` +
  `_ratio.ratio_resolve`): split a region into ratioed/sized rows (`split_row`)
  and columns (`split_column`), recursively; each leaf is rendered to an exact
  `(width, height)` block (`Segment.set_shape`) and tiled. Added console
  `height` (builder + detection) and `ConsoleOptions::update_dimensions`.
  Byte-parity-tested against real rich 15.0.0 (column/row/nested, incl. `size`).
  (Empty-leaf placeholder renders blank; height-aware container leaves like
  `Panel` don't yet expand to fill — DIVERGENCES #11.)
- **Console capture + export** (`Console::capture` / `export_text`, port of
  `Console.capture()` + `export_text(styles=False)`): run a closure with output
  recorded to an internal segment buffer instead of stdout, returning either the
  rendered ANSI (`capture`) or plain style-stripped text (`export_text`). Captures
  nest. Byte-parity-tested against real rich 15.0.0. (HTML/SVG export deferred.)
- **ANSI decoder** (`ansi.rs`, port of `rich/ansi.py`): `AnsiDecoder` tokenizes a
  terminal string and turns SGR sequences back into styled `Text` — attributes,
  16/256/truecolor foreground + background, lenient parsing, and cross-line style
  persistence. Byte-parity-tested against real rich 15.0.0 (re-render round-trip).
  Added `Color::from_ansi`/`from_rgb` and `Style::from_color`. (OSC hyperlinks are
  skipped, pending `Style` link support — DIVERGENCES #10.)
- **Control codes** (`control.rs`, port of `rich/control.py` + `ControlType`):
  the `Control` renderable and typed `ControlType` sequences — screen clear,
  cursor home/move/move_to/move_to_column, show/hide cursor, alt-screen toggle,
  bell, erase-in-line — plus `Console::control`/`clear`/`show_cursor`/`bell`.
  Control segments are written only to a real terminal (matching upstream's
  `_render_buffer`). Byte-parity-tested against real rich 15.0.0.
- **Markdown** (`markdown.rs`, port of `rich/markdown.py` core): renders
  paragraphs, ATX headings (h1–h6, centered h1), **bullet + ordered lists**,
  **block quotes**, **thematic breaks (hr)**, and inline strong/emphasis/code via
  `pulldown-cmark`, as justified full-width blocks separated by blank lines.
  Byte-parity-tested against real rich 15.0.0. (Code blocks, links, and tables
  deferred — DIVERGENCES #9.)
- **Full spinner table**: all 73 built-in spinners are vendored
  (`spinner_data.rs`) from `_spinners.py`, replacing the curated subset.
- **ReprHighlighter** (`highlighter.rs`, port of `rich/highlighter.py`): the
  built-in highlighter that auto-colors numbers, bools, `None`, strings, paths,
  URLs, braces, calls, IPs, UUIDs, and tags. Patterns vendored verbatim
  (`repr_patterns.rs`) and compiled with `fancy-regex`; enabled via
  `ConsoleBuilder::highlight(true)`. Byte-parity-tested against real rich 15.0.0
  across many pattern types.
- **Full emoji table**: the complete ~3600-entry `_emoji_codes` table is vendored
  (`emoji_codes.rs`, binary-searched), so every `:shortcode:` resolves. Resolves
  the former curated-subset divergence.
- **Full 256-color names**: the complete `ANSI_COLOR_NAMES` table is vendored
  (`color_names.rs`), so markup/style names like `[orange1]`, `[grey37]`,
  `[deep_sky_blue1]` resolve to the correct 8-bit colors. Byte-parity-tested;
  resolves the former partial-names divergence.
- **Table title / caption / show_lines**: `Table::title` and `caption` render
  centered above/below the table (italic / dim-italic), and `show_lines` draws a
  separator between body rows. Byte-parity-tested against real rich 15.0.0.
- **Table `expand` + per-column justify**: `Table::expand` fills the available
  width (distributing leftover space via `ratio_distribute`), and
  `Table::add_column_justify` justifies a column's cells left/center/right.
  Byte-parity-tested against real rich 15.0.0.
- **Table flexible widths**: when a table's natural width exceeds the console
  width, the widest columns now shrink and their cells wrap to fit (port of
  `Table._calculate_column_widths` + `_collapse_widths` + `_ratio.ratio_reduce`,
  with banker's rounding). Byte-parity-tested against real rich 15.0.0.
- **Measurement** (`Renderable::measure` + top-level measurement-fit): the print
  path now shrinks the width to a renderable's measured content width when no
  explicit justify is set. This makes a bare `Text::justify(...)` shrink to its
  content (matching upstream, resolving the former DIVERGENCES #9), while
  `print(justify=…)` and container-embedded justify still pad to full width.
  Byte-parity-tested against real rich 15.0.0.
- **Bar** (`bar.rs`, port of `rich/bar.py`): a horizontal bar spanning
  `[begin, end]` within `[0, size]`, with eighth-block sub-cell edges. Byte-parity-
  tested against real rich 15.0.0.
- **Emoji** (`emoji.rs`, port of `_emoji_replace` + a curated `_emoji_codes`
  subset): `:name:` shortcodes (with `-emoji`/`-text` variants) expand in the
  `Console` print path (default on, `ConsoleBuilder::emoji`). Byte-parity-tested
  against real rich 15.0.0; replacement logic is complete, code table curated.
- **Text justify** (`Text::justify` + `Console::print_justified`): left/center/
  right justification, padded to the render width. Byte-parity-tested against
  real rich 15.0.0 inside a `Panel` and via `print(justify=…)`. (Bare top-level
  measurement-fit is deferred — see DIVERGENCES #9.)
- **JSON** (`json.rs`, port of `rich/json.py`): parses a JSON string (via
  `serde_json` with `preserve_order`) and pretty-prints it with 2-space indent
  and the default highlight colors (bold braces, bold-blue keys, green strings,
  bold-cyan numbers, italic bools/null). Byte-parity-tested against real rich
  15.0.0 for ASCII documents. First core third-party content dependency.
- **Panel subtitle**: `Panel::subtitle` / `subtitle_align` (drawn into the bottom
  border, mirroring title alignment). Byte-parity-tested against real rich 15.0.0.
- **Spinner** (`spinner.rs`, port of `rich/spinner.py` + a subset of
  `_spinners.py`): `Spinner::render(time)` picks the animation frame for an
  elapsed time (dots/line/dots2/arrow/simpleDots). Frames verified against real
  rich 15.0.0. (Live-loop animation deferred.)
- **ProgressBar** (`progress_bar.rs`, port of `rich/progress_bar.py` static
  render): determinate bars with half-cell resolution and the default
  `bar.complete`/`bar.finished`/`bar.back` styles. Byte-parity-tested against
  real rich 15.0.0. (Indeterminate "pulse" deferred.)
- **Title alignment** for `Rule` and `Panel` (`HorizontalAlign::{Left,Center,Right}`,
  shared with `Align`): `Rule::align` and `Panel::title_align`. Byte-parity-tested
  against real rich 15.0.0.
- **Columns** (`columns.rs`, port of `rich/columns.py`): packs items into as many
  equal-gap columns as fit the width, filling row by row (ports the column-count
  fitting algorithm). Byte-parity-tested against real rich 15.0.0.
- **Constrain** (`constrain.rs`, port of `rich/constrain.py`): render a child
  within a reduced max width. Byte-parity-tested against real rich 15.0.0.
- **filesize** (`filesize.rs`, port of `rich/filesize.py`): `decimal()` SI byte
  formatting, unit-tested against real rich reference values.
- **Align** (`align.rs`, port of `rich/align.py` horizontal axis): left/center/
  right alignment of a child within the available width. Byte-parity-tested
  against real rich 15.0.0.
- **Tree** (`tree.rs`, port of `rich/tree.py`): renders a hierarchy with the
  `├──`/`└──` guide lines. Byte-parity-tested against real rich 15.0.0.
- **Table** (`table.rs`, port of `rich/table.py` core): columns, rows, box choice
  (default `HEAVY_HEAD`), per-cell padding, bold headers, and multi-line/wrapped
  cells. Byte-parity-tested against real rich 15.0.0 (SQUARE and HEAVY_HEAD).
  Added `Box::get_row`/`RowLevel`, `Segment::simplify`, and the `HEAVY_HEAD` box.
- **Word wrapping** (`wrap.rs`, port of `_wrap.divide_line` + `cells.chop_cells`):
  `Text` now wraps to the available width — breaking on words and folding
  over-long words — so `Panel`/`Padding` reflow long content instead of cropping.
  Byte-parity-tested against real rich 15.0.0.
- **Layout primitives**: width-aware render model (`ConsoleOptions`,
  `Console::render_lines`, `Segment::split_lines`/`adjust_line_length`) plus the
  first composite renderables — `box` (ROUNDED/SQUARE/HEAVY/DOUBLE/MINIMAL/ASCII),
  `Rule`, `Padding`, and `Panel` — all byte-parity-tested against real rich 15.0.0.
- Golden harness extended with a renderables fixture (`renderables.tsv`), captured
  in Python UTF-8 mode for deterministic box glyphs.

### Fixed
- **`Color::parse("color(N)")` for N < 16**: now returns a `Standard` color (SGR
  30–37/90–97) instead of an 8-bit palette color, matching `Color.parse`. This
  makes ANSI round-trips of the standard colors byte-identical to upstream.

### Foundation
- **Workspace scaffold**: `rich` (core, mirroring upstream **15.0.0**),
  `rich-ext` (`0.1.0`), and `rich-cli` (mirroring upstream **1.8.1**).
- **First vertical slice** of the `rich` core, parity-tested against real
  `rich` 15.0.0: `color`, `style`, `cells`, `segment`, `markup`, `text`, `theme`,
  `console`, plus the `protocol` extension points (`Renderable`, `Highlighter`).
- **Internal plugin registry** in `rich-ext` (`ExtensionRegistry`, `ConsoleExt`)
  with an example `NumberHighlighter`, demonstrating the core/ext boundary.
- **`rich-cli` binary**: argument handling, plain-file printing, and a capability
  demo. Rich rendering subcommands are tracked as roadmap issues.
- **Governance**: `AGENTS.md`, `docs/{ARCHITECTURE,PORTING,PLUGINS,DIVERGENCES}.md`,
  the `sync-upstream` and `port-module` skills, `UPSTREAM.toml` pins, and the
  golden parity harness (`scripts/capture_golden.py`).
