# rich color: a named colour found by typing, then the 256 palette on Tab;
# the colour is printed as rich takes it.
Set Title "rich color · pick a colour"
Set Size 100x26
Set TypingDelay 45ms
Type "accent=$(rich color --format name)"
Enter
Wait /tab palette/
Type "orange"
Wait /dark_orange/
Sleep 500ms
Screenshot names
Tab
Right 4
Down 12
Wait /red1/
Sleep 500ms
Screenshot palette
Enter
Wait /❯\s*$/
Type "rich --print \"[bold $accent]Hello from $accent[/]\""
Enter
Wait /Hello from/
Sleep 500ms
Screenshot answer
