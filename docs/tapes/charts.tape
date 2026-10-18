# ratatui's chart examples as a gallery on intuiTUIve (examples/charts.rs):
# the gauges and the moving chart running, then each tab frozen for the
# screenshots, so they are the same on every run.
Set Title "charts · ratatui's chart examples on intuiTUIve"
Set Size 100x30
Set TypingDelay 40ms
Hide
Exec "mkdir -p bin && ln -sf \"$(dirname \"$(command -v rich)\")/examples/charts\" bin/charts"
Type "export PATH=\"$PWD/bin:$PATH\"; clear"
Enter
Wait /❯\s*$/
Show
Type "charts"
Enter
Wait /Percentage/
Sleep 3s
Type "5"
Wait /A moving window/
Sleep 3s
Type "6"
Wait /You are here/
Sleep 1s
Type "m"
Sleep 800ms
Type "m"
Sleep 800ms
Type "q"
Wait /❯\s*$/
Type "charts --frozen"
Enter
Wait /42%/
Sleep 300ms
Screenshot gauge
Right
Wait /Left to right/
Sleep 300ms
Screenshot sparkline
Right
Wait /Weather/
Sleep 300ms
Screenshot bars
Right
Wait /Revenue by quarter/
Sleep 300ms
Screenshot groups
Right
Wait /A moving window/
Sleep 300ms
Screenshot chart
Right
Wait /You are here/
Sleep 300ms
Screenshot canvas
Type "m"
Type "m"
Wait /Marker: Block/
Sleep 300ms
Screenshot canvas-block
Type "q"
Wait /❯\s*$/
