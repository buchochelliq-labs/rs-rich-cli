# Approval queues and jumping to anything

Two components for scripts and tools that act on many things at once
(rs-rich-interact, 0.0.19 workstream 20):

- an [approval queue](#the-approval-queue) (#471), to review pending items
  one by one or in bulk and approve, reject or skip each, with a reason;
- a [jump navigator](#the-jump-navigator) (#484), one fuzzy box over
  several sources (files, headings, symbols, commands, recent places) with
  the results grouped by source.

Both are built on the same pieces as `Select`: the [item model](index.md#items)
with its previews, the [fuzzy](index.md#select-and-multiselect) matcher,
the kit's `ListState` and `TextBuffer`, and a [keymap](index.md#keys). Each
runs under `run` and the event loop, is tested headless, and degrades to a
line prompt without a terminal. `rich approve` and `rich jump` run them
from the shell: see [Review and jump](../../cli.md#review-and-jump).

## The approval queue

`ApprovalQueue::new(prompt, items)` lists the items, each with a mark for
its decision: `·` pending, `✓` approved, `✗` rejected and `↷` skipped. A
pane beside the list (below it on a narrow terminal) shows the focused
item: its label, description, details (`Item::meta`), its decision and
reason, then its `Preview`. Enter submits an `ApprovalReport`: one
`Decision` for every item, in order, with its `value`, `verdict` and
`reason`. Pending items are in the report too, so a review can stop
half-way and pick up later (`.decided(index, verdict, reason)`).

The queue only records decisions. Carrying them out (deleting what was
approved, sending the rejections back) is the caller's.

| Keys | What they do |
|------|--------------|
| `a`, `r`, `s` | Approve, reject or skip, then move on to the next pending item |
| `A`, `R`, `S` | The same, asking for a reason first (Enter confirms it, Esc backs out) |
| `u` | Back to pending |
| Space | Mark the item; a decision then applies to every marked item |
| Ctrl+A | Mark every pending item (again: unmark them) |
| `n`, Tab | The next pending item |
| ↑ ↓ (or `k` `j`), PgUp PgDn, Home End (or `g` `G`) | Move |
| `p` | Show or hide the detail pane |
| Enter, Esc | Submit; cancel |

`.require_reason(Verdict::Rejected)` asks for a reason on every rejection,
whichever key made it, and refuses an empty one. `.auto_submit(true)`
submits as soon as nothing is pending. `.advance(false)` keeps the cursor
where it is after a decision. The keys are in context `approve` and can be
rebound (`.rebind("approve", keys("y"))`).

With the mouse on (`.with_mouse(true)`), a click focuses a row, a click on
the cell after the pointer marks it, the wheel moves, and the buttons under
the list (`[a] approve`, `[r] reject`, `[s] skip`, `[u] undo`, `[enter]
submit`) are buttons.

```rust
use rich_interact::components::{ApprovalQueue, Verdict};
use rich_interact::{run, Item, Outcome, Preview, RunOptions};

let items = requests.iter().map(|request| {
    Item::new(request.id, &request.title)
        .description(&request.author)
        .meta("size", request.size.to_string())
        .preview(Preview::Text(request.diff.clone()))
});
let queue = ApprovalQueue::new("Requests", items).require_reason(Verdict::Rejected);
if let Outcome::Done(report) = run(queue, &RunOptions::default())? {
    for decision in report.with(Verdict::Rejected) {
        reply(decision.value, decision.reason.as_deref().unwrap_or(""));
    }
    eprintln!("{}", report.summary()); // 3 approved · 1 rejected · 0 skipped · 2 pending
}
```

### Theme keys

Decisions are drawn in the console theme's styles, so a theme restyles
them without code:

| Theme key | Default |
|-----------|---------|
| `approve.approved` | `bold green` |
| `approve.rejected` | `bold red` |
| `approve.skipped` | `yellow` |
| `approve.pending` | `dim` |

The marks read without colour as well. The rest (the pointer, hints, the
answer) comes from the component's [`Theme`](index.md#components), as for
every built-in.

### Without a terminal

It asks item by item, showing the label, description and details:
`a`, `r` or `s` (or `yes`, `no`, `skip`), with a reason after a colon
(`r: not this week`). An empty line skips, `q` stops, and the end of input
leaves the rest pending; a required reason is asked for again. Asked for
defaults, it answers with the report as it stands: nothing decided.

## The jump navigator

A `JumpIndex` is a searchable index over `JumpSource`s, each an id, a title
and `Item`s, with an optional prefix character and a limit.
`JumpIndex::search(query, only)` ranks every source's items and returns
`JumpGroup`s, best first within each, so the index serves a palette of your
own or a test without the component. `Jump` is the component over it:

```rust
use rich_interact::components::{Jump, JumpSource};
use rich_interact::{run, Item, Outcome, Preview, RunOptions};

let jump = Jump::new("Jump")
    .source(JumpSource::new("recent", "Recent").items(recent))
    .source(JumpSource::new("files", "Files").items(files))
    .source(JumpSource::new("headings", "Headings").prefix('#').items(headings))
    .source(JumpSource::new("commands", "Commands").prefix('>').items(commands));
if let Outcome::Done(jumped) = run(jump, &RunOptions::default())? {
    open(&jumped.source, jumped.value);
}
```

Typing searches every source at once. Results are grouped under their
source's title, in the order the sources were added, and the best match
overall is focused. Until the search is narrowed, a group shows its best
few (`JumpSource::limit`, 5 by default) and says how many more matched.

| Keys | What they do |
|------|--------------|
| Typing, Backspace, Ctrl+U | Search; delete; clear |
| A source's prefix first (`>`, `#`, `@`) | Search that source only |
| Tab, Shift+Tab | Narrow to the next source, or back towards every source |
| ↑ ↓ (Ctrl+P Ctrl+N), PgUp PgDn, Home End | Move across the groups |
| Alt+↑ ↓ | The previous or next group |
| Enter, Esc | Jump; cancel |

Enter returns a `Jumped`: the source's `id`, the item's `index` in it, its
`label` and its `value`. The focused item's `Preview` shows beside the list
(below it when narrow). With the mouse on, a click focuses a result and a
second click picks it, a click on a group's title narrows to that source,
and the wheel moves. The keys are in context `jump`.

Group titles use the theme keys `jump.group` (default `bold`) and
`jump.more` (`dim`).

**Without a terminal** it reads one line and answers with its best match.
Asked for defaults, it answers with the best match for the query it started
with (`.query(..)`), and has none when nothing was typed.

## Testing them

Both run in the [headless](index.md#testing-components) driver:

```rust
use rich_interact::components::{ApprovalQueue, Verdict};
use rich_interact::{headless, Outcome};

let queue = ApprovalQueue::new("Deploys", ["api", "web", "db"]);
let script = headless::Script::new().keys("a R").text("flaky").keys("enter s enter");
let (outcome, record) = headless::run(queue, script, 80, 12);
let Outcome::Done(report) = outcome.unwrap() else { panic!() };
assert_eq!(report.verdicts(), [Verdict::Approved, Verdict::Rejected, Verdict::Skipped]);
assert!(record.frames[0].contains("0/3 decided"));
```

The Python bindings are `rs_rich.interact.ApprovalQueue` and `Jump`: see
[the Python guide](../../python/interact.md#approvalqueue-and-jump).
