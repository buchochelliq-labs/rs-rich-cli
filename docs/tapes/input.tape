# `rich input` in a script: a placeholder while the line is empty, the
# error --required shows for an empty answer, then an answer captured with
# $(...). Recorded against the binary under test, and checked in CI.
Set Title "rich input · ask for a line"
Set Caption "--placeholder shows a hint, --required refuses an empty line"
Set Size 88x8
Set TypingDelay 35ms
Hide
Type "clear"
Enter
Wait /❯\s*$/
Show
Type "name=$(rich input --prompt 'Project name' --placeholder rs-rich-demo --required)"
Enter
Wait /rs-rich-demo/
Screenshot placeholder
Enter
Wait /an answer is required/
Screenshot required
Type "tapes"
Wait /tapes/
Screenshot typed
Enter
Type "echo \"creating $name (exit $?)\""
Enter
Wait /creating tapes \(exit 0\)/
Screenshot answer
