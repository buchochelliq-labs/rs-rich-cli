# rich schema: SQL DDL as a tree of tables, with what the reader skipped
# noted, then the same tables as an ER diagram (`--er`). The file is the
# fixture docs/tapes/fixtures/data/shop.sql.
Set Title "rich schema · DDL as a tree and an ER diagram"
Set Size 160x30
Set TypingDelay 40ms
Hide
Exec "cp $REPO/docs/tapes/fixtures/data/shop.sql ."
Type "clear"
Enter
Wait /❯\s*$/
Show
Type "rich schema shop.sql"
Enter
Wait "CREATE INDEX statement skipped"
Sleep 1500ms
Screenshot tree
Hide
Type "clear"
Enter
Show
Type "rich schema --er shop.sql"
Enter
Wait "CREATE INDEX statement skipped"
Sleep 1500ms
Screenshot er
