# rs-rich-interact's help overlay (#473), over the overlays example
# (cargo build -p rs-rich-interact --examples): every key the file list
# and the overlays have, grouped by context and searched as you type. F1
# opens it; a tape cannot press F1, so it opens from the palette, which
# sends the list F1. The spinner in the status bar moves, so it is masked.
Set Title "rich_interact · the help overlay"
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
Type "help"
Wait /overlays\s+help/
Enter
Wait /Search keys/
Screenshot help
Type "page"
Wait /Search keys › page/
Wait /2 keys · type/
Screenshot search
Escape
Wait /15 keys/
Escape
Wait /enter pick · esc cancel/
Escape
Wait /Cancelled/
