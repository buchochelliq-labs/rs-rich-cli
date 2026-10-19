# intuiTUIve demo loop: where things stand

Status: handoff note, updated when a session ends mid-round. The loop and its
rules are in [the demo gaps](intuituive-demo-gaps.md).

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
- Round 5 clones (round 1 of each; reports in [`gaps/`](gaps/)): gitui
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
| Round 5 plugins: files rebuilt on round-5 core, per-tab selection, Miller navigation, links, file picker, external previewers; one audio source type, `Pcm` errors, optional `stall_after`, tuner, `viz` persistence, audio output + player on a core `transport`, images from bytes, Kitty/Sixel | `r5-plugins` (wip-plugins) | in progress |
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
