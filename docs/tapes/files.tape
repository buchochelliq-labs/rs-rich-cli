# A Yazi-style file manager rebuilt on intuiTUIve (examples/files.rs), in
# the fixture project docs/tapes/fixtures/project (plus a link and a
# script): the parent, current and preview columns with Yazi's status line,
# a highlighted source preview loaded in the background, the which-key
# pop-up after `,` and the sort it picks, the help, a selection, the
# filter as it is typed, the confirmation before trashing, the right-click
# menu, the command palette and a second tab.
Set Title "files · a Yazi-style file manager on intuiTUIve"
Set Size 100x26
Set TypingDelay 40ms
Hide
Exec "mkdir -p bin && ln -sf \"$(dirname \"$(command -v rich)\")/examples/files\" bin/files"
Exec "mkdir -p .home/project && cp -r $REPO/docs/tapes/fixtures/project/. .home/project/"
Exec "cd .home/project && ln -s README.md readme-link && printf '#!/bin/sh\\necho hi\\n' > run.sh && chmod 755 run.sh"
Type "export PATH=\"$PWD/bin:$PATH\"; cd ~/project; clear"
Enter
Wait /❯\s*$/
Show
Type "files"
Enter
Wait /NOR/
Type "l"
Wait /event_loop/
Type "jj"
Wait /fn main/
Sleep 500ms
Screenshot files
Type "h"
Wait /Cargo/
Type ","
Wait /Sort by size/
Sleep 300ms
Screenshot files-which
Type "s"
Wait /1\.4K/
Type "~"
Wait /mgr\.help/
Sleep 300ms
Screenshot files-help
Escape
Space
Space
Sleep 300ms
Screenshot files-select
Escape
Type "f"
Wait /Filter:/
Type "md"
Wait /filter: md/
Wait /# orbit/
Sleep 300ms
Screenshot files-filter
Enter
Escape
Type "G"
Type "d"
Wait /Trash 1 selected file/
Wait /# orbit/
Sleep 300ms
Screenshot files-confirm
Type "n"
RightClick 30 3
Wait /Copy path/
Sleep 300ms
Screenshot files-menu
Escape
Ctrl+P
Wait /Commands/
Sleep 300ms
Screenshot files-palette
Escape
Type "gg"
Type "tt"
Type "l"
Wait /2 src/
Sleep 300ms
Screenshot files-tabs
Type "q"
Wait /❯\s*$/
