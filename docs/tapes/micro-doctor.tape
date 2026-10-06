# How a terminal's drawing mode is chosen (docs/guide/micro/terminals.md):
# `rich doctor`'s micro line under the variables a terminal sets. TERM and
# TERM_PROGRAM are set by hand here to show the choice each terminal gets;
# the PTY itself draws no images, so this records the selection, not the
# drawing. RICH_CELL_PIXELS gives the cell size images are fitted to.
# `micro` (defined out of view) prints doctor's one micro line; doctor runs
# under script(1) so it still sees a terminal, since a pipe would rightly
# get the text fallback. Each command waits for the prompt to return, so
# no keystroke reaches the shell before it is ready for input.
Set Title "rich doctor · which micro mode, and why"
Set Size 110x13
Set TypingDelay 35ms
Hide
Type "micro() { script -qc 'rich doctor' /dev/null | grep 'Micro assets'; }; clear"
Enter
Wait /❯\s*$/
Show
Type "micro"
Enter
Wait /Micro assets: blocks/
Wait /❯\s*$/
Type "TERM=xterm-kitty micro"
Enter
Wait /Micro assets: kitty/
Wait /❯\s*$/
Type "TERM_PROGRAM=iTerm.app micro"
Enter
Wait /Micro assets: iterm/
Wait /❯\s*$/
Type "TERM=xterm-kitty RICH_CELL_PIXELS=10x20 micro"
Enter
Wait /cell size 10x20/
Wait /❯\s*$/
Type "RICH_MICRO=text micro"
Enter
Wait /Micro assets: text/
Wait /❯\s*$/
Sleep 800ms
Screenshot doctor
