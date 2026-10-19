# intuiTUIve gaps: taskwarrior-tui

## Round 1: taskwarrior-tui

[`examples/tasks.rs`](https://github.com/buchochelliq-labs/rs-rich-cli/blob/main/crates/rich-intuituive/examples/tasks.rs)
rebuilds [taskwarrior-tui](https://github.com/kdheepak/taskwarrior-tui),
the Taskwarrior front end, on intuiTUIve with no framework change. It is
2,582 lines (after rustfmt): about 920 are the domain (times and
Taskwarrior's date names, the task model and its JSON, urgency as
Taskwarrior computes it, the store, the filter and modification syntax),
460 the reports (columns, sorting, Taskwarrior's default colour rules,
`task info`, `task projects`, `task timesheet` as text), 515 the app's
state and actions (the command bar's modes, completion, marks, undo), 530
the views and keys, and the rest the app, the sample tasks and `main`.

`task` is not installed here, so the tasks live behind a `Store` trait
(`load`, `commit` as one undoable change, `undo`, `contexts`,
`set_context`, `new_uuid`), implemented by `JsonlStore`: a directory with
`tasks.jsonl` (each task as `task export` writes it, through rs-rich-ext's
`data` JSON parser, so the example needs `--features rich-ext/data`),
`undo.jsonl` (each change's tasks as they were before it) and `taskrc`
(`context.NAME=FILTER`, `context=NAME`). Ids are the working set's,
renumbered on each load as Taskwarrior's are. The tests
([`tests/taskwarrior.rs`](https://github.com/buchochelliq-labs/rs-rich-cli/blob/main/crates/rich-intuituive/tests/taskwarrior.rs),
781 lines, 27 tests; `tests/tasks.rs` was taken by the background-task
tests) fill a store in a temporary directory with a fixed "now" (Monday
2026-10-19 12:00 UTC) and drive the app headless, checking the screen,
its styles and the files.

What is there, with taskwarrior-tui's layout, strings, colours and
default keys:

- **The navigation bar**: `Tasks Projects Timesheet Calendar`, reversed,
  the view in bold, `next [none]` (report and context) at the right; `]`
  and `[` change view.
- **The task report**: the `next` report's columns (ID, Active, Age, P,
  Project, Tag, Due relative, Description with `[n]` annotations, Urg),
  the empty ones dropped as taskwarrior-tui does, sorted by urgency, `• `
  on the selected row; Taskwarrior's default colour rules in their
  precedence (active `rgb555 on rgb410`, overdue `color9`, due and due
  today `color1`, tagged `color2`, the priorities' greys), alternate rows
  `on gray2`, the selection bold, the header underlined. j/k, J/K (a
  page), g/G.
- **The details** under the report (50/50, a rule on top): `task info`'s
  Name/Value rows, annotations under the description, virtual tags, and
  the urgency's terms; Ctrl+E/Ctrl+Y scroll, `z` hides them, `\` puts them
  beside the report.
- **The command bar**: two rows, the title reversed (`Filter Tasks`, `Add
  Task`, `Modify Task 1`, `Annotate Tasks 2,1`, `Jump to Task`), red when
  it fails. `/` filters as you type (Esc puts the filter back), `a` adds,
  `l` logs, `m` modifies (filled with the task's description, tags,
  project, priority and due date), `A` annotates, `:` jumps to an id; Tab
  completes `project:` values, tags, priorities, statuses, date names and
  the attribute names, cycling. Errors (`Additional text must be
  provided.`, `Cannot locate task id 42`, `No undo information
  available`) in a box until a key is pressed, the bar's title `Error`.
- **Actions**: `d` done, `x` delete (`Delete Task 1?` / `Press <x> to
  confirm or <q> to abort.`), `s` start/stop, `t` toggles `+next`, H/M/L/N
  the priority, `y` duplicates, `u` undo (asked: ``Run `task undo`?``), `r`
  reloads. The selection follows the task when its urgency moves it.
- **Marks**: `v` marks, `V` marks or clears all; marked rows show `✔`, and
  every action (and the titles, `Delete Tasks 2,1,3?`) takes the marked
  tasks as one change, which one undo reverts.
- **The filter syntax** (a subset of Taskwarrior's): `project:x` (and its
  subprojects), `project:` (none), `+tag`/`-tag` with the virtual tags
  (`+OVERDUE`, `+DUE`, `+ACTIVE`, `+TODAY`, `+TAGGED`, …), `status:`,
  `priority:`, `due.before:`/`due.after:`, ids (`2,4-5`), words (in the
  description or an annotation). Modifications take the same attributes,
  and dates as Taskwarrior names them (`today`, `tomorrow`, `eod`, `eow`,
  `sow`, `eom`, `eoy`, weekday names, ISO dates and times, `3d`, `2w`).
- **The context menu** (`c`) and **the report menu** (`R`): a box with
  `Context  (type to filter)` over a table (Name, Definition, Active),
  typing filters, ↑/↓ and Enter pick; reports `next`, `list`, `active`,
  `overdue`, `completed`, `all`, each with its columns and sort.
- **Projects** (`task projects`: the tree with counts and `(none)`),
  **Timesheet** (the last four weeks' completed and started tasks) and
  **Calendar**: the year, as many months across as fit (width / 24),
  centred, weeks from Sunday, the report's due dates in their tasks'
  colours, today bold; j/k a year, J/K ten.
- **Help** (`?`): the framework's, from the bindings' descriptions,
  sectioned by mode.

Left out: `e` (edit in `$EDITOR`), `!` (shell command), the user shortcuts
1–9, the command history (Up/Down, `[n / m]` in the title), the
completion pop-up and ghost text (Tab cycles in place instead), `task
log`'s auto-inserted quotes, recurrence, dependencies, waiting and
scheduled tasks, user-defined attributes, keyword colour rules, and
taskwarrior-tui's config (`uda.taskwarrior-tui.*`). Choices: delete and
undo ask first (taskwarrior-tui's `prompt-on-delete` and
`prompt-on-undo`; its defaults are off and Taskwarrior asks itself), done
does not (its default). A due date with no colour rule is underlined on
the calendar, where taskwarrior-tui leaves it plain, so every due date
shows. The months' titles carry the year (`October 2026`, not
`October`): `month` always draws it.

Bespoke, and why: no `Widget` of its own. What stands in for missing
components is counted below: the command bar (a `text` over an `input`
that is `disabled_when` it is not typing, the modes' keys by `in_mode`),
the completion (candidates and cycling over the input's signal), the
marks written into the ID cell, the selection following a task by uuid
after each change, the menus (an `input` over a `table` in a modal), the
year (twelve `month` nodes in a `grid`, how many across read from a
`RectHandle`), and `task info` and `task projects` laid out as text. The
date parsing, urgency, filters and the store are the domain, which a
Taskwarrior plugin would hold.

### New gaps and bugs

| Gap or bug | Needed for | API sketch or repro | Where |
|---|---|---|---|
| A table cannot show marks: `selection_markers` and `marker` are a list's only, so the marks are written into the ID cell (`✔ 2`), and the highlight symbol is fixed (taskwarrior-tui's is `⦿`/`⦾` on a marked/unmarked selected row while marking) | `v`/`V`, bulk actions | `TableOptions::selection_markers(sel, \|i\| key)`, `.marker(\|i\| style)`, `.symbol_for(\|i\| markup)` | core |
| A table has no `cursor_by`: a change of urgency re-sorts the report, so `reload` finds the selected uuid again by hand, as gitui's `Files` did for its tree | the selection following the task (taskwarrior-tui's `selection_fix`) | `TableOptions::cursor_by(\|i\| key)`, as `list` has | core |
| A table's columns are fixed when it is made: hiding the empty ones (taskwarrior-tui's `simplify_table`) and switching reports rebuild the table in a `switch` keyed by the visible set, and the focus target's id changes with each build (kept in a `Signal<Cell<Option<NodeId>>>` written from the builder) | the report's columns, the report menu | `table_columns(\|\| Vec<Column>, rows, sel, options)`; `Column::hide_when_empty()` | core |
| No completion on an input: no pop-up of candidates, no ghost text (taskwarrior-tui has both, Textual's `Suggester` and Bubbles' `textinput` suggestions have the ghost text); Tab is bound on the input and cycles candidates by rewriting the signal | `project:`, tags, dates in the command bar | `input(sig).complete(\|text, caret\| Vec<Completion>)` with `CompleteOptions::popup().ghost().cycle_keys("tab", "shift+tab")`; `Completion { replace: Range, text, detail }` | core |
| No history on an input: Up/Down through earlier entries matching the start of the line, `[3 / 7]` in a title | the command bar (taskwarrior-tui keeps one per action) | `input(sig).history(Signal<Vec<String>>)`, `.history_status() -> Memo<Option<(usize, usize)>>` | core |
| Dates read only as ISO: `Date: FromStr` takes `2026-10-07`; Taskwarrior's names (`today`, `eow`, `monday`, `3d`) are the example's `parse_date` | due dates typed in add/modify/filter | rs-rich-interact: `Date::parse_relative(word, today, WeekStart) -> Option<Date>`, `DateTime::parse_relative(word, now)`, and a `date_names()` list for completion | core (rs-rich-interact) |
| No local time: `Date`, `Time` and `DateTime` are UTC only. The example takes a fixed offset from rs-rich-ext's `fs::local_time(now, "%z")` (chrono, behind `local-time`), which is wrong across a change of summer time | every date shown or parsed | rs-rich-interact: `DateTime::now_local()`, `DateTime::from_unix(secs, Zone::Local)`, `to_unix()` | core (rs-rich-interact) |
| `month` always titles a month `October 2026` in `calendar.title` (bold); taskwarrior-tui titles it `October` and bolds only today's month. `CalendarOptions` are fixed when the node is made, so a style cannot follow the year shown | the Calendar view | `CalendarOptions::title(MonthTitle::Name \| NameYear \| None)`, `.month_style_with(\|first_of_month\| style)` | core |
| A node built later by a `switch` has a new id: moving the focus to "the report" needs the id kept from the builder; `autofocus` only works while nothing else holds the focus | the focus back on the report after a prompt, a menu or a view change | `cx.focus_within(container_id)` (its first focusable node), or `switch(..).keep_focus()` | core |
| A Taskwarrior backend: `task` is not installed here, so the clone has its own store. A plugin would run `task export`/`import`/`modify`/`undo` with `rc.confirmation=off`, read `_projects`, `_tags`, `context` and `show` (colours, reports, urgency coefficients), and watch the data directory (taskwarrior-tui polls `task`); taskchampion (Taskwarrior 3's storage crate) is not in the lockfile | everything taskwarrior-tui does with `task` | plugin `taskwarrior::Task::open(rc)` implementing the example's `Store`, `reports()`, `colour_rules()`; under it, core's `jobs` (a process with output) and the `watch` feature's `watch_path` | plugin |

### Friction with the existing APIs

- **The command bar is three nodes and a mode.** A `text` for the title
  (in a `switch` so an error can restyle the whole row, as `style` is
  fixed), an `input` that is `disabled_when` the mode is not typing (so
  Tab and the focus skip it), and `in_mode` bindings for each prompt.
  Prompts (`Press <x> to confirm or <q> to abort.`) are text written into
  the input's signal. `in_mode` worked well for the prompts' keys; `x`
  means delete in one mode and confirm in the next with no clash.
- **Live filtering needs a watch.** The input sets `cmd`; a `watch`
  copies it to the filter while the mode is Filter, and Esc puts the old
  filter back. `input(..).on_change(..)` would say it where the input is
  made (`PromptOptions::on_change` exists for `cx.prompt`).
- **Pages need the table's height.** J/K move a page as taskwarrior-tui's
  do; the table's own PgUp/PgDn cannot be called, so the example reads a
  `RectHandle` of the report area and halves it when the details are
  under it. A `TableHandle` (`page_down()`, `rows_in_view()`) would do.
- **The focus survives a rebuilt table by luck of the Tab order.** When
  the report's columns change (the only active task done, the details
  moved), the old table leaves and the focus goes to the next focusable
  node, which is the new table only because the input is disabled and
  the details are `no_focus()`.
- **`facts` has no header and no hanging lines.** `task info` is a
  `Name`/`Value` table with the annotations under the description;
  `facts` bolds its keys and has no header, so it is formatted as text in
  a `scroll_with`, whose offset Ctrl+E/Ctrl+Y move.
- **A filterable table is two nodes.** The menus are an `input` over a
  `table` in a modal, ↑/↓ bound on the column to move the table's
  selection while the input has the focus. `fuzzy_list` is one column;
  taskwarrior-tui's menus have three.
- **The year is a grid of months laid out by hand.** How many fit across
  is read from a `RectHandle` in a `memo`, the grid rebuilt in a
  `switch`, centred between two flexible blanks.
- **Escaping.** Every cell and line from the data is escaped (a filter's
  `[none]` in the navigation bar was read as a tag until it was).
- **What worked without friction:** `tabs` with theme styles for a
  reversed bar, `table_with`'s `row_style` for the colour rules and
  `symbol`/`HighlightSpacing::Always` for the indicator, `multi_select`
  for the marks, `cx.modal` and `on_any_key` for the error box,
  `CalendarOptions::events` reading the report (each month redraws when
  the tasks change), and `help_with` from the bindings' sections.

### Components that would help

| Component | What it would replace or enable | API sketch | Where | Value |
|---|---|---|---|---|
| A query language over records | the example's `Filter` (~150 lines): attribute terms, tags, words, ids; Taskwarrior's `or`, `xor`, parentheses and modifiers (`.before`, `.has`, `.startswith`) are not supported; bottom's process search was the same shape | core: `query::parse(text, &Schema) -> Query` over a `Record` trait (`field(name) -> Value`, `tags()`, `text()`), with `and`/`or`/`not`/parens, `name:value`, `name.modifier:value`, `+tag`/`-tag`, typed comparisons; `query.matches(&record)`, errors with a position; plugin: Taskwarrior's syntax and virtual tags on it | core + plugin | high |
| A command line | the command bar (title, modes, inline confirm, error state, completion and history) | core: `command_bar(state)` with `cmd.prompt(title, initial, on_submit)`, `cmd.confirm(question, key, on_yes)`, `cmd.error(text)`; vim's `:` line, lazygit's and k9s's prompts | core | high |
| Input completion and history | Tab cycling by hand, no pop-up, no history | as in the gaps; Textual's `Suggester`, Bubbles' `textinput.SetSuggestions`, rustyline's completer | core | high |
| Natural dates and local time | `parse_date`, the offset trick | as in the gaps; the `chrono-english`/`two_timer` crates do this (not in the lockfile) | core (rs-rich-interact) | high |
| A year view with an agenda | the grid of months, and the tasks of a day (taskwarrior-tui shows none; calcurse, khal and Textual apps do) | core: `year_view(year, YearOptions::new().across(Auto).events(..))`, `agenda(\|date\| Vec<markup>, day)`; plugin: none | core | medium |
| A table with marks, keyed cursor and dynamic columns | the ID-cell marks, `reload`'s uuid search, the `switch` per column set | as in the gaps; Textual's `DataTable` keys rows and adds/removes columns | core | high |
| A transaction log for undo | the store's `undo.jsonl` | core: `UndoLog<C>` (`push(change)`, `undo() -> Option<C>`, `redo`, a limit, serialisable by the app); `history(signal)` covers values in memory only | core | low |
| A Taskwarrior plugin | the `Store`, urgency, reports and colour rules read from `task show` | as in the gaps | plugin | high |
| A tree with columns | `task projects` as text: a tree of projects with a count column | core: `tree_table(columns, items, selected)` (tui-tree-widget plus a table) | core | low |
