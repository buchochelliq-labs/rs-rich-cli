# rs-rich raycaster in real pixels: the same walk-through as raycast.tape,
# drawn with iTerm2's inline images (a PNG over the view's cells each
# frame, four by eight pixels a cell), which the recorder's emulator
# decodes and draws into each video frame. The stills' text shows the
# view's cells blank, as a terminal's text does under an image. Every
# frame is a new picture, so this one is recorded at 24 frames a second
# in smaller text, to keep the GIF under 10 MB (its APNG would be 14 MB). (The example's Kitty mode
# places its image under cells with a background colour, and the view's
# cells have one, so a Kitty terminal, and the emulator, show none of it.)
Set Title "rs-rich raycaster · iTerm2 images"
Set Size 100x30
Set TypingDelay 60ms
Set Framerate 24
Set FontSize 8
Set KittyKeyboard on
Output gif png
Mask /\s+\d+ fps/ " NN fps"
Mask /\d+:\d\d/ "N:NN"
Hide
Exec "mkdir -p bin && ln -sf \"$(dirname \"$(command -v rich)\")/examples/raycast\" bin/raycast"
Type "export PATH=\"$PWD/bin:$PATH\"; clear"
Enter
Wait /❯\s*$/
Show
Type "raycast --mode iterm"
Enter
Wait /Enter start/
Sleep 1s
Enter
Wait /Kills 0\/4/
Sleep 2s
Screenshot start
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
