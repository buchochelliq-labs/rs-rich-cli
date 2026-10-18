# rich jump: one fuzzy box over the repository's docs and the interact
# sources (files, headings, symbols) and a commands file; narrow with Tab
# and with a prefix, and print the place picked.
Set Title "rich jump · jump to anything"
Set Size 110x26
Set TypingDelay 45ms
Hide
Exec "cp $REPO/docs/tapes/fixtures/jump/commands ."
Exec "cp -r $REPO/docs $REPO/crates/rich-interact/src ."
Type "clear"
Enter
Wait /❯\s*$/
Show
Type "place=$(rich jump --commands commands --recent .jump-recent docs src)"
Enter
Wait /results/
Sleep 500ms
Screenshot groups
Type "approval"
Wait /Approval queues/
Sleep 500ms
Screenshot search
Tab
Wait /\[Recent\]|\[Files\]/
Sleep 500ms
Screenshot narrowed
Ctrl+U
Type ">test"
Wait /cargo test/
Sleep 500ms
Screenshot commands
Ctrl+U
Type "@ApprovalQueue"
Wait /struct/
Sleep 500ms
Screenshot symbol
Enter
Wait /❯\s*$/
Type "echo \"$place\""
Enter
Wait /approve\.rs:\d+/
Sleep 500ms
Screenshot answer
