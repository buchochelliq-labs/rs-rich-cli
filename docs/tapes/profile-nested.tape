# rich profile of nested JSON Lines (0.0.19, #705): each path's type mix,
# the records that lack it, mixed-type paths marked `!` and a malformed line
# counted; then `--style-rule` (#701) styling a CSV table by value. The files
# are the fixtures docs/tapes/fixtures/data/requests.jsonl and orders.csv.
Set Title "rich profile · nested JSON Lines and style rules"
Set Size 100x50
Set TypingDelay 40ms
Hide
Exec "cp $REPO/docs/tapes/fixtures/data/requests.jsonl $REPO/docs/tapes/fixtures/data/orders.csv ."
Type "clear"
Enter
Wait /❯\s*$/
Show
Type "rich profile requests.jsonl --width 100 --force-terminal | head -25"
Enter
Wait /└─+┴/
Sleep 1500ms
Screenshot nested
Hide
Type "clear"
Enter
Show
Type "rich orders.csv --head 10 --style-rule 'amount>100:bold red' --style-rule 'status=cancelled:row:dim'"
Enter
Wait /└─+┴/
Sleep 1500ms
Screenshot rules
