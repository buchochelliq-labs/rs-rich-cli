# rich explore --icons: true, false and null values marked with the
# built-in status/success, status/error and status/info micro assets. The
# explorer runs without the graphics side channel and the PTY has no image
# protocol anyway, so each asset shows its emoji fallback.
Set Title "rich explore · micro-asset icons"
Set Size 100x22
Set TypingDelay 45ms
Hide
Exec "cp $REPO/docs/tapes/fixtures/explore/config.yaml ."
Type "clear"
Enter
Wait /❯\s*$/
Show
Type "rich explore --icons config.yaml"
Enter
Wait /6 keys/
Down 6
Right
Wait /recommendations/
Sleep 500ms
Screenshot icons
Escape
Wait /❯\s*$/
