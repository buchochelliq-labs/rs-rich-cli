# rich approve: review five JSON requests, approve one, reject one with a
# reason, mark the rest and approve them together, then submit; one JSON
# line per item comes out with its decision.
Set Title "rich approve · review a queue"
Set Size 110x22
Set TypingDelay 45ms
Hide
Exec "cp $REPO/docs/tapes/fixtures/approve/requests.jsonl ."
Type "clear"
Enter
Wait /❯\s*$/
Show
Type "rich approve --header Requests --require-reason < requests.jsonl > decisions.jsonl"
Enter
Wait /0\/5 decided/
Sleep 500ms
Screenshot queue
Type "a"
Wait /1\/5 decided/
Type "r"
Wait /Reason to reject/
Type "needs a migration plan"
Sleep 300ms
Screenshot reason
Enter
Wait /2\/5 decided/
Ctrl+A
Wait /3 marked/
Sleep 500ms
Screenshot marked
Type "a"
Wait /5\/5 decided/
Sleep 500ms
Screenshot decided
Enter
Wait /4 approved · 1 rejected/
Type "cat decisions.jsonl"
Enter
Wait /needs a migration plan/
Sleep 500ms
Screenshot output
