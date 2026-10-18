# ratatui's classic demo rebuilt on intuiTUIve (examples/demo.rs): the
# data moving on the Graphs tab, a task picked, the chart hidden and shown,
# the World and Colours tabs, then the same frozen for the screenshots, so
# they are the same on every run.
Set Title "demo · ratatui's classic demo on intuiTUIve"
Set Size 100x32
Set TypingDelay 40ms
Hide
Exec "mkdir -p bin && ln -sf \"$(dirname \"$(command -v rich)\")/examples/demo\" bin/demo"
Type "export PATH=\"$PWD/bin:$PATH\"; clear"
Enter
Wait /❯\s*$/
Show
Type "demo"
Enter
Wait /Sparkline:/
Sleep 3s
Down
Down
Sleep 1s
Type "t"
Sleep 1500ms
Type "t"
Sleep 1500ms
Right
Wait /Servers/
Sleep 2s
Right
Wait /bright_white/
Sleep 1500ms
Type "q"
Wait /❯\s*$/
Type "demo --frozen"
Enter
Wait /12\.00%/
Down
Down
Sleep 300ms
Screenshot graphs
Type "t"
Sleep 300ms
Screenshot no-chart
Type "t"
Right
Wait /Servers/
Sleep 300ms
Screenshot world
Right
Wait /bright_white/
Sleep 300ms
Screenshot colours
Type "?"
Wait /show or hide the chart/
Sleep 300ms
Screenshot keys
Escape
Type "q"
Wait /❯\s*$/
