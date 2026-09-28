# rich asset: an emoji by name, then a box style with its preview, used
# together for a panel.
Set Title "rich asset · emoji, boxes and spinners"
Set Size 100x20
Set TypingDelay 45ms
Type "icon=$(rich asset)"
Enter
Wait /enter pick/
Type "rocket"
Wait /rocket/
Sleep 500ms
Screenshot emoji
Enter
Wait /❯\s*$/
Hide
Type "clear"
Enter
Show
Type "style=$(rich asset --kind box)"
Enter
Wait /enter pick/
Type "heavy"
Wait /heavy_head/
Sleep 500ms
Screenshot box
Enter
Wait /❯\s*$/
Type "rich --print \"$icon Launch day\" --panel \"$style\""
Enter
Wait /Launch day/
Sleep 500ms
Screenshot answer
