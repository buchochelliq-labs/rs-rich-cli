# raycast: a first-person ray caster on intuiTUIve

## Round 1: rs-rich raycaster

The demo is [`examples/raycast/`](../../../crates/rich-intuituive/examples/raycast/main.rs),
an original first-person shooter in the style of the early-90s ray casters
(no id Software assets or names: the levels, textures, sprites and face
are generated in code). The algorithm follows Lode Vandevenne's
[raycasting tutorial](https://lodev.org/cgtutor/raycasting.html) (DDA,
camera plane, floor casting, sprite projection against a z-buffer), with
Wolfenstein-style doors: a thin plane across the middle of the cell that
slides into the wall, and a jamb texture on the walls beside it.

What was built (2,924 lines: `game.rs` 1,005, the model with no framework
in it; `art.rs` 677, procedural textures and sprites; `render.rs` 271,
the ray caster into an RGB framebuffer; `view.rs` 295, the framebuffer on
screen; `main.rs` 676, the app; tests 593 lines in `tests/raycast.rs`, 26
tests):

- **The 3D view**: textured walls (brick, stone, wood, tech panels, a
  door with a hazard stripe, its jamb, an EXIT switch) with distance fog
  and darker east-west faces; floor and ceiling cast with textures (or
  `--flat`: flat colours fading into the dark); sprites billboarded and
  depth-sorted against the z-buffer (barrels, columns, hanging lamps,
  ammunition, medikits, armour, and a hooded sentry with standing, two
  walking, firing, hurt and three dying frames); the pistol bobbing as the
  player walks, its muzzle flash lighting the room; red when hurt, gold on
  a pickup.
- **Gameplay**: two hand-made levels as strings (`E1 Hangar`, `E2 Depot`);
  doors opened with Space that close by themselves unless something stands
  in them; hitscan shooting along the view's centre; pickups; enemies that
  wake on sight, react after a moment, walk at the player and fire with a
  hit chance by distance; armour soaking a third; an exit switch; death
  and a tally (kills, items, time); a seeded generator so a test sees the
  same fight.
