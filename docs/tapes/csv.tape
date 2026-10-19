# A csvlens-style CSV viewer rebuilt on intuiTUIve (examples/csv.rs), over
# the fixture docs/tapes/fixtures/data/orders.csv: the table and csvlens's
# status line, a find (`/`) with its matches lit, a filter (`&`) that
# keeps the matching rows, cell selection (Tab) and the help (`H`).
Set Title "csv · csvlens on intuiTUIve"
Set Size 90x22
Set TypingDelay 40ms
Output gif png svg
Hide
Exec "mkdir -p bin && ln -sf \"$(dirname \"$(command -v rich)\")/examples/csv\" bin/csv"
Exec "cp $REPO/docs/tapes/fixtures/data/orders.csv orders.csv"
Type "export PATH=\"$PWD/bin:$PATH\"; clear"
Enter
Wait /❯\s*$/
Show
Type "csv orders.csv"
Enter
Wait /order_id/
Sleep 1s
Screenshot table
Type "/"
Wait /Find:/
Type "apac"
Enter
Wait /apac/
Sleep 500ms
Type "n"
Sleep 500ms
Screenshot find
Type "&"
Wait /Filter:/
Type "pending"
Enter
Sleep 800ms
Screenshot filter
Type "r"
Sleep 300ms
Tab
Tab
Sleep 300ms
Type "lll"
Sleep 800ms
Type "H"
Wait /csvlens/
Sleep 500ms
Screenshot help
Escape
Sleep 300ms
Type "q"
Wait /❯\s*$/
