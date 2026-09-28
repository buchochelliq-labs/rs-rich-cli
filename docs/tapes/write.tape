# rich write: several lines typed into a script's variable. Enter starts a
# line, Ctrl+D submits; the answer goes to stdout, so $(…) captures it.
Set Title "rich write · several lines for a script"
Set Size 90x16
Set TypingDelay 40ms
Type "msg=$(rich write --header 'Commit message' --show-line-numbers --char-limit 200)"
Enter
Wait /ctrl\+d submit/
Type "Add rich write"
Enter
Enter
Type "A multi-line editor for shell scripts,"
Enter
Type "in the manner of gum."
Wait /gum\./
Sleep 500ms
Screenshot editing
Ctrl+D
Wait /❯\s*$/
Type "printf '%s\n' \"$msg\""
Enter
Wait /in the manner of gum\.\s*\n[^\n]*❯/
Sleep 500ms
Screenshot answer
