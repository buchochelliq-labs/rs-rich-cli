# rs-rich raycaster (crates/rich-intuituive/examples/raycast): a
# first-person ray caster on intuiTUIve, drawn in half blocks, two pixels a
# cell. The title over the first level turning slowly; the start, the map
# (Tab), the keys (?) and the pause menu (Esc), each taken before the
# player moves, so their text is the same on every run (the time and the
# frame rate are masked). Then a walk-through played with held keys: east
# to the door, Space to open it, the sentry behind it shot as it comes,
# on through the hall to the second door, the guard behind that one shot,
# and the ammo beyond it picked up. The fight runs on the game's clock, so
# the video varies a little from run to run; the stills do not.
#
# Recorded at 35 frames a second, every frame the game draws, with the
# kitty keyboard protocol reported, so the arrows' releases reach the game
# and a held arrow walks as long as it is held.
Set Title "rs-rich raycaster · a ray caster on intuiTUIve"
Set Size 100x30
Set TypingDelay 60ms
Set Framerate 35
Set FontSize 10
Set KittyKeyboard on
Output gif apng png svg
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
Sleep 3s
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
# The walk-through, facing east all the way: both doors and both guards
# are on the row the player starts on, so each guard comes through its
# doorway into the line of fire, and a closed door stops the walk in front
# of it. The trigger is held until the first guard is down, and for five
# and a half seconds at the second (a Wait there would fail the run when
# the guard hangs back), by when the door has begun to close again, so
# Space opens it once more.
Hold Up 2s
Space
KeyDown f
Wait /Kills 1\/4/ 20s
KeyUp f
Hold Up 3s
Space
KeyDown f
Sleep 5500ms
KeyUp f
Space
Sleep 1500ms
Hold Up 1500ms
Hold Alt+Left 340ms
Hold Up 1500ms
Sleep 1500ms
Escape
Wait /Paused/
Sleep 500ms
Type "q"
Wait /❯\s*$/
