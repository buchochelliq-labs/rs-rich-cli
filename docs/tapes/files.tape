# A Yazi-style file manager rebuilt on intuiTUIve (examples/files.rs), in
# the fixture project docs/tapes/fixtures/project (plus a link and a
# script and an image): the parent, current and preview columns with
# Yazi's status line, a highlighted source preview loaded in the
# background, the which-key pop-up after `,` and the sort it picks, the
# help, a selection, the filter as it is typed, an image previewed, the
# confirmation before trashing, the rename prompt below the hovered row,
# the "Open with:" pick, a notification, the right-click menu, the command
# palette and a second tab. CI builds the example with its `watch` and
# `art` features, so the image is drawn.
Set Title "files · a Yazi-style file manager on intuiTUIve"
Set Size 100x26
Set TypingDelay 40ms
Hide
Exec "mkdir -p bin && ln -sf \"$(dirname \"$(command -v rich)\")/examples/files\" bin/files"
Exec "mkdir -p .home/project && cp -r $REPO/docs/tapes/fixtures/project/. .home/project/"
Exec "cd .home/project && ln -s README.md readme-link && printf '#!/bin/sh\\necho hi\\n' > run.sh && chmod 755 run.sh && cp $REPO/crates/rich-micro/builtin/fun/coffee/static.png logo.png"
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
Type "f"
Wait /Filter:/
Type "logo"
Wait /filter: logo/
Enter
Wait /NOR.*logo\.png/
Sleep 1s
Screenshot files-image
Escape
Type "G"
Type "d"
Wait /Trash 1 selected file/
Wait /# orbit/
Sleep 300ms
Screenshot files-confirm
Type "n"
Type "r"
Wait /Rename:/
Sleep 300ms
Screenshot files-rename
Escape
Type "O"
Wait /Open with:/
Sleep 300ms
Screenshot files-open
Escape
RightClick 30 3
Wait /Copy path/
Wait /echo hi/
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
Type "a"
Wait /Create:/
Type "../outside"
Enter
Wait /must stay/
Sleep 300ms
Screenshot files-notify
Type "q"
Wait /❯\s*$/