- **Everything round the view is intuiTUIve nodes**: the status bar
  (`digits` for ammunition, health and armour in `panel`s, the face as a
  `renderable` of rs-rich-art's `ImageArt`, a `text` with the level, the
  time, kills and items, and the frame rate from the core's `rate`); the
  map (`Tab`, a `text` of markup in a `panel` placed over the view with
  `layers` and `.place`); the pickup message; the title (`big_text`, over
  the first level turning slowly); the level-done, dead and cleared boxes
  (`switch` on the state); the pause menu (`cx.modal`); help
  (`App::help_key("?")` from the `.bind_in` descriptions) and the palette.
- **Keys**: W/↑ S/↓ walk, A/← D/→ turn, Q/E (Alt+←/→, `,` `.`) strafe,
  capitals run, F or Ctrl+F fires, Space uses, Enter starts and goes on,
  Tab the map, Esc pauses, `?` keys, Ctrl+Q quits; 1 and 2 on the title
  pick a level.
- **Display modes** (`--mode`): `blocks` (half blocks, two pixels a cell,
  the default), `quadrants`, `braille`, `kitty` and `iterm` (real pixels,
  4×8 a cell), `auto` (rs-rich-ext's `GraphicsEnvironment::detect`: Kitty
  or iTerm2 when found, else half blocks; Sixel is detected but cannot be
  drawn, see below). The Kitty and iTerm2 modes are tested headless (the
  escapes are sent each frame, cells drawn under overlays) but have not
  been seen in a real Kitty, WezTerm or iTerm2 yet.
- **The clock**: one `every(TIC)` at 35 Hz, Doom's tic. In a test each
  tick is one tic, so `Driver::advance(TIC * n)` steps the game exactly;
  run for real (`Options::realtime`), each tick runs the tics owed by the
  wall clock, five at most, so a slow frame drops frames, not game time.
  `--uncapped` draws on every turn of the loop to measure the cost;
  `--bench SECS` walks round by itself and prints the frame rate.
- **The tape**: [`docs/tapes/raycast.tape`](../../tapes/raycast.tape)
  (title, start, map, keys and pause menu as checked screenshots; then the
  walk to the door, the door opening, the sentry coming through it and
  the fight, in the cast and the GIF,
  `docs/media/tapes/raycast/raycast.gif`). `rich record --check` passes
  twice in a row.

**Bespoke drawing, and why**: the 3D view itself. It is a framebuffer of
RGB bytes rendered each tic by the example (`render.rs`), as any ray
caster's is; nothing in a widget library draws a 3D scene. Getting it on
screen is not bespoke in the cell modes (rs-rich-art's `ImageArt` rendered
on a `Widget`'s canvas), but is in the pixel modes: the Kitty and iTerm2
escapes and the `PlacementSource` are written in `view.rs`, because the
`art` feature exposes neither (see the gaps). The view is a `Widget` of
its own for one more reason: key releases reach widgets only. The
Braille mode dithers the frame first (a 4×4 Bayer pass, 15 lines),
because rs-rich-art's Braille has no colour and a fixed threshold.

**Frame rate** (`--bench 5`, the auto-walk, a real PTY, the reader only
draining it; frames a second; capped is one frame a tic, at most 35):

| build | size | half blocks | half blocks, flat floor | quadrants | Braille | Kitty (PNG) |
|---|---|---|---|---|---|---|
| debug | 80×24 (80×38 px) | 26.8 capped, 26.2 uncapped | 31.3 | 21.9 | 155.7 | 51.7 |
| debug | 160×50 (160×90 px) | 7.2 capped, 9.8 uncapped | 12.3 | 7.3 | 43.8 | 11.8 |
| release | 80×24 | 34.7 capped, 66.9 uncapped | 68.7 | 74.6 | 511 | 210 |
| release | 160×50 | 19.4 capped, 22.1 uncapped | 28.9 | 21.5 | 214 | 72.8 (640×360 px) |

(The columns after the first are uncapped.) The ray caster is not the
cost: 0.08 ms a frame at 80×38 and 0.39 ms at 160×90 in release (0.05
and 0.2 ms flat). At 160×50 in half blocks a frame costs about 41 ms in
release, of which `ImageArt` is 7 ms and intuiTUIve's canvas, screen diff
and painter the rest (callgrind: 35% `Screen::write_lines`, 31%
`StyleTable::intern`, mostly SipHash over `Style`s, 23% `Painter::paint`,
14% `Screen::underlay`, then `malloc`/`free` and `Style::clone`). Braille
and the pixel modes are fast because they write few or no styled cells.
The floor cast stays on by default: it costs a fraction of a millisecond.

**Left out**: sound (no output in the audio plugin yet); mouse-look (no
pointer lock or relative motion); a gamepad; more weapons, secrets,
keys and locked doors; Sixel (see the gaps); a status bar drawn as one
pixel picture, as the originals' is.

### New gaps and bugs

| Gap or bug | Needed for | API sketch or repro | Where |
|---|---|---|---|
| **Bug: one input event per loop turn** | Keys typed while frames are slow: `run_loop` reads one event, then updates and renders, so queued keys are handled one a frame. At 100×30 in debug (about 100 ms frames while moving) six `e` presses written 60 ms apart were handled 100 ms apart (traced), holding a latched strafe twice as long | Drain what is waiting before drawing: after one `read(wait)`, `read(Some(ZERO))` until empty (capped, say 64 events), then update and render once | core (`App::run_loop`) |
| No app clock in timer handlers | A fixed timestep that catches up: an `every` handler is not told the time, nor how many ticks it missed (it ticks once after falling behind), so the example measures the wall clock with `Instant` and runs owed tics itself, which a test's `Driver` clock cannot step: tests run with catch-up off | `cx.now()` on `Ctx` (the app's clock, as `DrawCx::now`), or `fixed_rate(step, \|cx, ticks\| ..)` called with the ticks due | core |
| Key releases for bindings, and for plain keys (again, after `blocks`) | Walking while W is held: letters, Space and Enter never send a release (kitty flag 8, report all keys as escapes, is not asked for), and `KeyUp` reaches only a `Widget`'s `event`. The example latches a press for 8 tics and each repeat for 5 more, so a tap walks 0.7 cells and holding stutters through the terminal's repeat delay; once it has seen a key let go (arrows under the kitty protocol) it holds that key by press and release | `node.on_key_up(keys, f)`; `App::key_releases(All)` pushing kitty flags 1+2+8; `held_keys() -> Memo<HashSet<Key>>` | core |
| Which key fired a binding | Telling `w` from `↑` in one `.bind("w up", ..)`: the example binds each key on its own (one described, the rest `.on_key`) to know which may send releases | `cx.key()` in a handler, or `.bind_with_key(keys, desc, \|cx, key\| ..)` | core |
| Modifier keys alone | Fire on Ctrl, run on Shift, as the originals: a terminal sends no key for a modifier alone; the kitty protocol does with flag 8 | with all-keys reporting: `Key::Modifier(Ctrl)` presses and releases | core |
| An RGB framebuffer node | The view: in cells through `ImageArt` on a widget's canvas (a copy of the buffer and a resize each frame), in pixels through a hand-written `PlacementSource`; the `image` node takes only a path | `pixels(\|\| frame.get()).mode(Auto \| Blocks \| Quadrants \| Braille \| Kitty \| Sixel \| Iterm)` picking by capability, its size in pixels told to the app, cells as the fallback under an overlay | plugin (`art`), on the core's generic graphics placement (`App::graphics`) |
| `art` does not turn on rs-rich-art's protocol encoders | Kitty, Sixel and iTerm2 images: `rich_art::kitty`, `sixel` and `iterm` are behind rs-rich-art features that intuiTUIve's `art` leaves off (and an example cannot turn them on), so the example writes the Kitty and iTerm2 escapes itself and has no Sixel | `art = ["dep:rich-art", "rich-art/image", "rich-art/kitty", "rich-art/sixel", "rich-art/iterm"]`, or an `art-graphics` feature | plugin (`art`) |
| A full screen of truecolour cells is slow to paint | 160×50 half blocks: about 33 ms a frame in release in the canvas, diff and painter (22 fps uncapped) | Intern styles with a fast hasher or by colour pair, keep cells as `(fg, bg, attrs)` without cloning `Style`, a `Canvas::put_rgb(x, y, glyph, fg, bg)` that skips `Style` | core (rs-rich-ext `frame::StyleTable`, `Screen`) |
| rs-rich-art's Braille has no colour and a fixed threshold | A Braille fallback for a scene: a dot where luma ≥ 128, no style at all, so a dim room is blank. The example dithers first | `BrailleArt::color(true)` (each cell's mean colour as its foreground), `.threshold(t)`, `.dither(Dither::Bayer)` | rs-rich-art |
| Sound out (again, after `blocks`) | Shots, doors, pickups: the `audio` plugin captures and plays onto a signal; nothing reaches a speaker (another agent is adding `audio::output`) | `audio::output()?.play(sfx)`, fire-and-forget, mixed | plugin (`audio`) |
| Pointer motion for mouse-look | Turning with the mouse: `report_movement` gives cell positions only, with no relative motion or pointer lock, so the pointer leaves the window | relative `dx`/`dy` from SGR-pixel mouse mode (1016), and an app-level "capture the pointer" | core |
| Gamepad | Playing with a controller | `gamepad::events() -> Signal<..>` (gilrs, not in the lockfile) | plugin (`gamepad`); no core shape: a controller is a device |

