# Four of ratatui's smaller apps rebuilt on intuiTUIve (batch D3): the
# colour explorer's tabs and its turning swatch (examples/colors.rs); the
# canvas, with the ball moving, a stroke drawn and the markers changed
# (examples/canvas.rs); a form filled in, wrong then right
# (examples/form.rs); and the year calendar going round its looks
# (examples/calendar.rs). Each then frozen or still for the screenshots.
Set Title "apps · ratatui's smaller apps on intuiTUIve"
Set Size 100x32
Set TypingDelay 40ms
Hide
Exec "mkdir -p bin && for app in colors canvas form calendar; do ln -sf \"$(dirname \"$(command -v rich)\")/examples/$app\" bin/$app; done"
Type "export PATH=\"$PWD/bin:$PATH\"; clear"
Enter
Wait /❯\s*$/
Show

# colors: the named colours, the cube, then the swatch turning.
Type "colors"
Enter
Wait /bright_magenta/
Sleep 1500ms
Right
Wait /the greys/
Sleep 1500ms
Right
Wait /frames a second/
Sleep 3s
Right
Wait /Crossed out/
Sleep 1500ms
Right
Wait /fuchsia/
Sleep 1500ms
Type "q"
Wait /❯\s*$/
Type "colors --frozen"
Enter
Wait /bright_magenta/
Sleep 300ms
Screenshot colors-named
Type "3"
Wait /stopped/
Sleep 300ms
Screenshot colors-rgb
Type "q"
Wait /❯\s*$/

# canvas: the ball, you moving east, then every marker.
Type "canvas"
Enter
Wait /You are here/
Sleep 2s
Type "l"
Type "l"
Type "k"
Sleep 1s
Enter
Sleep 800ms
Enter
Sleep 800ms
Enter
Sleep 800ms
Enter
Sleep 800ms
Type "q"
Wait /❯\s*$/
Type "canvas --frozen"
Enter
Wait /You are here/
Sleep 300ms
Screenshot canvas
Type "q"
Wait /❯\s*$/

# form: a digit in a name is wrong at once; fixed, the form is sent and
# printed.
Type "form"
Enter
Wait /First name/
Type "Ada1"
Sleep 1s
Screenshot form-error
Backspace
Tab
Type "Lovelace"
Tab
Type "36"
Sleep 1s
Enter
Wait /36 years old/
Sleep 1500ms

# calendar: a few days on, a month on, then round the looks.
Type "calendar --today 2026-10-17"
Enter
Wait /December 2026/
Sleep 1500ms
Type "l"
Type "l"
Type "j"
Sleep 1s
Type "s"
Sleep 1s
Type "s"
Sleep 1s
Screenshot calendar
Type "s"
Sleep 1s
Type "s"
Sleep 1s
Type "s"
Sleep 1s
Type "q"
Wait /❯\s*$/
