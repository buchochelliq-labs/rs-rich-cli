# rs-rich-embed's terminal pane (0.0.18): its `embed` example, a shell
# in a terminal pane beside a web view, with a status line built from both
# panes' signals. Every key goes to the program in the focused pane: here
# bash (the example runs `$SHELL`; here bash with a short prompt) lists
# the directory and runs `rich` on a CSV file. F6 moves to the web view,
# where w3m (the browser the example names; CI installs it) shows a local
# page. Ctrl+L edits the address (a local file as `file:`, since an
# address without a scheme gets `https://`), which the view opens and the
# status line follows. F10 quits.
Set Title "rs-rich-embed · a terminal pane beside a web view"
Set Size 100x22
Set TypingDelay 40ms
Hide
Exec "mkdir -p bin && ln -sf \"$(dirname \"$(command -v rich)\")/examples/embed\" bin/embed"
Exec "cp $REPO/docs/tapes/fixtures/embed/* ."
Write bin/pane-shell "#!/bin/sh\nexport PS1='$ '\nexec bash --norc --noprofile\n"
Exec "chmod +x bin/pane-shell"
Type "export PATH=\"$PWD/bin:$PATH\" SHELL=pane-shell; clear"
Enter
Wait /❯\s*$/
Show
Type "embed runbook.html"
Enter
Wait /Checkout runbook/
Sleep 1s
Type "ls"
Enter
Wait /deploys\.csv/
Sleep 400ms
Type "rich deploys.csv"
Enter
Wait /v1\.9\.3/
Sleep 1s
Screenshot pane
F6
Sleep 600ms
Ctrl+L
Sleep 400ms
Backspace 12
Type "file:deploys.html"
Enter
Wait /Recent deploys/
Wait /· file:deploys\.html ·/
Sleep 1s
Screenshot followed
F10
Wait /❯\s*$/
