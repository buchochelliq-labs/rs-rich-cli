# intuiTUIve's widgets after Textual's and other kits' (examples/kit.rs):
# the notice bar opened, the wizard's stepper and the accordion clicked,
# a toast with a button, and a tag typed in. Nothing in it runs
# on a clock, so the screenshots are the same on every run.
Set Title "kit · notices, steps, chips, toasts on intuiTUIve"
Set Size 80x24
Set TypingDelay 40ms
Hide
Exec "mkdir -p bin && ln -sf \"$(dirname \"$(command -v rich)\")/examples/kit\" bin/kit"
Type "export PATH=\"$PWD/bin:$PATH\"; clear"
Enter
Wait /❯\s*$/
Show
Type "kit"
Enter
Wait /Changed on disk/
Sleep 800ms
Screenshot kit
Enter
Wait /keep your edits/
Sleep 300ms
Screenshot notice
Click 25 4
Click 2 16
Sleep 400ms
Screenshot steps
Type "t"
Wait /Undo/
Sleep 300ms
Screenshot toast
Click 76 12
Type "docs,"
Sleep 400ms
Screenshot tags
Ctrl+Q
