# intuiTUIve's text area (0.0.19): rs-rich-intuituive's `editor` example.
# Rust source highlighted with line numbers over a notes area that wraps:
# a line typed at the end, a selection made with Shift, the typing undone
# as one step with Ctrl+Z, then Escape and Tab to move on to the notes.
Set Title "intuiTUIve · a text area"
Set Size 80x32
Set TypingDelay 40ms
Hide
Exec "mkdir -p bin && ln -sf \"$(dirname \"$(command -v rich)\")/examples/editor\" bin/editor"
Type "export PATH=\"$PWD/bin:$PATH\"; clear"
Enter
Wait /❯\s*$/
Show
Type "editor"
Enter
Wait /count\.rs \d+ lines/
Sleep 800ms
Screenshot editor
Ctrl+End
Type "// ties sort by word"
Wait /ties sort by word/
Sleep 500ms
Up 4
End
Shift+Home
Sleep 800ms
Screenshot selected
Ctrl+Z
Sleep 800ms
Screenshot undone
Escape
Tab
Type "Check the ties: bat, cat and hat come out in that order."
Wait /notes 12 words/
Sleep 800ms
Screenshot notes
Ctrl+Q
Wait /❯\s*$/
