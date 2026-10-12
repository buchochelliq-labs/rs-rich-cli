# intuiTUIve's accessibility (0.0.18): rs-rich-intuituive's `access`
# example, which the screen reader checklist walks through
# (docs/guide/intuituive/screen-readers.md). First in text mode
# (`--accessible`): boxes drawn as blanks with their titles kept, no
# colour, `>` on the selected item, and the terminal's cursor on the
# focus. Then in linear mode (`--linear`): no screen at all, the
# accessibility tree written as lines, then only the lines that change,
# the focus moving as `→`, and a toast as its text.
Set Title "intuiTUIve · text mode and linear mode"
Set Size 90x24
Set TypingDelay 40ms
Hide
Exec "mkdir -p bin && ln -sf \"$(dirname \"$(command -v rich)\")/examples/access\" bin/access"
Type "export PATH=\"$PWD/bin:$PATH\"; clear"
Enter
Wait /❯\s*$/
Show
Type "access --accessible"
Enter
Wait /q quits/
Sleep 800ms
Tab
Sleep 400ms
Right
Wait /main\.rs/
Sleep 400ms
Down
Sleep 600ms
Screenshot text-mode
Type "q"
Wait /❯\s*$/
Hide
Type "clear"
Enter
Wait /❯\s*$/
Show
Type "access --linear"
Enter
Wait /q quits/
Sleep 800ms
Tab
Wait /→ Folders/
Sleep 400ms
Right
Wait /expanded/
Sleep 400ms
Down
Wait /main\.rs, selected/
Sleep 400ms
Type "s"
Wait /Saved/
Sleep 600ms
Screenshot linear
Type "q"
Wait /❯\s*$/
