# A bottom-style system monitor rebuilt on intuiTUIve (examples/top.rs).
# It reads this machine's /proc and /sys, so the dashboard differs on
# every run: the video shows it (the CPU, memory and network graphs, the
# temperatures, disks and processes, the sort menu after `s` in the
# process table, a widget expanded with `e`), and the stills are of the
# help (`?`), its first page and a later one. Two masks keep only the
# help's box in the stills' text grids, so `--check` compares the help
# and not the live data around it: the first cuts every line to the
# box's 80 columns, the second blanks the lines that are not the box's.
Set Title "top · bottom on intuiTUIve"
Set Size 100x30
Set TypingDelay 40ms
Output gif png svg
Mask /(?m)^.{10}(.{80}).*$/ "$1"
Mask /(?m)^[^│┌└].*$/ ""
Hide
Exec "mkdir -p bin && ln -sf \"$(dirname \"$(command -v rich)\")/examples/top\" bin/top"
Type "export PATH=\"$PWD/bin:$PATH\"; clear"
Enter
Wait /❯\s*$/
Show
Type "top"
Enter
Wait /Processes/
Sleep 3s
Type "?"
Wait /1 - General/
Sleep 800ms
Screenshot help
PageDown
Wait /3 - Process widget/
Sleep 800ms
Screenshot help-processes
Escape
Sleep 500ms
Ctrl+Down
Sleep 300ms
Ctrl+Down
Sleep 300ms
Ctrl+Right
Sleep 1s
Type "s"
Sleep 1500ms
Escape
Sleep 300ms
Type "e"
Sleep 2s
Escape
Sleep 1s
Type "q"
Wait /❯\s*$/
