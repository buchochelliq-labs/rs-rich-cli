# A taskwarrior-tui-style task list rebuilt on intuiTUIve
# (examples/tasks.rs), over a store written by
# docs/tapes/fixtures/tasks/make.py: every time in it is the moment the
# tape runs plus or minus whole hours, and the uuids are fixed, so the
# ages, dues and urgencies read the same on every run (the absolute dates
# in the details are masked). The report sorted by urgency over the
# selected task's details, a filter in Taskwarrior's syntax (with `or`),
# a new task typed with its project completed in a pop-up (Tab), and the
# Projects view.
Set Title "tasks · taskwarrior-tui on intuiTUIve"
Set Size 110x28
Set TypingDelay 40ms
Set Env TZ UTC
Output gif png svg
Mask /\d{4}-\d\d-\d\d \d\d:\d\d:\d\d/ "YYYY-MM-DD hh:mm:ss"
Hide
Exec "mkdir -p bin && ln -sf \"$(dirname \"$(command -v rich)\")/examples/tasks\" bin/tasks"
Exec "python3 $REPO/docs/tapes/fixtures/tasks/make.py store"
Type "export PATH=\"$PWD/bin:$PATH\"; clear"
Enter
Wait /❯\s*$/
Show
Type "tasks store"
Enter
Wait /Pay the rent/
Sleep 1s
Screenshot report
Type "/"
Wait /Filter Tasks/
Sleep 300ms
Type " project:work or +phone"
Enter
Wait /3 *5d *work/
Sleep 800ms
Screenshot filter
Type "/"
Wait /Filter Tasks/
Ctrl+U
Type "status:pending"
Enter
Wait /Pay the rent/
Sleep 300ms
Type "a"
Wait /Add Task/
Sleep 300ms
Type "Water the plants project:h"
Tab
Sleep 800ms
Screenshot add
Escape
Sleep 300ms
Escape
Sleep 300ms
Type "]"
Wait /5 projects/
Sleep 800ms
Screenshot projects
Type "q"
Wait /❯\s*$/
