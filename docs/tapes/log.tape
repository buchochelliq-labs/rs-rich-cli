# rich log (0.0.19, #724): one log in four formats (JSON Lines, logfmt, a
# timestamped line and an access log, with a stack trace after an error),
# read as records; then filtered by level and field, folded where it
# repeats, and grouped by trace id with a summary of the levels and the
# traces that failed. The file is the fixture docs/tapes/fixtures/log/app.log.
Set Title "rich log · filter, fold and group a log"
Set Size 120x34
Set TypingDelay 40ms
Hide
Exec "cp $REPO/docs/tapes/fixtures/log/app.log ."
Type "clear"
Enter
Wait /❯\s*$/
Show
Type "rich log app.log"
Enter
Wait "WARN disk 91% full"
Sleep 1500ms
Screenshot records
Hide
Type "clear"
Enter
Show
Type "rich log app.log --level warn+ --where 'service=api'"
Enter
Wait "TimeoutError"
Sleep 1500ms
Screenshot filtered
Hide
Type "clear"
Enter
Show
Type "rich log app.log --where 'msg~health' --collapse --time relative"
Enter
Wait /× 3/
Sleep 1500ms
Screenshot folded
Hide
Type "clear"
Enter
Show
Type "rich log app.log --group trace"
Enter
Wait "traces with errors"
Sleep 2000ms
Screenshot groups
