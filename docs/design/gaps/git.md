# intuiTUIve gaps: gitui

## Round 1: gitui

[`examples/git.rs`](https://github.com/buchochelliq-labs/rs-rich-cli/blob/main/crates/rich-intuituive/examples/git.rs)
rebuilds [gitui](https://github.com/extrawurst/gitui), the git TUI, on
intuiTUIve with no framework change. It is 2,219 lines (after rustfmt):
about 630 are the repository through the `git` command line (status,
per-file diffs, `git apply` of a hunk or of chosen lines, commit, log,
refs, details, branches, stashes, files at HEAD), 180 the file trees, 210
the diff view, and the rest the tabs, panes, popups and keys. Its tests
([`tests/git.rs`](https://github.com/buchochelliq-labs/rs-rich-cli/blob/main/crates/rich-intuituive/tests/git.rs), 535
lines, 21 tests) build a repository in the temporary directory with the
`git` command line for each test, isolated from the user's and the
system's git configuration, and drive the app headless.

What is there, with gitui's layout, strings, colours and default keys:

- **The top bar**: `Status [1] | Log [2] | Files [3] | Stashing [4] |
  Stashes [5]` over a rule, the repository's path at the right; 1-5,
  Tab and Shift+Tab switch.
- **Status**: the Unstaged Changes and Staged Changes trees (60/40) beside
  the diff (50/50); gitui's status letters and colours (`M`, `+`, `-`,
  `R`, `!`), folders open, single-folder chains folded (`a/b/c`); the
  branch (`↑1 ↓0 {main}`) on the focused list's edge. Enter stages or
  unstages a file or a whole folder, `a` stages (unstages) everything, D
  resets a file after a confirm, `i` adds it to `.gitignore`, `w` moves
  between the lists, → (or `l`) to the diff and ← back.
- **The diff**: gitui's hunk markers (`┌`, `│`, `└`, lit for the cursor's
  hunk), a line cursor, Shift+↑/↓ to select lines, `n`/`p` between hunks;
  Enter stages (unstages) the hunk, `D` resets it, `s` stages (unstages)
  the selected lines, `d` resets them (both asked first), `y` copies them.
  Partial staging builds a one-hunk patch and runs `git apply --cached`
  (`--reverse` to unstage or reset), the lines left out kept as context
  or dropped as `git add -p` does.
- **The commit popup** (`c`, only with something staged): 60% x 20% of
  the screen, `{branch}` on its top edge, `type commit message..`,
  `[subject length: N]` past 50; Ctrl+D commits, Ctrl+A amends (`Commit
  (Amend)`, HEAD's message loaded), Ctrl+E hands the terminal to
  `$GIT_EDITOR`/`core.editor`/`$VISUAL`/`$EDITOR` on `COMMIT_EDITMSG`
  with gitui's comment lines and reads it back without them, Esc closes
  it and keeps the message.
- **Log**: `hash time author <tag> {local} [remote] subject` in gitui's
  colours and widths, `Commit n/N` counted from the bottom as gitui does;
  Enter toggles the details beside it (Info: author, date, committer,
  sha, tags, subject; the files), → inspects a commit full screen (its
  details and files, the diff of the selected file), `y` copies the hash.
- **Files**: the tree at HEAD and the selected file's text, numbered and
  highlighted by its extension.
- **Stashing**: Files to Stash beside the Options box (`[x] stash
  untracked`, `[_] keep index`, `u`/`i`), `s` asks a message and stashes.
  **Stashes**: the list; `a` applies, `p` pops, `D` drops (asked), →
  inspects.
- **The branch list** (`b`): a thick-bordered popup, `Local | Remote`,
  `*` on HEAD and `↑` where tracked; Enter checks out (a remote branch as
  a tracking one), `c` creates, `r` renames, `D` deletes (asked), `m`
  merges.
- **The key bar** (`key_hints`) and **the help** (`h`, `Help: all
  commands`, sectioned by pane), errors in gitui's `Error` box.

Left out: push, pull and fetch (and their progress and credential
popups), blame (`B`), file history (`H`), tags and remotes popups, log
search (`f`) and marking (Space), compare commits, reword, reset to a
commit, checkout of a commit, submodules, the options popup (whitespace
and context lines), the fuzzy file finder, and gitui's theme and key
config files. gitui's log draws no graph, so the clone has none.

Bespoke, and why: no `Widget` of its own. Two helpers stand in for what
is missing, and are counted as gaps below: `titled` (a panel whose title
follows a signal, as a `text` layer over its top edge) and the diff's
row model (`diff_rows` and `Diff`: hunk markers, a range selection, hunk
jumps, over `list_rows`). The git layer is the data source the brief
allows (`std::process::Command`), parsed with rs-rich-ext's
`diff::git::parse_unified`.

### New gaps and bugs

| Gap or bug | Needed for | API sketch or repro | Where |
|---|---|---|---|
| A panel's titles are fixed when it is made. gitui's titles change (`Diff: src/lib.rs`, `Commit 3/10`, `Stashes 1/1`), and are bold while focused and grey otherwise; `titled` lays a `text` over the top edge, so the title's focus look comes from the app's own focus signal, not the panel's | every titled pane in gitui | `.panel_with(\|\| markup)`, `.title_with(Edge, Align, \|\| markup)`; the theme's `title`/`title.focused` styles | core |
| A node shown or hidden by state. `Node::shown_if` exists but is `pub(crate)` (it is how `hide_below` works); the details pane beside the log is a `switch` between two copies of the commit list on one selection signal, with a `watch` moving the focus to the copy shown | Log's Enter (details), any toggled sidebar | `.shown_when(\|\| bool)`: `shown_if` made public | core |
| Sizes that follow state. gitui swaps the lists to 40/60 when the staged one is focused and gives the diff the whole width while it has the focus (the lists 0%); the clone keeps 60/40 and 50/50. Sizes are read when the node is made; a stylesheet's `class_when` changes colours only | gitui's focus-driven layout | `.size_with(\|\| Size)`, or classes that relayout (`class_when` matching `size` rules) | core |
| `tree` keeps its selection and its open folders by index path. Staging a file moves every path after it, so the clone reopens every folder on each refresh (a folder the user closed opens again) and finds the selection again from a remembered row (`Files::new`'s two watches) | the status trees, the Files tab | `tree_keyed(items, key, selected: Signal<Option<K>>, expanded: Signal<HashSet<K>>)` (tui-tree-widget keys by identifier), or `tree(..).cursor_by(\|path\| key)` as `list` has | core |
| `tree`'s rows cannot put anything before the indent: a leaf is indented past the arrow column (`  + notes.md`), gitui puts the status letter first (`+ notes.md`, `M   lib.rs` under a folder) and the arrow on the name (`▾src`); no `selected_style_for` either (the trees use the theme's `selected`, the lists `white on blue`) | gitui's tree look | `TreeItem::prefix(markup)` drawn before the indent; the list's row options (`selected_style_for`, `row_style`) on `tree` | core |
| A range selection on a list: an anchor and the cursor, Shift+↑/↓ (and Shift+click) to extend, the rows between lit. The diff keeps an `anchor` signal and moves the cursor by its own bindings (`Diff::step`), so the list's own ↑/↓ are bypassed. `multi_select`'s visual mode is by key and draws markers, not rows | the diff's line selection | `list(..).range(anchor: Signal<Option<usize>>)`: Shift with the moves extends, the theme's `selected.range` lights the rows, `range()` reads it | core |
| No way to know the framework's focus as state, or to hear it move. gitui's diff follows the list last focused; the clone keeps its own `Focus` signal, moves the real focus to node ids from a `watch`, and adds an `on_click` to each pane to keep the two in step | which list the diff shows | `focus_within(id) -> Memo<bool>`, or `.on_focus(\|cx\| ..)` / `.on_blur` | core |
| `key_hints` has one look: `key desc`, innermost first, `bind_when` bindings left out. gitui's bar is `Stage [⏎] Reset [D] …` in its own order, greys the commands that cannot run now instead of hiding them, and `more [.]` expands it to several lines | gitui's command bar | `key_hints_with(KeyHintsOptions::new().format(\|key, desc\| ..).key_symbols(true).show_disabled(true).expand_key("period"))`; `bind_when` bindings as disabled hints | core |
| A modal's size has no clamp: gitui's branch list is 80% x 50% of the screen but at least 60 x 20 (`rect_inside`) | the branch popup on a small terminal | `ModalOptions::min(w, h)` / `max`, or `Size::Percent(80).at_least(60)` | core |
| A text area's language is set when it is made: the Files tab rebuilds the area per extension in a `switch` on the same text | Files, any viewer of changing files | `.language_with(\|\| name)` | core |
| Markup everywhere: a plain string with brackets (`[subject length: 55]`, a commit subject, diff text) is read as tags unless escaped; the clone escapes ~25 places, and missed one (fixed) | every text from outside the app | `text_plain(\|\| s)`, or `seg`-style spans: `text_spans(\|\| vec![(s, style)])` | core |
| The git plugin's own model: rs-rich-ext's `git` reads status letters for a file manager only; the staged/unstaged split, per-file diffs, `git apply` of a selection, log with refs, details, branches, stashes are the clone's ~630 lines (gitui leans on libgit2 through its `asyncgit` crate) | all of gitui | a `git` plugin (feature): `Repo::status/diff/apply/commit/log/refs/branches/stashes/blame`, each also as a `resource`; under it, none in core (it is git's domain) | plugin |
| Building a patch from part of a hunk (`hunk_patch`): rs-rich-ext parses a unified diff but cannot write one back | partial staging, reset lines, `add -p` | `FilePatch::select(hunk, \|line\| bool, Direction::Forward \| Reverse) -> String`, `Patch::to_string()` | plugin (rs-rich-ext `diff`) |
| All git work runs on the app's thread: fine for a test repository, a frozen frame on a large one. gitui runs everything on worker threads with a spinner, and push/fetch with a progress popup. `spawn`, `resource` and `jobs` exist; what is missing is a process with progress (stderr's `Counting objects: 42%`) and cancellation, and a refresh after the repository changes on disk (the clone polls every 5 s; gitui watches with notify) | push, pull, fetch, big repositories | core: `jobs.run_process(title, Command, parse_progress, done)` (the files round's "Jobs running a process"); plugin: the git plugin's async calls and a `.git` watcher on the `watch` feature | core + plugin |

### Friction with the existing APIs

- **Two sources of focus.** Because gitui's pane focus decides what the
  diff shows, the clone mirrors it in a signal; the framework's focus
  drives the borders, the app's drives the titles. A click on a pane
  sets both only because each pane has an `on_click`.
- **Keys that must beat the tree's.** Enter on a folder stages it in
  gitui; the tree toggles it. Each list's wrapper `previews_keys()` and
  binds Enter, `a`, `D`, `i`; `h` (help) is on the root with
  `previews_keys()` so the tree's `h` (to the parent) never runs. → on a
  leaf bubbling out of the tree to the page's "Diff" binding works as
  gitui does; on a folder the tree opens it, as gitui does too.
- **`list_rows` and the cursor's hunk.** Every row's marker depends on
  which hunk the cursor is in, so the row closure reads the cursor and
  every row is made again on each move.
- **Copies of a pane for a layout change.** The log's two commit lists
  share `log_at`, but each keeps its own scroll offset: with the details
  open the list can show a different first row.
- **Help sections** come from `.name(..)`, which also names the node
  for `assert_focused` and the inspector; gitui's help groups by
  component and shows a description pane for the selected command.
- **`handoff_then` works well** for Ctrl+E, and the headless driver's
  `take_handoff`/`handed_back` made it testable; the app still writes
  the file, quotes `$EDITOR` through `sh -c`, and strips the comments
  (the files round's `cx.edit_text` would be all of it).
- **`bind_when` on `c`** keeps Commit out of the bar and the help when
  nothing is staged, where gitui greys it.

### Components that would help

| Component | What it would replace or enable | API sketch | Where | Value |
|---|---|---|---|---|
| An interactive diff view | `diff_rows`, `Diff` and `diff_bindings` (~280 lines): hunk markers lit for the cursor's hunk, line ranges, `n`/`p`, and also a side-by-side layout (rs-rich-ext's `PatchView` has one, as a renderable only), syntax-highlighted lines (rs-rich-ext's `highlight_lines` is private), word emphasis, folding unchanged lines; delta, lazygit and tig draw these | plugin `diff_view(patch: Signal<Option<FilePatch>>, DiffOptions::new().side_by_side().highlight(lang).on_hunk(..).on_lines(..))`; the generic core shape under it: a **sectioned list** (rows in sections, a gutter marker per section lit for the cursor's, jump to the next/previous section, a range kept within a section) | core (sectioned list) + plugin (diff) | high |
| A git plugin | the clone's `Git` (~630 lines) and the async work gitui does on threads | `git::repo(path)` with `status`, `diff`, `stage/unstage/reset(paths)`, `apply(selection)`, `commit`, `log` (paged, with refs), `branches`, `stashes`, `blame`, `push/fetch` as jobs with progress; a `.git` watcher (feature `watch`) | plugin (none in core: git's domain) | high |
| A keyed tree | `Files` (the selection and open folders found again after each refresh) | `tree_keyed`: items with keys, selection and expansion by key, `prefix` before the indent, the list's row options | core | high |
| A commit message editor | the commit popup's layers (branch on the edge, the subject warning) and the editor round trip | core: `text_area(..).line_style(\|n, line\| ..)` (dim `#` lines, a red subject past 50), `.ruler(72)`, `cx.edit_text(initial, ..)`; plugin: `git::message_editor(sig)` with the conventions (subject 50/body 72, comment lines, co-author trailers) | core + plugin | medium |
| A command bar | `key_hints` in gitui's (or lazygit's) look | `key_hints_with(..)` as above; Bubbles' `help` short/full toggle, Textual's `Footer` with `show` priorities | core | medium |
| A blame view | gitui's `B`: the file with each line's commit, author and age in a gutter, Enter to the commit | core: a text view with a per-line gutter (`text_area(..).gutter(\|line\| markup)` or `list_rows` with a leading column); plugin: `git::blame` parsing `git blame --porcelain` | core + plugin | medium |
| A lane graph for a DAG | a commit graph in the log (lazygit, tig, gitu draw one; gitui's is an open request) | core: `dag_lanes(rows: &[(id, parents)]) -> Vec<String>` (the glyphs per row, `│ ├─╮ ╰─╯`), any DAG (a build graph, a task tree); plugin: the git log's | core + plugin | medium |
| A progress popup for a running process | push/pull/fetch with stages (`adding objects (1/3)`) and a credential prompt | `jobs.run_process` above, `jobs_panel` in a modal; `cx.prompt_with(.., mask('•'))` for the password exists | core + plugin | medium |
| Reactive layout: `shown_when`, `size_with` | the log's two lists, gitui's 0/100 and 40/60 layouts | as in the gaps | core | medium |
