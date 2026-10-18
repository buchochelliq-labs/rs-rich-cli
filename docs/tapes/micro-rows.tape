# Micro assets two rows high (#711; docs/guide/micro/index.md#two-rows): a
# 2×2 icon in a line of text, where it reserves the row below the line, and
# in `rich micro show`'s table, where its row grows to hold it; then a 3×2
# animation previewed with its frames magnified. The PTY speaks no image
# protocol, so the first line shows each asset's fallback (the emoji on the
# first row, a blank row below) and the second RICH_MICRO=blocks half-blocks
# over both rows; Kitty, iTerm2 and Sixel draw the images in the same cells.
# RICH_ANIMATION=0 keeps animated assets on their still frame in the printed
# lines.
Set Title "rich micro · two rows"
Set Size 92x34
Set TypingDelay 40ms
Hide
Type "export RICH_ANIMATION=0 LINE='Release :micro:fun/trophy: shipped, team night at the :micro:fun/campfire: :micro:status/success:'; clear"
Enter
Wait /❯\s*$/
Show
Type "rich -p --emoji \"$LINE\""
Enter
Wait /Release 🏆/
Wait /❯\s*$/
Type "RICH_MICRO=blocks rich -p --emoji \"$LINE\""
Enter
Wait /❯.*\n.*Release [▀▄█ ]/
Wait /❯\s*$/
Type "rich micro show fun/trophy"
Enter
Wait /Alt text/
Wait /❯\s*$/
Sleep 800ms
Screenshot icon
Type "clear; rich micro preview fun/campfire"
Enter
Wait /Magnified/
Wait /❯\s*$/
Sleep 800ms
Screenshot animation
