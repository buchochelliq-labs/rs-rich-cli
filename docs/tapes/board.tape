# intuiTUIve's lazy tree, tooltips and drag-and-drop (0.0.18):
# rs-rich-intuituive's `board` example. Past sprints sit in a `tree_lazy`
# that loads each one on another thread when it is opened (the example
# waits 1.2 s, as for a tracker over the network), showing "loading…"
# meanwhile and the error in red when the oldest sprint's archive is
# offline. The pointer resting on a card shows its tooltip, and a card
# dragged to another lane moves there.
Set Title "intuiTUIve · a lazy tree, tooltips and drag-and-drop"
Set Size 104x18
Set TypingDelay 40ms
Hide
Exec "mkdir -p bin && ln -sf \"$(dirname \"$(command -v rich)\")/examples/board\" bin/board"
Type "export PATH=\"$PWD/bin:$PATH\"; clear"
Enter
Wait /❯\s*$/
Show
Type "board"
Enter
Wait /Past sprints/
Sleep 800ms
Right
Wait /loading…/
Screenshot loading
Wait /Gift cards/
Sleep 600ms
Down 5
Wait /tree: Sprint 39/
Right
Wait /archive offline/
Sleep 600ms
Screenshot lazy
MouseMove 58 1
Wait /Seen when the cart/
Sleep 600ms
Screenshot tooltip
Drag 58 1 88 5
Wait /moved to Done/
Sleep 600ms
Screenshot dropped
Type "q"
Wait /❯\s*$/
