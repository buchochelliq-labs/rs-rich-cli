# csv: a CSV viewer on intuiTUIve

## Round 1: csvlens

The clone is [`examples/csv.rs`](https://github.com/buchochelliq-labs/rs-rich-cli/blob/main/crates/rich-intuituive/examples/csv.rs),
a rebuild of [csvlens](https://github.com/YS-L/csvlens) (YS-L, MIT; ratatui),
the `less` for CSV. Its main view was the scope: the table, its keys, its
status line and its lazy reading of long files.

What was built (1,534 lines: about 470 the file layer with no framework in it
(the record index, the parser, the delimiter sniffer and the find/filter/sort
pass), 380 the state and the status line, 600 the table, the prompts and the
keys, 60 `main`; tests 550 lines in `tests/csv.rs`, 14 tests and one measure
run by hand):

- **The look**, csvlens's: a rule, the header in bold, row numbers in a gutter
  with `│` after them (the selected one bold and underlined, the gutter as wide
  as the largest number in view), columns fitted to their title and the cells
  in view with csvlens's four cells between them and its cap of 30% of the
  width (`…` past it), a rule, and the status line in its grey:
  `cities.csv [Row 3/5, Col 1/4] [Find "Re": 2/4] [Filter "Ci|Pop": 2/4 cols]`.
  Its dark theme's colours: the selected row, column or cell
  (`bold #c0c0c0 on #3e3d32`), marked rows, matches in red.
- **Lazy reading**, as csvlens reads: the first 64 KiB are indexed before the
  first frame; a `spawn_stream` task indexes the rest by record boundaries
  (quote-aware), and the row count shows `N+` until it is done; a row in view
  is a seek and a read of its bytes, parsed then and kept in a small cache. Only
  the window is read (the tests count the rows parsed). Finding, filtering and
  sorting read the whole file once, on a `spawn` task, and give back an index
  of the rows in view.
- **Input:** a file, or stdin (`cat x.csv | csv`, saved to a temporary file as
  csvlens does, keys read from the terminal); the delimiter sniffed (`,` tab
  `;` `|`, `.tsv` is tab), or `-d C` / `-d '\t'` / `-t`; `--no-headers`
  (columns named 1, 2, 3); `-i` (smart case); `--filter`, `--find`,
  `--columns` for the first view.
- **csvlens's keys:** hjkl and the arrows; Ctrl+F/B and PgDn/PgUp a window,
  Ctrl+D/U and d/u half one; G/End, g/Home; `<n>G` (the digits typed into the
  status line, ended by G, g or Enter) and `:<n>`; `/` finds (matches in red,
  `[Find "re": i/n]`), n/N go to the next and previous row found; `&` filters
  rows, `*` filters columns (all shown when nothing matches, as csvlens does);
  Tab cycles row, column and cell selection; `>`/`<` widen and narrow the
  selected column by four cells; J/Shift+↓ sorts by it (numbers as numbers when
  the whole column is, else text; again to reverse; `[▴]`/`[▾]` in its title),
  Ctrl+J naturally (`[▴N]`); `#`/`@` find and filter like the selected cell;
  `y` copies the row or cell; Enter (cell mode) prints the cell and exits;
  `-S`/`-W` wrap; `f<n>` freezes n columns; `m`/`M` mark rows, Ctrl+E prints
  the marked rows with the header and exits; `r` resets; H/? the help (the
  framework's, in csvlens's sections: Moving, Search, Selection modes, Other
  options); q quits. The brief's `-`/`+` resize and `-w` wrap are not
  csvlens's keys; its own (`<`/`>`, `-S`/`-W`) are used.

**Measured** (unoptimised test build, `cargo test`, on the shared machine;
a release build would be several times faster, the ratios hold):

| File | First frame | Rows parsed for it | Whole index (background) | A key (j) | G | Filter (regex, whole file) |
|---|---|---|---|---|---|---|
| 200,000 rows, 7 MB | 95–120 ms | 36 (the rows in view) | — | — | 29–35 ms, 36 rows parsed | — |
| 1,000,000 rows, 38 MB | 130–200 ms | 36 | 410–450 ms | 28 ms | 30–55 ms | 3.3 s |
| 50 rows | — | — | — | 30 ms | — | — |

A key costs the same on a million rows as on fifty: the cost is the frame
(a 120x40 table drawn again), not the file. The filter is one regex scan of
the whole file (cells one to a line, `(?m)`), through rs-rich-ext's
`HighlightMatches`; with a `Text` and a `KeepLines` per row it took 11 s.

**Bespoke:** none drawn. The table is `virtual_table_with`; the gutter is its
first column, frozen, whose cells end in `│`; the prompts are `popup_at` a row
high over the status line, an `input` after a label. The file layer (index,
parser, sniffer, the pass) is plain std, as the brief asks when rs-rich-data
is not a dependency.

**Left out:** csvlens's input history (↑/↓ in a prompt), `--echo-column`,
`--prompt`, `--color-columns`, auto-reload of a changed file, the header row's
underline of the selected column, its yellow on the match in the selected row,
the horizontal rule under the header and the `┬`/`┴`/`╥` joins, per-row wrap
heights, `Col n` reporting the scroll across in row mode, Ctrl+H/L and
Ctrl+←/→ in row mode (they move the cell cursor in column and cell modes): each
is a gap below.

### New gaps and bugs

| Gap or bug | Needed for | API sketch or repro | Where |
|---|---|---|---|
| A table's options and columns cannot change after it is built | Every csvlens change of layout: a column filter (`*`), `f<n>`, a width set with `<`/`>`, Tab's modes (the cell cursor on or off), `-S`, the sort arrow in a title. The example hashes all of it into a key and builds a new table under a `switch`, which loses the scroll across (see next) and keeps every table it ever built | `table_with(columns_fn, ..)` with `columns: impl Fn() -> Vec<Column>`, and options that take signals: `.freeze_columns_signal(s)`, `.cells_when(\|\| on, col)`, `.row_height_signal(s)` | core |
| The scroll across is not readable or settable | csvlens's `Col n/m` (the first column in view), Ctrl+H/L (a window across), Ctrl+←/→ (to the ends) in row mode, and keeping the place when the table is rebuilt. `TableOptions::offset` is the rows' only | `TableOptions::offset_x(Signal<usize>)`: the first scrolled column in view, read or set; `.page_x()` for the columns in view | core |
| A node that rebuilds its child when a key changes, dropping the old one | The rebuild above. `switch` keeps every child it built (a leak as widths change), `each` lays its children at their natural heights (a table in it gets one row) | `keyed(move \|\| key(), \|key\| node)`: one child, flexible, rebuilt and the old forgotten on a new key | core |
| Column widths in percent of the table, and set from the keyboard | csvlens caps a fitted column at 30% of the width; `Column::max` is cells, so the example reads the table's `RectHandle` and rebuilds on a resize. `<`/`>` need the column's width now: `resizable()` keeps dragged widths inside the table, and the natural width (title and cells in view) is the table's to know, so the example measures the rows in view again | `Column::max_percent(30)`; `TableOptions::widths(Signal<Vec<Option<u16>>>)` shared with the mouse's drag; `table.width_of(col)` | core |
| Rows of different heights in a virtual table | `-S`: csvlens makes each row as tall as its longest cell. `virtual_table` takes one `row_height` for all rows, so the example sizes every row to the tallest in view when wrap is turned on | `TableOptions::row_height_for(\|i\| lines)`, or `multiline` measuring only the rows in view | core |
| The selection's highlight covers the cells' own styles | The matches in the selected row (csvlens draws them red on yellow there); a table has no `highlight_layer` as a list has | `TableOptions::highlight_layer(HighlightLayer::Under)` | core |
| Help rows for keys a child handles | `h`/`l` (scroll across) are the table's own in row mode, so they cannot be bound on the root to be listed in the help without taking them from the table | `node.describe_key("h l", "Scroll one column")`: listed, not bound | core |
| Port gap: rich's `Text.markup` | Turning a cell with its matches styled (a `Text` from `HighlightMatches`) into the markup a table cell takes; the example writes the spans out as markup itself | `Text::markup(&self) -> String`, upstream's property | rich (mirror) |
| A prompt without history | csvlens's ↑/↓ through the patterns typed before (shared by find and filter) | `input(text).history(Signal<Vec<String>>)` | core |

### Friction with the existing APIs

- **Uniform column spacing.** csvlens has two cells after the gutter's `│`
  and four between columns. `column_spacing` is one number, so every data cell
  and title carries two trailing spaces of padding, which then shows as
  `OH…` when a column is narrowed to its title. A `Column::padding(left,
  right)` would say it.
- **No header rule, no border joins.** csvlens's header sits between two
  rules, and its gutter line meets them with `┬` and `┴` (`╥`/`╨` at the
  frozen columns' edge). `header_gap` gives blank lines, not a line; the
  example has a rule above the header and one above the status line, and the
  gutter's `│` stops at the rows.
- **Titles are fixed.** csvlens underlines the selected column's title in
  column and cell modes; titles are `Column` strings, so that needs a rebuild
  on every h/l, and was left out.
- **The selection's index in a row closure.** The gutter's selected number is
  bold: the row closure reads `selected`, so every row is asked for again on
  each move (the cache keeps that from reading the file). A `row(i, state)`
  telling the closure whether the row is selected, or a gutter option, would
  not.
- **Finding over a source is the app's.** The example compiles the pattern
  with rs-rich-ext's `KeepLines`/`HighlightMatches` (compiled once, which
  `Text::highlight_regex` is not), scans the whole file on a task, and keeps
  the positions found, n/N and the `i/n` count itself. `list_search` does
  this for a list in hand; nothing does it for a lazy source.
- **Sorting a virtual table is the app's**, by design (its rows are never all
  in hand): the example reads the column on a task and keeps the order as an
  index of the view. That index (filtered, sorted) over a lazy source is the
  generic piece every data viewer will rebuild.
- **The CSV layer is std.** rs-rich-data has a CSV reader, a sniffer and the
  `RowSource` trait, but it is not a dependency of rs-rich-intuituive, and its
  sources read forward only: none can seek to row 900,000. rs-rich-ext's
  `VirtualRows` (`row_count`, `row(i)`) is the random-access shape, with no
  indexed file source behind it.
- **What worked well:** `virtual_table_with` asked for exactly the rows in
  view (36 of a million), `freeze_columns` kept the gutter and frozen columns
  in place with `‹`/`›` for the rest, the cell cursor and `column_highlight`
  gave column and cell modes, `theme_styles` on the table turned the row
  highlight off for them, `spawn_stream` plus a proxy gave the `N+` count,
  `popup_at(Origin::BottomLeft, ..)` put the prompts in the status line,
  `bind_sequence` gave `-S` and `f<n>`, `bind_when` kept the cell-cursor keys
  out of row mode and its help, and `cx.copy`, `quit_with` and `run_for`
  gave `y`, Enter and Ctrl+E.

### Components that would help

| Component | What it would replace or enable | API sketch | Where | Value |
|---|---|---|---|---|
| A table over a source | The example's `Source`, the `len`/`row` closures, the cache and the `N+` count: a table that asks a source for rows by index and its count as it grows | Core, generic: `trait TableSource { fn len(&self) -> Count /* n, exact */; fn row(&self, i: usize) -> Vec<String>; }` and `source_table(src, selected, options)`, polled after a source says it grew. Plugin `data` (rs-rich-data, off by default): `data::csv_source(path, Dialect)` (a seekable, background-indexed CSV with the sniffer), `data::rows_source(Rows)`, adapters from `VirtualRows` | core trait + plugin `data` | High: every data viewer (csvlens, a SQL client, a log table) |
| A view index: filter, sort and find over a source | `answer()`, `run_query`, n/N, the `Finding...` state: about 200 lines | Core, generic: `view_of(source, ViewQuery { keep: Option<Matcher>, sort: Option<(col, Cmp)>, find: Option<Matcher> })` returning `Signal<Option<Rc<Vec<u32>>>>` and the found positions, run on a task, cancelled when the query changes; `.next_found()`/`.prev_found()`. The matcher is a compiled regex over a row's cells (rs-rich-ext `transform` underneath) | core | High |
| A row-number gutter | The frozen first column, its `│`, the selected number's style, the width from the numbers in view | `TableOptions::row_numbers(\|i\| n).gutter_rule(true)`, joining the header rule | core | Medium |
| Table chrome as csvlens and Textual's `DataTable` draw it | The two rules, the joins, the header rule | `TableOptions::header_rule(true)`, `.rules(Rules::Frame)` with joins (`┬ ┴ ╥ ╨`) at the gutter and the frozen edge | core | Medium |
| Reactive table options | The `switch` rebuild and its `layout_key` | Options from signals (see the gap), as Textual's `DataTable` and Bubbles' `table.Model` change columns and widths in place | core | High |
| Match highlighting in cells | `cell_markup`: spans of a compiled regex written out as markup | `highlight_matches(text, &matcher, style) -> String` (markup), or table cells that take a `Text` | core (on rs-rich-ext's `HighlightMatches`) | Medium |
| A status-line prompt | `prompt()`: a popup a row high over the status line, a prefix, Enter/Esc, the goto's G | `status_prompt(prefix, initial).on_submit(..).ends_on("G g")`, a one-line `input` docked where the status line is (vim's and less's command line, Bubbles' `textinput` in a footer) | core | Medium |
| A sniffer and record parser exported to apps | `scan`, `parse_record`, `sniff`: 100 lines of std | rs-rich-data's `csv::sniff` and `CsvReader` already exist; the `data` plugin above would make them reachable from an intuiTUIve app | plugin `data` | Medium |
| Per-cell wrap in a virtual table | `toggle_wrap`'s height guess | `row_height_for`, or `multiline()` on a virtual table measuring the rows in view | core | Low |

Comparable libraries: Textual's `DataTable` keeps columns, widths and cursor
type (row, column, cell) as reactive attributes and has `fixed_columns` and
`fixed_rows`; Bubbles' `table.Model` has `SetColumns`/`SetRows`/`SetWidth` in
place; ratatui's `Table` is immediate (csvlens draws its own `CsvTable`
widget on a buffer, including the gutter and the joins). None of them reads a
file lazily; csvlens's own `csv.rs` (a position table built on a thread, rows
read by seeking) is the model the `data` plugin's source would follow.

