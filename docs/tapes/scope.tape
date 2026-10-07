# scope-tui rebuilt on intuiTUIve (examples/scope.rs): the test signal
# moving in the oscilloscope and the vectorscope, then held still and
# paused for the screenshots, so they are the same on every run, in each
# of the three scopes.
Set Title "scope · scope-tui on intuiTUIve"
Set Size 110x26
Set TypingDelay 40ms
Hide
Exec "mkdir -p bin && ln -sf \"$(dirname \"$(command -v rich)\")/examples/scope\" bin/scope"
Type "export PATH=\"$PWD/bin:$PATH\"; clear"
Enter
Wait /❯\s*$/
Show
Type "scope"
Enter
Wait /oscillo::scope-tui/
Sleep 1500ms
Tab
Wait /vector::scope-tui/
Sleep 2s
Tab
Wait /spectro::scope-tui/
Sleep 1s
Type "q"
Wait /❯\s*$/
Type "scope --still"
Enter
Wait /oscillo::scope-tui/
Sleep 500ms
Space
# Two ticks of the counter after the pause, so it reads 0.
Sleep 2200ms
Wait / 0fps/
Sleep 300ms
Screenshot oscilloscope
Tab
Wait /vector::scope-tui/
Sleep 300ms
Screenshot vectorscope
Tab
Wait /spectro::scope-tui/
Sleep 300ms
Screenshot spectroscope
Type "?"
Wait /Keys/
Sleep 300ms
Screenshot keys
Escape
Type "q"
Wait /❯\s*$/
