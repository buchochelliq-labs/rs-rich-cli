# A Yazi-style file manager rebuilt on intuiTUIve (examples/files.rs), in
# the fixture project docs/tapes/fixtures/project: the parent, current and
# preview columns, a highlighted source preview loaded in the background,
# the keys overlay, a second tab, and the filter prompt anchored above
# the status line.
Set Title "files · a Yazi-style file manager on intuiTUIve"
Set Size 110x22
Set TypingDelay 40ms
Hide
Exec "mkdir -p bin && ln -sf \"$(dirname \"$(command -v rich)\")/examples/files\" bin/files"
Exec "mkdir -p .home/project && cp -r $REPO/docs/tapes/fixtures/project/. .home/project/"
Type "export PATH=\"$PWD/bin:$PATH\"; cd ~/project; clear"
Enter
Wait /❯\s*$/
Show
Type "files"
Enter
Wait /keys/
Enter
Wait /main\.rs/
Down
Down
Wait /fn /
Sleep 500ms
Screenshot files
Type "?"
Wait /Help/
Sleep 300ms
Screenshot files-help
Escape
Ctrl+P
Wait /yank/
Sleep 300ms
Screenshot files-palette
Escape
RightClick 20 3
Wait /Yank path/
Sleep 300ms
Screenshot files-menu
Escape
Type "t"
Type "h"
Wait /2 project/
Sleep 300ms
Screenshot files-tabs
Type "/"
Wait /Filter/
Sleep 300ms
Screenshot files-filter
Escape
Type "q"
Wait /❯\s*$/