## Round 2: rebuilt on round 6

`examples/csv.rs` went from 1,534 lines to 1,375 (tests 526, 14 tests and
the hand-run measure, all passing; three expectations changed, below).

### Replaced

| Hand-built (round 1) | Now | Lines gone |
|---|---|---|
| The whole-file pass (`answer`, `rows_matching`, `Answer`, `run_query`, the task slot, the `view`/`found`/`busy` signals, picking a record by position) | the file as a `TableSource` (`Indexed`: the count from the indexer's signal, `N+` while it runs, `RowCount`) and `view_of(source, \|\| ViewQuery)` with `keep`, `find`, `sort` and, for Ctrl+J, `compare(natural_cmp)`; `View::source_row`, `place`, `is_busy`, `next_found`/`previous_found` | ~150 |
| The `switch` on a hash of every layout input (`layout_key`), which rebuilt the table on each change and kept every table built | one table per selection mode (`keyed(mode)`, the old one dropped), everything else read as it draws: `columns_with` (titles, the sort arrow, widths set by `<`/`>`, the column filter), `freeze_columns_with`, `multiline_when` | ~25 |
| The 30% cap from the table's `RectHandle` (`cap`) | `Column::max_percent(30)` | ~10 |
| Measuring a column's width again for `<`/`>` (`width_of`), and the page from the `RectHandle` | `TableLayout::width_of` and `rows_in_view` | ~20 |
| `-S`'s guess at one height for every row (`toggle_wrap`) | `multiline_when(\|\| wrap)`: each row as tall as its own cells, as csvlens | ~15 |
| Writing a highlighted cell's spans out as markup | `Text::markup()` | ~15 |
| The prompts as a one-row `popup_at` over the status line | a `command_line` in place of the status line (`Mode::new("Find: ").initial(..).history("finder")`), `<n>G`'s G bound with `bind_when` while it is the goto's; find and filter now share an input history (↑/↓), csvlens's, which round 1 left out | ~10 |

The rows' sorting, the record index and the parser stay the example's:
rs-rich-data is not a dependency of the crate, so the CSV layer stays std
as the brief says.

### Behaviour changes

- **The 30% cap counts the clone's two cells of padding**: `max_percent`
  caps the column's whole width, where the clone capped the text (30%
  less 2). A long cell shows two more characters before its `…`
  (`quoted, with comm…` for `quoted, with co…` at 60 columns), and `-S`
  wraps it a little later. Two tests moved with it (the look test's two
  cut cells; `-S` now checked on "biggest little / city"). The padding is
  the round-1 workaround for one `column_spacing`; `Column::padding` would
  end it, and the cap would then be csvlens's exactly.
