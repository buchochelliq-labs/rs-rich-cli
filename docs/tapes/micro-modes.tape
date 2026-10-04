# One line of micro assets in the two drawing modes a pseudo-terminal can
# show (docs/guide/micro/library.md): the default for a colour terminal
# with no image protocol (each asset's emoji) and RICH_MICRO=blocks (its
# image as half-block cells), then one asset previewed with both of its
# fallbacks and its frames magnified. The PTY speaks no image protocol, so
# Kitty, iTerm2 and Sixel, which draw the images themselves in the same
# cells, are what it cannot record. RICH_ANIMATION=0 keeps animated assets
# on their still frame in the printed lines.
Set Title "rich micro · drawing modes"
Set Size 92x31
Set TypingDelay 40ms
Hide
Type "export RICH_ANIMATION=0 LINE='Build :micro:status/success:  tests :micro:status/warning:  deploy :micro:status/error:  :micro:fun/heart: :micro:dev/bug: :micro:fun/coffee:'; clear"
Enter
Wait /❯\s*$/
Show
Type "rich -p --emoji \"$LINE\""
Enter
Wait /Build ✅/
Wait /❯\s*$/
Type "RICH_MICRO=blocks rich -p --emoji \"$LINE\""
Enter
Wait /❯.*\n.*Build [▀▄█ ]/
Wait /❯\s*$/
Type "rich micro preview fun/heart"
Enter
Wait /Magnified/
Wait /❯\s*$/
Sleep 800ms
Screenshot modes
