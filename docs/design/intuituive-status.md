# intuiTUIve demo loop: where things stand

Status: handoff note, updated when a session ends mid-round. The loop and its
rules are in [the demo gaps](intuituive-demo-gaps.md).

## Where things stand (2026-10-20)

- **Rounds 2 to 7 are merged and published.** The loop's branch merged to
  `main` as #757; the 0.0.20 cohort was published on 2026-10-19 from
  `56c6c97` by one release run (`release-0.0.20`; record tags
  `<crate>-v<version>` for all 17 crates; see the
  [0.0.20 release notes](../releases/0.0.20.md)), with `rs-rich` 0.0.8 on
  PyPI (`python-v0.0.8`).
- **0.0.21 is in progress** ([plan](../plans/0.0.21.md)); its versions are
  open. Its first wave is running now, each workstream on a
  branch of its own: round 7's bugs and regressions (workstream 1), the
  DevTools pipe on Linux and macOS (workstream 8), and the marketplace's
  listing format and schema (workstream 7).
- **Next:** the four parity workstreams (3 to 6) and the marketplace
  crate; then round 7's shared gaps (workstream 2) and a re-run of the
  eleven clones' suites and tapes; then the release test. Moving
  rs-rich-doom onto the 0.0.20 crates is part of workstream 1.

The sections below are the loop's history, kept as they were written.

## Done and on this branch

- Rounds 1–4: Yazi (`examples/files.rs`) and scope-tui (`examples/scope.rs`)
  rebuilt each round; round 4 added the runtime, widgets, files plugin,
  sound/charts and the `viz` plugin (meter, sparkline, spectrogram and bar
  spectrum views out of core).
- Round 5 core (`r5-core`, merged): autofocus through wrappers, help wrapping
  and `/` filter, scaled/modifier-blind/fall-through/hidden bindings,
  `Ctx::now`, played feeds with state, `Latest::wait_for`, `recent_of` /
  `recent_timed`, `falling`, Miller overlay, `marks_by`, `next_from`,
  `tabs_for_indexed`, `StatusSegment::when`, `facts` value styles,
  `cx.edit_text`, the `state` module, `FrecencyStore`, `Jobs::run_process`.
