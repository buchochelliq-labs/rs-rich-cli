# rata-tetris rebuilt on intuiTUIve (examples/blocks.rs): the title, a
# game on a seeded bag (`--seed 7`, so the pieces come in the same order
# on every run) with a few pieces moved, rotated, slammed down and one
# held, the pause (which hides the board), and the game over after every
# piece is slammed down where it falls. No top score is kept
# (`--no-score-file`). Each still is taken within a fifth of a second of
# the last key, well before gravity at level 0 (48 frames, 0.8 s) moves the
# falling piece.
Set Title "blocks · rata-tetris on intuiTUIve"
Set Size 80x30
Set TypingDelay 40ms
Output gif png svg
Hide
Exec "mkdir -p bin && ln -sf \"$(dirname \"$(command -v rich)\")/examples/blocks\" bin/blocks"
Type "export PATH=\"$PWD/bin:$PATH\"; clear"
Enter
Wait /❯\s*$/
Show
Type "blocks --seed 7 --no-score-file"
Enter
Wait /start: space/
Sleep 1500ms
Screenshot title
Space
Wait /STATISTICS/
Sleep 500ms
Up
Sleep 400ms
Left 3
Sleep 300ms
Up
Sleep 400ms
Right 3
Sleep 300ms
Up
Sleep 400ms
PageDown
Sleep 300ms
Left
Sleep 300ms
Up
Sleep 400ms
Type "c"
Sleep 150ms
Screenshot playing
Space
Sleep 500ms
Screenshot paused
Space
Sleep 300ms
Up 30
Wait /You scored/
Sleep 1s
Screenshot game-over
Type "q"
Wait /❯\s*$/
