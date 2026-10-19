# blocks: a falling-block game on intuiTUIve

## Round 1: rata-tetris

The clone is [`examples/blocks.rs`](../../../crates/rich-intuituive/examples/blocks.rs),
a rebuild of [rata-tetris](https://github.com/nw-wood/rata-tetris) (Nathanyel
Wood, MIT; ratatui 0.29, the most-downloaded ratatui Tetris on crates.io whose
source is short enough to follow screen by screen). quantonganh's `tetris-tui`
is better known but draws with crossterm alone, so it was not the model.

What was built (919 lines: about 545 the game model with no framework in it,
195 the look, 150 the app; tests 390 lines in `tests/blocks.rs`, 15 tests):

- **The original's screens**, at its places on its fixed 64x28 screen, centred:
  the start screen (block-letter title and the zoom tip), the playing screen
  (statistics with each piece's art and count, `LINES - 000`, the board, `TOP`
  and `SCORE`, `NEXT`, `LEVEL 00`), the paused screen (the board hidden, as on
  the NES), the game-over screen (its lettering, "Reach level n, and cleared n
  lines.", "New top score set at n!" or "You scored n points."), and "Terminal
  must be at least 64 x 28!" below that size. Its ten level palettes (xterm
  colours) recolour the pieces every level.
- **Its rules:** a 10x20 field, a shuffled bag of seven (seeded here), its
  rotation matrices with no kicks (it has no SRS), gravity in NES frames (48 at
  level 0 down to 1, its table), a level every ten lines, 40/100/300/1200 times
  the level plus one, lock on contact (no lock delay), top-out when a new piece
  cannot fall, the ghost (`┌┐` in the piece's colour, as its bordered `Block`
  drew it), the cleared rows' 300 ms + 100 ms a row pause, and a top score kept
  in a file.
- **Its keys:** Space starts, pauses, resumes and plays again; ← → move; ↓ soft
  drop; ↑ slam (hard drop); PageUp/PageDown rotate; `q` quits.
- **Added:** a hold panel (`c`, once a piece; the original has none), and the
  cleared rows flash (lit/dark every four frames; the original shows them
  still). Fixed on the way: the original takes input during the clear pause.
- **The clock:** one `every(17 ms)` tick is a NES frame (the original's timer
  multiplies frames by 17 ms). The model counts frames; a `View` signal with
  `PartialEq` means a frame where nothing shown changed draws nothing. Tests
  seed the bag, script keys with `Driver::press` and step the clock with
  `Driver::advance`, so each run is the same game.

**Bespoke drawing:** the playfield only, and it is not a widget: a `text`
node whose markup is twenty rows of ten `[color(n) on color(234)]██[/]` cells
(25 lines). The side panels are plain `label`/`text` nodes in `.borders(..)`
boxes, placed with `layers` and `.inset`.

**Left out:** sound (rata-tetris ships fourteen WAVs, wired to rodio and
commented out: there is no sound output to wire them to here either); the
backdrop's tiled pattern of grey blocks (one glyph instead, see below); its
zoom-tip emoji.

### New gaps and bugs

| Gap or bug | Needed for | API sketch or repro | Where |
|---|---|---|---|
| **Bug: keys typed during the start-up probe are lost** | Pressing Space as the game opens | With a terminal that never answers the probe (`\x1b[c`), keys sent in the first 2–3 s are dropped: a PTY that writes `q` at 0.1, 0.5 or 1.5 s does not quit, the same `q` at 3 s does. Keys that arrive before the answer should be queued, not discarded | core (rs-rich-interact's session start) |
| A fixed-rate tick that catches up | Gravity counted in frames: `every` ticks once after falling behind, so a stalled loop (a slow frame, a suspended process, a busy machine) slows the game down instead of dropping the piece the rows it owes | `every(d, f).catch_up(Catch::All)`, or `fixed_rate(d, \|cx, ticks\| ..)` given how many ticks are due | core |
| A timer that can change, pause and stop | A level-up speeds gravity; pause stops it; the start and game-over screens need no tick. `every` returns nothing, so the example ticks at 60 Hz for the app's whole life and counts frames itself | `let t = every(..); t.set_interval(d); t.pause(); t.resume(); t.cancel()`, or `every_with(move \|\| interval.get(), f)` (`None` = stopped) | core |
| Key repeat under the app's control (DAS/ARR) | Holding ← moves at the terminal's autorepeat (often 500 ms, then 30/s, set by the OS); a game wants its own delay and rate, and the first press at once | `node.on_key_repeat("left", Repeat { delay: ms(167), every: ms(33) }, f)`, built on press and release; with no release events (no kitty protocol) it falls back to the terminal's repeat | core |
| Key releases for bindings, and for plain keys | Soft drop while ↓ is held; a repeat that stops on release. `KeyUp` reaches only a `Widget`'s `event`, and plain letters, Space and Enter never send one (the kitty flag that reports all keys as escapes is not asked for) | `node.on_key_up("down", f)`; `App::key_releases(KeyReleases::All)` asking for the kitty "report all keys" flag | core |
| Layers are opaque | A backdrop under the centred screen: `layers([fill(..), column([..]).justify(Center)])` shows only blanks round the content, as the upper layer paints its whole rectangle. The example rings the screen with four `fill`s instead | `layers` child `.transparent()` (its blank, unstyled cells let the one below show), or `Node::backdrop(glyph, style)` | core |
| A fill with a pattern | The original's backdrop tiles a 12x12 pattern of four grey `██` shades | `fill_pattern(&["1121..", ..], [("1", "color(237)"), ..])` or `fill_by(\|x, y\| (glyph, style))` | core |
| Placing a node at a rectangle | The original lays out by absolute rects on a fixed screen; `.inset(top, right, bottom, left)` needs right and bottom worked out from the stage's size (the example's `at()` helper) | in `layers`: `.at(x, y, width, height)`, or `.rect(Rect)` | core |
| A blink or keyframe animation | The cleared rows' flash; `cx.animate` moves an `f64` only, so the flash is counted in the model's frames | `blink(period)` → `Signal<bool>` on the app's clock, or `keyframes([(ms(0), a), (ms(68), b)]).repeat(n)` | core |
| Sound out | Move, rotate, lock, line, tetris, pause and game-over sounds (the original's WAVs). The `audio` plugin captures, and `audio::play` plays PCM onto a signal for a view: nothing reaches a speaker | `audio::output()?.play(audio::wav_bytes(include_bytes!(..)))`, fire-and-forget, mixed, on cpal (already the plugin's) | plugin (`audio` + `wav`); no core shape: a sound card is a device |
| A state directory for the app | The top score: the example works out `$XDG_STATE_HOME`/`~/.local/state` by hand (and would get Windows' and macOS' places wrong); the original writes `~/top_score` | `App::state_dir("blocks") -> io::Result<PathBuf>`, and `persisted(signal, "top_score")` that loads at start and saves on write (serde behind a feature) | core (generic: a key and a value, no domain) |

