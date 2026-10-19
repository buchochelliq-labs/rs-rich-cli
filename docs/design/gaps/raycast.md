# raycast: a first-person ray caster on intuiTUIve

## Round 1: rs-rich raycaster (WIP)

Status: work in progress. Done: the game (`examples/raycast/`: two levels,
textured walls, cast floor and ceiling, sprites against a z-buffer, doors,
hitscan, pickups, enemies, exit), the view in half blocks / quadrants /
Braille through rs-rich-art's `ImageArt`, Kitty and iTerm2 pixels through
`App::graphics`, the HUD (`digits`, a face, fps via `rate`), map, pause
menu, title, help; 26 tests in `tests/raycast.rs`; the tape
`docs/tapes/raycast.tape`. Left: re-record the tape after the start moved
into the door's row, clippy/rustfmt, and the full gap tables below.

Gaps found so far (to be written up): one input event per loop turn
(queued keys spread over slow frames); no app clock in timer handlers
(`cx.now()`), so catch-up needs the wall clock; key releases only for
non-text keys and only to widgets; no RGB framebuffer node (cell modes go
through `ImageArt`, pixel modes need a hand-written `PlacementSource`);
painting a full screen of distinct truecolour cells costs ~33 ms at 160x50
in release (StyleTable::intern with SipHash, Style clones); rs-rich-art's
Braille has no colour and a fixed threshold; `art` does not enable
rs-rich-art's `kitty`/`sixel`/`iterm` encoders; no sound output.
