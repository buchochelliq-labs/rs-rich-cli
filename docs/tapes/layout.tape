# ratatui's constraint explorer rebuilt on intuiTUIve (examples/layout.rs):
# blocks edited and the gap widened in every justify mode, then each kind
# of size, the flex cases and what ratatui's layout lacks. Nothing in it
# runs on a clock, so the screenshots are the same on every run.
Set Title "layout · ratatui's constraint explorer on intuiTUIve"
Set Size 100x30
Set TypingDelay 40ms
Hide
Exec "mkdir -p bin && ln -sf \"$(dirname \"$(command -v rich)\")/examples/layout\" bin/layout"
Type "export PATH=\"$PWD/bin:$PATH\"; clear"
Enter
Wait /❯\s*$/
Show
Type "layout"
Enter
Wait /SpaceEvenly/
Sleep 1500ms
Screenshot explorer
Type "++"
Sleep 500ms
Right
Up
Up
Sleep 500ms
Screenshot explorer-gap
Right
Type "4"
Sleep 800ms
Type "a"
Sleep 800ms
Screenshot explorer-flex
Tab
Wait /Fixed │ Percent/
Sleep 500ms
Right
Right
Wait /Four quarters/
Sleep 300ms
Screenshot sizes-ratio
Right
Right
Right
Wait /At most 20/
Sleep 300ms
Screenshot sizes-max
Tab
Wait /Start, gap 0/
Sleep 300ms
Screenshot flex
Down
Down
Down
Down
Down
Down
Down
Down
Sleep 300ms
Tab
Wait /align_items/
Sleep 300ms
Screenshot beyond
Type "q"
Wait /❯\s*$/