### Friction with the existing APIs

- **A model that is not a signal.** The game lives in an `Rc<RefCell<Game>>`
  that every handler mutates, then writes `view.set(game.view())`. That is
  the right shape (the timer needs to change it without redrawing on every
  frame), but it is hand-made glue: a `Signal::update` that returns whether
  the closure changed anything, or a `store(model, view_fn)` that derives a
  view and notifies only when it changes, would remove it.
- **`every` exists only while building.** Starting the tick when Space starts
  the game is not possible from a handler, so it ticks on the start screen too
  (and the model ignores it).
- **The board as markup.** Two hundred `[color(n) on color(234)]██[/]` runs
  are formatted and parsed again whenever the view changes; it works, and a
  20x20 board is cheap, but a bigger grid (a map, Life) would want cells.
- **A box without a title** is `.borders(Sides::ALL)` (a panel without a
  title, by the guide's rule for non-panels), then `.border(Border::Plain)`
  and `.border_style(..)`: three builders for ratatui's `Block::bordered()`.
- **Centring a fixed-size stage** takes a `column` of a `row` with
  `.justify(Justify::Center)`, or the 3x3 ring of `fill`s once a backdrop is
  wanted (the opaque-layer gap). `layers`' `.place(..)` sizes the layer to
  its content's measure rather than to a size given.
- **The test clock is good.** `Driver::advance(FRAME * 47)` stepping to each
  due tick made gravity, the clear pause and the flash testable frame by
  frame with no seams beyond a seed and a prepared `Game`.

### Components that would help

| Component | What it would replace or enable | API sketch | Where | Value |
|---|---|---|---|---|
| A cell grid | The playfield as markup; any game board, a map, a Life or Minesweeper grid, a heat map: a grid of styled cells, `cell_width` columns each, redrawing only changed cells, with clicks reported as cell coordinates | `cell_grid(cols, rows, move \|x, y\| Cell::new("██", style)).cell_width(2)`, `.on_cell_click(\|x, y, cx\| ..)` | core (generic) | high |
| A game loop | The `Rc<RefCell>` + `every` + frame counter: a fixed-step update with catch-up and a cap, paused and resumed, with the step's time | `game_loop(step, \|dt, cx\| ..).max_catch_up(5)` returning a handle (`pause`, `resume`, `set_step`) | core (generic: a timer with these options) | high |
| Held-key state | DAS/ARR, soft drop while held, diagonal moves in other games | `held_keys() -> Memo<HashSet<Key>>` (from presses and releases, with the kitty protocol) and `on_key_repeat` | core | medium |
| High-score table | Not in the original; most arcade clones keep a top ten with names (`tetris-io` and `tetris-tui` keep scores in SQLite) | `persisted(..)` plus a `table`; a plugin for names and ranks | plugin (`games`), on the core's `persisted` | low |
| Block-letter text | The title, PAUSED and GAME OVER are the original's literal `██` strings; `big_text` (FIGlet, `art`) and `digits` (three rows) draw other letters | `big_text(..).font(Font::Blocks)`: a five-row block font with no FIGlet file, in every build, as `digits` is | core (generic, as `digits`) | low |
| Sound effects | See the gap: short sounds fired from handlers, as rata-tetris meant to, and as ratatui games do with rodio | `audio::sfx(include_bytes!("drop.wav")).play()` | plugin (`audio`) | medium |
| Seeded random numbers | The example's SplitMix64 and bag shuffle; any game or demo that tests deterministically | none in core: a game uses `rand`/`fastrand`; listed for the template, not the framework | none, plugin or app | low |

Comparable libraries: ratatui and crossterm games (`tetris-io`, `tetris-tui`, rata-tetris)
all run their own loop with `crossterm::event::poll` and a fixed tick, handle
DAS/ARR themselves, and use rodio for sound; Bubble Tea's `tea.Tick` and
`tea.Every` return a command per tick (re-armed with a new duration at will),
which is the changeable timer above; Textual's `set_interval` returns a
`Timer` with `pause()`, `resume()` and `reset()`, and its `Key` events carry
no release, the same limit as a terminal without the kitty protocol.
