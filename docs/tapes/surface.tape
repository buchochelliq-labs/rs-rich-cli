# A `Surface` (0.0.19): rs-rich-ext's `guide_surface` example draws a small
# dashboard into rectangles of an inline surface, eleven rows below the
# prompt, and redraws it every half second. Only the cells that changed
# are sent; the scrollback above stays as it was, and the last frame stays
# on screen when it ends.
Set Title "rs-rich-ext · a surface"
Set Size 64x18
Set TypingDelay 40ms
Hide
Exec "mkdir -p bin && ln -sf \"$(dirname \"$(command -v rich)\")/examples/guide_surface\" bin/guide_surface"
Type "export PATH=\"$PWD/bin:$PATH\"; clear"
Enter
Wait /❯\s*$/
Show
Type "echo 'earlier output stays above'"
Enter
Wait /❯\s*$/
Type "guide_surface"
Enter
Wait /tick 1 /
Sleep 200ms
Screenshot ticking
Wait /tick 4 /
Wait /❯\s*$/
Sleep 600ms
Screenshot done
