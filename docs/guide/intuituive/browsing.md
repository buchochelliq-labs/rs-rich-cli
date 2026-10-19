# Browsing, jobs and app state

Generic pieces that apps of many kinds rebuild by hand: Miller columns
over any tree, background jobs with progress, a streaming fuzzy list, a
preview pane, editing text in the user's editor, state kept between
sessions, and processes run as jobs.

## Browsing, jobs, finding and previewing

Four components that apps of many kinds rebuild by hand are in the core,
generic over what they show: none knows about files, so a file manager, a
menu, a package browser or a database client builds on the same pieces.
The `files` plugin ([a file manager's components](plugins.md#a-file-managers-components))
is each of them with a file system plugged in.

### Miller columns

```rust
struct Menu;
impl MillerSource for Menu {
    type Key = String;
    fn items(&self, at: &String) -> Vec<String> { /* the level's names */ }
    fn child(&self, at: &String, name: &str) -> Option<String> { /* a submenu */ }
    fn parent(&self, at: &String) -> Option<(String, String)> { /* up, and our name there */ }
}
let columns = miller_of(Menu, String::new());
columns.navigation(columns.view())
```

- **Miller columns over any tree:** `miller_of(source, start)` takes a
  `MillerSource` (the items of a level by key, the level an item opens,
  the level above; `arrive` runs when the columns get to a level) and is
  the level above, the current level and a preview of the hovered item,
  in panes whose rails the mouse drags, as signals (`at`, `items`,
  `cursor`, `hovered`) in a `Copy` struct. Each level remembers the item
  last hovered in it; a click in the parent column goes there
  (`click_parent`), one in the preview enters it (`enter_at`).
  `columns.navigation(node)` binds `j`/`k`, `h`/`l`, `g g`/`G`, Ctrl+D/U
  and PgDn/PgUp; `cd`, `enter`, `leave`, `arrow`, `page` and `keep` are
  there for your own. `view()` draws names in lists; `layout(parent,
  current, preview)` takes columns of your own (`track_current` marks the
  current one), and `miller_in(source, at)` takes a current-level signal
  made first, so the source can follow it. Each signal has a method
  (`at()`, `cursor()`, `hovered()`, `items()`, `first()`, the parent's,
  `preview_cursor()`, `remembered_all()`), and `visited()` is the levels
  shown, the most recent last.
- **Your own keys and wording:** `navigation_with(node, &[(Move::Up, "k
  up", "Previous file"), ..])` binds a table of moves, keys and the
  descriptions the help shows, where `navigation`'s are fixed; `go(Move)`
  makes any move from a binding of your own (a sequence:
  `.bind_sequence("g g", "Top", move |_| m.go(Move::Top))`).
- **Options on the columns' lists:** `view_with(MillerOptions::new()
  .current(|list| list.highlight_layer(HighlightLayer::Under)))` hands
  each default column's list to a hook (`current`, `parent`, `preview`),
  so any list option or handler reaches it; a view of your own passes
  its lists through `wrap_current` and the like.
- **Other items at a level:** `m.overlay(at, vec![(name, Some(level)),
  (name, None)])` goes to level `at` and lists those items in place of
  its own, each opening a level of its own (or a leaf): a search's
  results, bookmarks, a "recent" list. It lasts until the columns leave
  the level: entering an item goes to its level, and `leave` (or
  `end_overlay`) goes back to the level's own items, on the one hovered
  before, as Yazi leaves a search (the source's `arrive` runs then too).
  `overlay_at()` says where one is, and `update_overlay(items)` replaces
  its items in place, the cursor kept: results arriving while a search
  runs.


### Jobs with progress

```rust
use intuituive::{jobs, jobs_panel};

let list = jobs::<f64>();
column([body.flex(1), jobs_panel(list).fixed(4)]).on_key("r", move |_| {
    list.run(
        "Reindex",
        |cancel, progress| {
            for n in 1..=100 {
                if cancel.is_cancelled() {
                    break;
                }
                reindex_part(n);
                progress(&(n as f64 / 100.0));
            }
            Ok::<_, String>(())
        },
        |_, cx| cx.toast("Reindexed"),
    );
})
```

- **Jobs with progress:** `jobs::<P>()` runs any work as a cancellable
  job: `list.run(title, |cancel, progress| { ..; outcome }, |outcome, cx|
  ..)` on a thread of its own, its progress (`P: JobProgress`: a share
  done and a detail; plain `f64` is one) reaching a signal at most every
  50 ms. The outcome (`JobOutcome`: `()`, `Result<T, String>`, or your
  own) says what failed: a clean or cancelled job leaves the list, a
  failed one stays until `clear_finished`. `jobs_panel(list)` lists them
  with progress bars (`x` cancels the hovered job, `X` all, `c` clears the
  failed); `jobs_panel_with(list, JobsPanelOptions::new()
  .failed_title(false))` shows a failed job's error without its title.

### A streaming fuzzy list

```rust
use intuituive::finder::{fuzzy_list_with, FuzzyOptions, FuzzySource};

cx.modal(Size::Percent(60), Size::Percent(60), move || {
    fuzzy_list_with(
        FuzzySource::iter(commands()),
        |c: &Command| c.name.clone(),
        |c, cx| { cx.pop(); run(c, cx) },
        FuzzyOptions::new().title("Run").border(Border::Rounded),
    )
});
```

- **A streaming fuzzy list:** `fuzzy_list(source, |item| text, |item,
  cx| ..)` is a fuzzy list over any source of items, read on its own
  thread: `FuzzySource::iter(items)` for an iterator, or
  `FuzzySource::feed(|feed| ..)` for a producer pushing items as they
  arrive (a search's results), stopping once `feed.push` says false.
  Results show while it goes on (`12/3400 (loading…)`), a new batch is
  matched alone, Enter picks the hovered item (the item, not its text),
  Esc closes the screen or modal it is on (on the first screen it leaves
  Esc to the app: `cx.is_modal()` says which a handler runs on), and the
  source stops when the screen closes. `fuzzy_list_with(.., FuzzyOptions::new().title("Jump")
  .border(Border::Rounded).prompt(":").on_cancel(..))` adds a panel, a
  prompt, or an Esc of your own. **`finder_of(items, ..)`** is one over an
  iterator, with the same look and keys.

### A preview pane

```rust
use intuituive::preview_of;

preview_of(
    move |_width| rows.with(|r| r.get(cursor.get()).map(|r| r.id)),  // None: nothing hovered
    |id| load_record(*id),                                            // on a worker thread
    |record| text(move || record.with(|r| r.summary())),
)
```

- **A preview pane:** `preview_of(|width| key, |key| value, |state| node)`
  loads a value off the app's thread whenever the key changes (it may
  read signals, and gets the pane's width), drops results for keys left
  behind, and draws the value from a signal. `preview_of_with(..,
  PreviewOfOptions::new().state(sig).clear_after(d).on_change(..)
  .on_loaded(..))` shares the value, clears a slow one, resets a scroll
  when the key changes and acts on a value as it arrives. It is a
  `resource_for` keyed by the width and the key (`clear_after` its
  `stale_after`, on the app's clock), so the first frame lays it out and
  already shows the first value.

## Editing text, state files and process jobs

```rust
// $VISUAL, else $EDITOR, on a temporary file; read back when it ends.
cx.edit_text_as(&message, "md", move |edited, cx| match edited {
    Ok(text) => commit(text, cx),
    Err(why) => cx.notify(Level::Error, "Editor", why.to_string()),
});

// State between sessions, by app name.
let recent = frecency_in("launcher", "recent.tsv").keep(200);
recent.visit("build");
let best: Vec<String> = recent.ranked();
update_state("launcher", "history", |old| append(old, "build"))?;

// A process as a job: progress from its lines, killed on cancel.
jobs.run_process_with("Fetch", command, |line| parse_percent(line), |outcome, cx| ..);
```

- **`cx.edit_text(text, |edited, cx| ..)`** puts `text` in a temporary
  file, hands the terminal to the user's editor on it (`$VISUAL`, else
  `$EDITOR`, else `vi`; Notepad on Windows) and gives back the file's
  text once it ends, or why it could not run; `edit_text_as(text, "md",
  ..)` names the file's extension. Under the headless driver the editor
  is a recorded `Handoff`, as any `handoff`.
- **State files:** `state_dir(app)` is where an app keeps state
  (`$XDG_STATE_HOME/app`, else `~/.local/state/app`; Application Support
  on macOS; `%LOCALAPPDATA%` on Windows), `state_file(app, name)` a file
  in it; `write_state(app, name, bytes)` writes it whole (a temporary
  file synced and renamed over it: `write_atomic(path, bytes)` for any
  path), `read_state` reads it, and `update_state(app, name, |old|
  new)` (`update_file` for any path) reads, changes and writes it under
  a lock another session waits for, so two windows' changes add up.
- **A frecency store:** `frecency_in(app, name)` (or
  `frecency_store(file)`, or `frecency_session()` for the session only)
  ranks string keys by how often and how lately each was used, as zoxide
  does: `visit`, `remove`, `ranked`, `visits`, `reload`; `keep(n)` caps
  it, `max_age(total)` sets when the ranks age. Each visit is merged into
  the file's latest under its lock.
- **Process jobs:** `jobs.run_process(title, command, |outcome, cx| ..)`
  runs a command as a job, its output read line by line; cancelling the
  job kills it. A process that exits otherwise than with 0 fails the job
  with the last line of its standard error (`ProcessOutcome::Failed(why,
  output)`); `run_process_with` reads progress from each line.
  `job.errors()` is a failed job's errors.
