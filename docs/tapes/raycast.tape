# rs-rich raycaster (crates/rich-intuituive/examples/raycast): a
# first-person ray caster on intuiTUIve, drawn in half blocks, two pixels a
# cell. The title over the first level turning slowly; the start, the map
# (Tab), the keys (?) and the pause menu (Esc), each taken before the
# player moves, so their text is the same on every run (the time and the
# frame rate are masked). Then a walk east to the door, Space to open it,
# and shots at the sentry behind it as it comes (in the cast and the GIF
# only: the fight runs on the wall clock, so it varies from run to run).
Set Title "rs-rich raycaster · a ray caster on intuiTUIve"
Set Size 100x30
Set TypingDelay 60ms
Mask /\s+\d+ fps/ " NN fps"
Mask /\d+:\d\d/ "N:NN"
Hide
Exec "mkdir -p bin && ln -sf \"$(dirname \"$(command -v rich)\")/examples/raycast\" bin/raycast"
Type "export PATH=\"$PWD/bin:$PATH\"; clear"
Enter
Wait /❯\s*$/
Show
Type "raycast --mode blocks"
Enter
Wait /Enter start/
Sleep 2s
Screenshot title
Enter
Wait /Kills 0\/4/
Sleep 4s
Screenshot start
Tab
Wait /Map/
Sleep 500ms
Screenshot map
Tab
Type "?"
Wait /Walk forward/
Sleep 500ms
Screenshot keys
Escape
Sleep 300ms
Escape
Wait /Paused/
Sleep 300ms
Screenshot paused
Escape
Sleep 500ms
Type "wwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwww"
Sleep 400ms
Space
Sleep 1500ms
Type "f"
Sleep 600ms
Type "f"
Sleep 600ms
Type "f"
Sleep 600ms
Type "f"
Sleep 1500ms
Escape
Wait /Paused/
Sleep 500ms
Type "q"
Wait /❯\s*$/
