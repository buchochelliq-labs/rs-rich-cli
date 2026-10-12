# intuiTUIve's tables (0.0.18): rs-rich-intuituive's `tables` example, a
# process table built with `table_with` and `TableOptions`. A click on a
# header sorts by it and a second reverses it, ←/→ move the cell cursor
# and `s` sorts by its column, and dragging the gap after a header
# resizes that column.
Set Title "intuiTUIve · sorting, resizing and a cell cursor"
Set Size 80x20
Set TypingDelay 40ms
Hide
Exec "mkdir -p bin && ln -sf \"$(dirname \"$(command -v rich)\")/examples/tables\" bin/tables"
Type "export PATH=\"$PWD/bin:$PATH\"; clear"
Enter
Wait /❯\s*$/
Show
Type "tables"
Enter
Wait /sorted by CPU % ▼/
Sleep 800ms
Click 45 1
Wait /sorted by Memory ▲/
Sleep 600ms
Click 45 1
Wait /sorted by Memory ▼/
Sleep 600ms
Screenshot sorted
Left 2
Down 2
Wait /User: dev/
Sleep 400ms
Type "s"
Wait /sorted by User ▲/
Sleep 600ms
Screenshot cells
Drag 25 1 35 1
Wait /server\.js           app/
Sleep 600ms
Screenshot resized
Type "q"
Wait /❯\s*$/
