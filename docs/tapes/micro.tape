# rich micro (#568, #581, #587): the built-in library listed, one asset
# previewed inline and magnified, and :micro: codes in --print --emoji
# text. Recorded in a PTY, which speaks no image protocol (no Kitty, iTerm2
# or Sixel), so every asset shows its fallback: its emoji, or with
# RICH_MICRO=blocks (the last command) colour half-blocks drawn from its
# image. A terminal with an image protocol draws real pixels in the same
# cells. RICH_ANIMATION=0 shows each animation's still frame, so the
# screenshots are stable.
Set Title "rich micro · emoji-sized images"
Set Size 100x30
Set TypingDelay 45ms
Hide
Type "export RICH_ANIMATION=0; clear"
Enter
Wait /❯\s*$/
Show
Type "rich micro list --layer built-in"
Enter
Wait /status\/warning/
Sleep 500ms
Screenshot list
Hide
Type "clear"
Enter
Show
Type "rich micro preview status/loading"
Enter
Wait /Magnified/
Sleep 500ms
Screenshot preview
Hide
Type "clear"
Enter
Show
Type "rich -p --emoji \"Deploying :micro:status/loading: then :micro:status/success: :sparkles:\""
Enter
Wait /Deploying ⏳/
Wait /❯\s*$/
Type "RICH_MICRO=blocks rich -p --emoji \"Blocks: :micro:dev/bug: :micro:fun/cat: :micro:status/warning:\""
Enter
Wait /Blocks: [▀▄█ ]/
Sleep 500ms
Screenshot markup
