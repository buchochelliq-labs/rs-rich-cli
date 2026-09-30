# rs-rich-interact's command palette (#453), over the overlays example
# (cargo build -p rs-rich-interact --examples): Ctrl+O lists every key the
# file list has, by category and with its shortcut, and the example's own
# commands; typing searches them; Enter runs the one picked by sending the
# list its key. The spinner in the status bar moves, so it is masked.
Set Title "rich_interact · the command palette"
Set Size 100x26
Set TypingDelay 45ms
Mask /[⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏] indexing/ "* indexing"
Hide
Exec "mkdir -p bin && ln -sf \"$(dirname \"$(command -v rich)\")/examples/overlays\" bin/overlays"
Type "export PATH=\"$PWD/bin:$PATH\"; clear"
Enter
Wait /❯\s*$/
Show
Type "overlays"
Enter
Wait /FILES/
Ctrl+O
Wait /Commands/
Screenshot palette
Type "move down"
Wait /Command › move down/
Wait /1\/16 · ↑↓ move/
Screenshot search
Enter
Wait /❯ src\/overlay\.rs/
Screenshot ran
Enter
Wait /opened src\/overlay\.rs/