- **`n` and `N` wrap round** (`View::next_found`): past the last match `n`
  goes to the first. csvlens does too; the clone stopped.
- **Sorting text ignores case**, and a column of numbers and text puts the
  numbers first (`view_of`'s default keys); csvlens compares text by bytes
  and sorts a column as numbers only when every value is one.
- **A pass is much slower on a million rows**: the filter that took 3.3 s
  takes 15.4 s (unoptimised test build). About 8 s is reading every row
  through `TableSource::row` (a lock, a seek, a read, a parse, a cache
  insert and a copy a row; a buffered reader that skips the seek on a
  sequential read saved nothing measurable) and about 7 s is the regex:
  round 1 ran one regex over the whole file's cells, one a line, where a
  `RowTest` gets one row, and the only compiled regex rs-rich-ext offers
  (`KeepLines`) takes a `Text`, built per row. First frame, keys and `G`
  are unchanged (150 ms, 28 ms a key, 31 ms).

### Still hand-built, and why

- **The CSV layer**: the record index on a `spawn_stream`, the parser, the
  sniffer (std; rs-rich-data is not a dependency).
- **The gutter**: a frozen first column whose cells end in `│`, sized to
  the numbers in view, the selected one bold; no row-number option.
- **The selection modes' styles**: `column_highlight` and `theme_styles`
  are fixed when the table is built, so Tab rebuilds the table (`keyed`
  on the mode). `cells_when` alone would not do: the row highlight must go
  and the cell's come in their place.
- **The keys and the help sections** (`KEYS`, `run`, `keymap`): csvlens's,
  through the core's bindings.

### New gaps

| Gap | Needed for | API sketch | Where |
|---|---|---|---|
| **Reactive highlight styles.** `column_highlight` and the `selected`/`selected.cell` theme styles are fixed at build; Tab between row, column and cell modes has to rebuild the table | csvlens's modes; any table with a cursor kind | `TableOptions::column_highlight_when(\|\| bool)`, `theme_styles_with(\|\| ..)` on a node, or a `CursorKind` signal (row, column, cell) that sets the styles | core, generic |
| **A bulk read for passes.** `view_of` reads `row(i)` once a row; a file source would rather hand out rows in order from one buffered read | fast whole-file passes over large sources | `TableSource::rows(range, \|i, cells\| ..)` with a default that calls `row`, which `view_of` uses | core, generic |
| **A compiled matcher on plain text.** A `RowTest` with a regex builds a `Text` per row for `KeepLines`; rs-rich-ext has no `is_match(&str)` | regex filters in `view_of`, the finder, the query module's `Grammar::words` | `rich_ext::text::Pattern::new(re)?.is_match(&str)` (fancy-regex underneath), and a `ViewQuery::keep_matching(pattern)` that tests cells one by one | plugin (rs-rich-ext) + core |
| **A prompt that ends on a key.** `<n>G`: the command line has no "these keys submit"; the clone wraps it and binds `G g` while a signal says the goto is open | vim-like counts, `less`'s prompts | `Mode::submit_keys("G g")` | core, generic (small) |
| **Root keys reach a command line's text.** A `previews_keys` container round the command line takes its letters before the input does (typing `42` reopened the goto with `2`); the line has to sit outside the container with the app's keys | every app with letter keys and a command line | the command line (and any focused text input) seeing printable keys before ancestors' previews, or a note in `previews_keys`'s docs | core, generic |
| **Column padding** (round 1's friction, still open) | the cap above, the `OH…` cut | `Column::padding(left, right)` | core |

Round 1's still open: a row-number gutter, header rules and border joins,
help rows for keys a child handles, and the `data` plugin (a seekable,
indexed CSV source on rs-rich-data).

### Bugs found

**Fixed in 0.0.21:** the previews-keys trap ([2a36216](https://github.com/buchochelliq-labs/rs-rich-cli/commit/2a36216)): a
focused text box takes the keys it uses that have no Ctrl or Alt before a
`previews_keys` container, so the command line now sits in the keys'
container and `<n>G` ends in the goto line's `on_change`; and the filter's
speed ([ee8c474](https://github.com/buchochelliq-labs/rs-rich-cli/commit/ee8c474)): `TableSource::rows` and rs-rich-ext's
`transform::Pattern`, the million-row filter 3.8 s (from 15.4 s) in the
unoptimised build, bounded by an ignored release benchmark.

None that block. The previews-keys interaction above reads as a trap
rather than a bug. `view_of`'s query closure is run again whenever the
source's count changes (the watch reads both), so a `ViewQuery` built in
it gets new `Arc`s each time and the pass restarts on every indexing step
while a query is set; the clone puts the query in a `memo`. Worth a note
in `view_of`'s docs, or a dependency on the count's exactness only.

### Components that would help

| Component | What it would replace | API sketch | Where | Value |
|---|---|---|---|---|
| A `data` plugin CSV source | `Source`, `scan`, `parse_record`, `sniff`, `index_in_background`, `Indexed` (~330 lines) | `data::csv_source(path, Dialect)` as a `TableSource` with `parsed()` | plugin (rs-rich-data) | high |
| A row-number gutter | the gutter column and its width sums (~20 lines) | `TableOptions::row_numbers(\|i\| n)` | core | medium |
| Reactive cursor kind | the `keyed` rebuild per mode | `TableOptions::cursor_kind(Signal<CursorKind>)` | core | medium |
| A compiled matcher | `row_test`, the `Text` per row | `Pattern::is_match` | plugin (rs-rich-ext) | medium |