- Round 5 clones (round 1 of each; reports in [`gaps/`](https://github.com/buchochelliq-labs/rs-rich-cli/tree/main/docs/design/gaps)): gitui
  (`git`), bottom (`top`), slumber (`http`), rata-tetris (`blocks`), csvlens
  (`csv`), diskonaut (`disk`), termusic (`music`), taskwarrior-tui (`tasks`,
  test `taskwarrior`), and a Doom-style raycaster (`raycast`). Sprites
  (`Sprite`, `stage`, `pixels`, sprite sheets, micro assets) merged. All ten clone suites pass on the merged branch; the
  gap note's "Round 5: nine new apps" section lists the shared gaps.

## In flight when this note was written

Each on its own local branch; pushed as a backup branch named
`claude/next-priority-items-3c5p8e-wip-<name>` where noted.

| Work | Branch | State |
|---|---|---|
| Round 5 plugins: files rebuilt on round-5 core, per-tab selection, Miller navigation, links, file picker, external previewers; one audio source type, `Pcm` errors, optional `stall_after`, tuner, `viz` persistence, audio output + player on a core `transport`, images from bytes, Kitty/Sixel | `r5-plugins` (wip-plugins) | merged (065f0f5) |
| Real Doom (GPL, separate repo) | [rs-rich-doom](https://github.com/buchochelliq-labs/rs-rich-doom) `claude/doom-frontend` (62008c2) | working: half/Braille/Kitty/Sixel/iTerm2 at 33–35 fps, SFX; not seen in a real Kitty/Sixel terminal yet; no PR |

## Next

1. Finish and merge the in-flight branches (check each one's commit
   message and status notes; run the gate in `scripts` style: fmt, clippy
   `--all-features`, the intuiTUIve/ext/interact/art tests, each clone's
   test with its features, the tapes check).
2. Round 6: the shared gaps of round 5 (panel titles and borders from
   state, `shown_when`/`size_with`, focus scopes, `keyed(key, build)`,
   reactive tables, keyed tree, input completion/history/command line,
   query language, timers with a handle, key repeat and releases, spatial
   focus, treemap layout, natural dates, `App::min_size`) and the bugs it
   found; then rebuild all eleven clones on them.
3. Release bumps owed on main once the 0.0.19 cohort is out: web, data and
   cli changed since 2fc9772.

All three done: the branches merged, round 6 ran (below), and the bumps
went into the 0.0.20 cohort.

## Round 6 (fix round)

Brief: `/home/user/apps/R6BRIEF.md` in the session (its rules: regression
test first, generic core, stay in your area).

| Agent | Branch | Area | State |
|---|---|---|---|
| A | `r6-render` | style table compaction, Fx interning, `put_rgb` pixel cells, `DrawCx::rect()` | merged |
| B | `r6-input` | input drained per frame, start-up keys, Shift+arrows, `Driver::send`; key state and releases, every key, DAS/ARR repeat, `Timer` handles, `fixed_step`, `max_fps`, stats | merged |
| C | `r6-react` | `.auto()` switches, layers that composite, `keep_last`, titles/borders from state, `shown_when`/`size_with`, focus scopes and direction, `keyed`, treemap, `App::min_size`, fitted text, `key_hints_with`, whole-app help | merged; layers then made single-pass (05305b7: each layer draws once per change, no second pass) |
| D | `r6-tables` | reactive tables and columns, markers, keyed cursor, offset across, widths and priorities, row heights, keyed/lazy trees, list ranges, `TableSource` and `view_of` | merged |
| E | `r6-inputs` | completion, history, command line, query language, natural dates, calendar options | merged (e4bc67c; full gate green) |
| F | `r6-parity` | `Text.markup` port with goldens; template manifest | merged |
| G | `r6-record` | rich-record graphics capture, every frame at a set fps, held keys, APNG; raycaster and Doom videos | merged; Doom videos on rs-rich-doom `claude/doom-frontend` (114c39a) |

Found by G for the rebuild: the raycaster's Kitty mode draws its image
under cells with the root's background, so nothing shows (give the view the
default background, or place at z below -2^30); its held arrow keys count as
held only after a release was seen.

## Round 7: the clones rebuilt on round 6

Brief: `/home/user/apps/R7BRIEF.md` in the session (clones use components only; no library edits;
each clone's gap report gets a "Round 2" section). From e4bc67c:

| Agent | Branch | Clones | State |
|---|---|---|---|
| A | `r7-a` | git, tasks (taskwarrior), http | merged |
| B | `r7-b` | disk, top, csv | merged |
| C | `r7-c` | blocks, raycast (with its Kitty-mode and held-key fixes), music | merged |

Merged, and folded into the gap note's "Round 7" section (bugs, csv's filter
regression, shared gaps). The PR is rs-rich-cli #757; rs-rich-doom #1 is
merged. Core went to 0.0.10 with its published dependents (d35d906), since
the `Text.markup` port changed a published file.

Next: the round 7 gaps (round 8 of the loop): the four bugs first, then the
shared gaps, then rebuild again. rs-rich-doom is pinned to
866b897 (before round 6); moving it onto pixel cells, held keys, `fixed_step` and stats is open.
The [0.0.21 plan](../plans/0.0.21.md) took these up: the four bugs, csv's
filter and rs-rich-doom in workstream 1, the gaps three clones share in
workstream 2; the rest waits for a round 8, if it runs.

**When all of it has landed and the full gate passes: open two PRs** (the
user asked for this): rs-rich-cli `claude/next-priority-items-3c5p8e` →
`main` (PR template, release handoff comment, drive CI and review), and
rs-rich-doom `claude/doom-frontend` → `main`. Done: rs-rich-cli #757 and
rs-rich-doom #1 are merged, and 0.0.20 is published (see the top of this
note).