### Friction with the existing APIs

- **A model that is not a signal** (as `blocks` found): the game lives in
  an `Rc<RefCell<Game>>`, every handler mutates it, and the timer renders
  and writes a `Signal<Rc<Frame>>` with `replace` (a frame never equals
  the last, so `set`'s comparison would only cost) and a `Signal<Hud>`
  with `set`, which redraws the status bar only when what it shows changed.
- **A widget only for releases.** The view had to be a `Widget` anyway,
  but a game whose view were a stock node would need one just to hear
  `KeyUp`.
- **Overlays over pixels.** A Kitty or iTerm2 picture is drawn after the
  cells, so the map, the pause menu and the end boxes would be painted
  over by it. The example draws the view in half blocks while anything
  covers it (and gives Kitty's image a z-index below cells with a
  background); a `PlacementSource` told the rectangles of the layers over
  its placement could clip instead.
- **`App::graphics` is set outside `App::new`.** The source is built
  before the app and shares state with the view through an
  `Arc<Mutex<..>>`; the view finds its own rectangle through a
  `RectHandle` made before it.
- **`layers` are opaque, and `.place` sizes a layer to its content**: an
  empty message drew a padded blank cell over the view until the padding
  moved into the text.
- **Timers per screen.** The game's `every` keeps running under the pause
  modal (the screen below keeps drawing), so pausing is the model's flag.
- **The test clock is excellent.** With catch-up off, `Driver::advance(TIC
  * n)` steps the game exactly, so tests assert on the framebuffer (the
  centre column's wall and distance, a pixel's colour), on the HUD text
  and accessibility names, and that the same keys give the same frame
  bytes.

### Components that would help

| Component | What it would replace or enable | API sketch | Where | Value |
|---|---|---|---|---|
| A pixel surface | The view widget and `view.rs`: a node that takes an RGB(A) buffer each frame and shows it in the best way the terminal has, cells as the fallback; emulators, plots, image viewers, video, games | `pixels(\|\| frame.get()).mode(Auto)`, `.on_resize(\|w_px, h_px, cx\| ..)` | plugin (`art`) on the core's graphics placement | high |
| A game loop | The `every` + wall-clock catch-up + `Rc<RefCell>`: fixed steps with catch-up on the app's clock, an interpolation factor for drawing, pause | `game_loop(TIC, \|cx, ticks\| ..).max_catch_up(5)` returning a handle (`pause`, `resume`) | core (generic: a timer with these options) | high |
| Held-key state | The latching in `game.rs` and the release bookkeeping in `main.rs` | `held_keys()`, `is_held("w")`, from presses and releases with the kitty protocol, latching on repeats without | core | high |
| A fast cell path | Full-screen truecolour pictures at 60 fps | `Canvas::put_rgb`, `Screen` cells as colour pairs, a fast style hash | core | high |
| Sound effects | Shots and doors | `audio::sfx(bytes).play()` | plugin (`audio`) | medium |
| A pixel font for HUD digits | The originals draw their status bar into the picture; `digits` is box glyphs | `digits(..).font(Font::Pixel)` in every build | core (as `digits`) | low |

Comparable libraries: ratatui games draw a framebuffer as half blocks
straight into the `Buffer` (or through `Canvas`), and use `ratatui-image`
for Kitty, Sixel and iTerm2 (a `StatefulImage` widget whose `Picker`
queries the terminal and picks the protocol, half blocks as the fallback):
that crate is the pixel surface above. Textual's `textual-image` does the
same for Textual; Bubble Tea has no pixel widget. None of them reports key
releases without the kitty protocol; terminal games everywhere latch
presses as this one does.

## Round 2: rebuilt on round 6

The example went from 2,924 lines to 2,891 (`main.rs` 676 → 662, `view.rs`
295 → 276; `game.rs`, `art.rs` and `render.rs` unchanged), and
`tests/raycast.rs` from 593 lines to 652 (26 tests to 28). Two bugs found
while recording it are fixed in the example, each with a test that fails
on round 1's code.

### Bugs fixed (example bugs, not library ones)

- **Kitty mode showed nothing.** The image is placed at a z below -2^30,
  which Kitty draws under every cell that has a background colour, and
  the root's `.style("on #101018")` gave every cell of the view one. The
  z is right (the level's name over the view, in a box with a background,
  still covers the image); the background moved from the root to the
  status bar, so the view's cells keep the terminal's own. The map's
  panel takes `on #101018` itself. Test:
  `the_kitty_view_keeps_the_terminals_background` (no background in the
  view's rows but the level-name box; the status bar keeps it).
- **An arrow counted as held only after the app had seen a release for
  it**, so until then each press was latched for 8 tics. A movement key
  is now held from its first press wherever its release will be reported
  (`cx.key()` and `cx.releases_reported().reported(&key)`), until its
  `on_key_up`; where none will come (a legacy terminal, or a letter
  without every key reported) it is latched as before, the terminal's
  repeats extending it. The app asks for every key (`App::all_keys`), so
  under the kitty protocol W/A/S/D are held too, and Shift let go stops
  running (`on_key_up("shift")`). Tests:
  `a_key_whose_release_is_reported_is_held_from_its_first_press` (replaces
  `a_key_seen_let_go_is_held_until_it_is_released`, whose behaviour was
  the bug) and `with_every_key_reported_letters_are_held_and_shift_runs`.

### Replaced by components

| Before (hand-built) | Now | Component |
|---|---|---|
| `every(TIC)` plus `Instant` bookkeeping to run the tics owed (five at most), off under tests because a timer handler had no clock | A fixed step of a tic, five at most after a stall, exact under `Driver::advance`; the `realtime` option is gone | `fixed_step`, `Timer::max_catch_up` |
| Releases heard by the view widget's `event` (`WidgetEvent::KeyUp`), a `Releasing` set of keys seen let go | `on_key_up` per key on the root; which releases come from the terminal | `Node::on_key_up`, `Ctx::key`, `Ctx::releases_reported`, `App::all_keys` |
| The view drawing rs-rich-art `ImageArt` on its canvas (a copy into an `RgbImage`, half blocks, quadrants or Braille), with a dithering pass for Braille | A `pixels_fit` node, its `PixelMode` by the view's mode, filled from the frame | `pixels::pixels_fit`, `Node::pixel_mode` (cells written with `put_rgb`) |
| A `RectHandle` made before the view for its own rectangle | `cx.rect()` in the pixel view's `draw` | `DrawCx::rect` |
| One view widget switching between cells and an image by `covered`, clearing the picture by hand | `keyed` on `covered`: the cells node or the pixel view, whose `on_unmount` takes the picture down | `keyed`, `Node::on_unmount` |
| The face as a `renderable` of `ImageArt` | `pixels(10, 6, ..)` | `pixels::pixels` |

The test suite runs in about half the time (2 s instead of 4.5 s in a
debug build), the cells now going through the pixel path.

**Changed look:** Braille is now drawn in two colours a cell (the
`pixels` node's) instead of a dithered black and white; shapes read as
before and the scene keeps its colours.

### Still hand-built, and why

- **The pixel view and its `PlacementSource`** (`view::Pixels`, the PNG
  encoding and the Kitty and iTerm2 escapes): the `pixels` node draws cells
  only; the `art` plugin's `image_bytes_of` draws Kitty (by placeholders),
  Sixel and iTerm2 but takes encoded image bytes and is not built for 35
  new pictures a second.
- **The latch** in `game.rs` (`Held`, `FIRST_LATCH`, `REPEAT_LATCH`) for
  keys whose release will not come. `held_keys()` holds only keys whose
  release is known (or made up by `synthesize_releases`, whose `hold` must
  outlast the terminal's repeat delay, about 500 ms, far too long for a tap
  in a game), so the model keeps both kinds in one place.
- **Pausing** is still the model's flag: the modal's screen keeps the
  game's timer, so `Timer::pause` would only save the stepping it does
  not do.
- **The bench**'s frame count and first-frame `Instant` (it counts frames
  rendered from the first one, which `StatsHandle` does not).

### New gaps

| Gap | Needed for | API sketch | Where |
|---|---|---|---|
| Held keys that fall back on a latch | A key held where its release is known, and held for a few ticks a press (extended by each repeat) where it is not: the model's `Held` and the press/release bindings | `held_keys_with(Unreported::Latch { first: ms(230), repeat: ms(140) })`, so `cx.is_held("w")` is right on every terminal | core (generic) |
| A pixel buffer that shows real pixels | The view's PNG encoding, `PlacementSource`, Kitty z-order and iTerm2 escapes, and the switch to cells under an overlay | `pixels_fit(..).graphics(Graphics::Auto)`: Kitty, Sixel or iTerm2 where the terminal has them, cells where something covers it | plugin (`art`), on the core's graphics placement |
| Copying a frame into a `PixelBuf` | The loop from RGB bytes into RGBA, twice (the view and the face) | `PixelBuf::copy_rgb(width, height, &bytes)` (scaled or clipped) | core (generic) |
| Kitty's z-order and backgrounds | The bug above is easy to make: any style on an ancestor puts a background under the image and hides it, with no warning | the graphics placement told which cells under it have a background (debug-build warning), or an `App::graphics` option that places at a z above backgrounds and clips at the layers over it | core (`App::graphics`) |

### Bugs found in the library

None this round. (`App::all_keys` and the release bindings worked as the
guide says; the driver's `set_releases` made both fixes testable.)

### Components that would help

| Component | What it would replace or enable | API sketch | Where | Value |
|---|---|---|---|---|
| Real pixels on the `pixels` node | `view.rs`'s encoder and placement source (most of what is left of it) | see the gap | plugin (`art`) | high |
| Latching held keys | `Held` in `game.rs` and the bindings round it | see the gap | core | high |
| Sound effects (round 1's) | Shots, doors, pickups | `audio::sfx(bytes).play()` | plugin (`audio`) | medium |
| Mouse-look (round 1's) | Turning with the mouse | relative motion (SGR pixels), pointer capture | core | low |
