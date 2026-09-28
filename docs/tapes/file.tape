# rich file: browse the fixture project, preview a file, open a directory
# and pick a file; the path goes to stdout for the script.
Set Title "rich file · pick a path"
Set Size 100x20
Set TypingDelay 45ms
Hide
Exec "cp -r $REPO/docs/tapes/fixtures/project/. ."
Type "clear"
Enter
Wait /❯\s*$/
Show
Type "path=$(rich file --header Open)"
Enter
Wait /Cargo\.toml/
Sleep 500ms
Screenshot browse
Type "src"
Wait /1\/\d+ ·/
Right
Wait /event_loop\.rs/
Type "loop"
Wait /1\/\d+ ·/
Sleep 500ms
Screenshot preview
Enter
Wait /❯\s*$/
Type "echo \"$path\""
Enter
Wait /src\/event_loop\.rs\s*\n[^\n]*❯/
Sleep 500ms
Screenshot answer
