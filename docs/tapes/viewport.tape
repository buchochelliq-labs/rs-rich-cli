# rs-rich-interact's Viewport, as its example pager: a Markdown file paged on
# the alternate screen, repainted cell by cell, and the terminal given back.
# The example is built next to `rich` (cargo build -p rs-rich-interact
# --examples), so its path comes from rich's.
Set Title "rich_interact::Viewport · the example pager"
Set Size 76x18
Hide
Exec "mkdir -p bin && ln -sf \"$(dirname \"$(command -v rich)\")/examples/viewport\" bin/viewport"
Exec "cp $REPO/docs/tapes/fixtures/project/README.md ."
Type "export PATH=\"$PWD/bin:$PATH\"; clear"
Enter
Wait /❯\s*$/
Show
Type "viewport README.md"
Enter
Wait /lines 1–17/
Screenshot top
PageDown
Wait /lines 18–34/
Screenshot paged
Down 3
Wait /lines 21–37/
End
Wait /lines \d+–\d+ of \d+/
Sleep 300ms
Enter
Wait /left at line/
Screenshot returned
