# Making a micro asset (docs/guide/micro/authoring.md): a 128x128 PNG
# (docs/tapes/fixtures/micro/rocket.png, drawn for this project) previewed
# through the pipeline at the size it will be drawn, written into the user
# layer with `rich micro create --add`, listed, then used in text. The PTY
# speaks no image protocol, so the asset shows its emoji; a Kitty, iTerm2
# or Sixel terminal draws the image in the same two cells.
# `create` prints where it wrote the package, which is under the run's
# temporary home, so the screen is cleared before the second screenshot.
Set Title "rich micro · make your own asset"
Set Size 96x18
Set TypingDelay 40ms
Hide
Exec "cp $REPO/docs/tapes/fixtures/micro/rocket.png ."
Type "clear"
Enter
Wait /❯\s*$/
Show
Type "rich micro preview rocket.png --sharpen 0.8"
Enter
Wait /2x1 cells, 16×16 pixels/
Wait /❯\s*$/
Sleep 600ms
Screenshot pipeline
Type "rich micro create rocket.png --name team/rocket --alt 'teal rocket, lifting off' --emoji 🚀 --text '^^' --license MIT --sharpen 0.8 --add"
Enter
Wait /created team\/rocket/
Wait /❯\s*$/
Sleep 1s
Hide
Type "clear"
Enter
Wait /❯\s*$/
Show
Type "rich micro list --layer user"
Enter
Wait /team\/rocket/
Wait /❯\s*$/
Type "rich -p --emoji 'Launched :micro:team/rocket: by the team'"
Enter
Wait /Launched 🚀/
Wait /❯\s*$/
Sleep 800ms
Screenshot added
