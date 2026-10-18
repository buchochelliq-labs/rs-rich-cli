# ratatui's widget examples as a gallery on intuiTUIve
# (examples/widgets.rs): each tab, and a move inside it. Nothing in it
# runs on a clock, so the screenshots are the same on every run.
Set Title "widgets · ratatui's widget examples on intuiTUIve"
Set Size 80x24
Set TypingDelay 40ms
Hide
Exec "mkdir -p bin && ln -sf \"$(dirname \"$(command -v rich)\")/examples/widgets\" bin/widgets"
Type "export PATH=\"$PWD/bin:$PATH\"; clear"
Enter
Wait /❯\s*$/
Show
Type "widgets"
Enter
Wait /Bordered/
Sleep 1500ms
Screenshot block
Right
Wait /Centred/
Sleep 500ms
Type "m"
Sleep 300ms
Screenshot text
Type "m"
Right
Wait /> Apples/
Down
Down
Tab
Up
Sleep 300ms
Screenshot list
Right
Wait /Lentil soup/
Down
Down
Right
Sleep 300ms
Screenshot table
Type "v"
Wait /Table app/
Down
Type "c"
Sleep 300ms
Screenshot table-app
Type "5"
Wait /In the middle/
Type "n"
Type "n"
Sleep 300ms
Screenshot tabs
Type "6"
Wait /with arrows/
Type "jjjll"
Sleep 300ms
Screenshot scrollbar
Type "7"
Wait /October 2026/
Sleep 300ms
Screenshot calendar
Type "8"
Wait /Behind/
Type "p"
Wait /p or Esc closes it/
Sleep 300ms
Screenshot popup
Escape
Type "9"
Wait /Top left/
Down
Down
Sleep 300ms
Screenshot borders
Type "0"
Wait /Press a button/
Down
Enter
Sleep 300ms
Screenshot custom
Type "?"
Wait /the Block tab/
Sleep 300ms
Screenshot keys
Escape
Type "q"
Wait /❯\s*$/
