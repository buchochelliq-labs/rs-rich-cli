# intuiTUIve, rs-rich's framework for terminal apps, through its examples
# (cargo build -p rs-rich-intuituive --examples): the tutorial's to-do app,
# the same app with the inspector docked, a grid of cards with a modal,
# and an inline app that leaves its last frame in the scrollback.
Set Title "rs-rich-intuituive · terminal apps"
Set Size 100x22
Set TypingDelay 40ms
Hide
Exec "mkdir -p bin && for e in todo screens inline; do ln -sf \"$(dirname \"$(command -v rich)\")/examples/$e\" bin/$e; done"
Type "export PATH=\"$PWD/bin:$PATH\"; clear"
Enter
Wait /❯\s*$/
Show
Type "todo"
Enter
Wait /ctrl\+q quits/
Type "Record the docs"
Enter
Wait /Record the docs/
Tab
Space
Wait /2 left/
Sleep 300ms
Screenshot todo
Ctrl+Q
Wait /❯\s*$/
Hide
Type "clear"
Enter
Show
Type "INTUITUIVE_INSPECT=1 todo"
Enter
Wait /inspector/
Tab
Space
Wait /1 left/
Sleep 300ms
Screenshot inspector
Ctrl+Q
Wait /❯\s*$/
Hide
Type "clear"
Enter
Show
Type "screens"
Enter
Wait /services/
Tab
Type "q"
Wait /Quit\?/
Sleep 300ms
Screenshot modal
Type "y"
Wait /❯\s*$/
Hide
Type "clear"
Enter
Show
Type "echo 'an inline app keeps the scrollback'"
Enter
Type "inline"
Enter
Wait /finished/
Wait /❯\s*$/
Sleep 300ms
Screenshot inline
