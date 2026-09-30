# rs-rich-interact's status bar (#481) and breadcrumbs (#482), round the
# overlays example (cargo build -p rs-rich-interact --examples): a badge,
# a spinner, a note and key hints read from the keymap under the list,
# the path over it. Ctrl+K opens the region's actions in a modal (#474);
# running one, or a command from the palette, changes the note. The
# spinner moves, so it is masked.
Set Title "rich_interact · status bar and breadcrumbs"
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
Screenshot status
Ctrl+K
Wait /Actions · Files/
Down
Wait /❯ Show hidden files/
Screenshot actions
Enter
Wait /Show hidden files · Files/
Ctrl+O
Wait /Commands/
Type "refresh"
Wait /Command › refresh/
Wait /1\/16 · ↑↓ move/
Enter
Wait /refreshed/
Screenshot refreshed
Enter
Wait /opened src\/lib\.rs/
