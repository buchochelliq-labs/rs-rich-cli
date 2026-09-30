# Micro assets in rs-rich-interact's chrome (0.0.14 workstream 7), over
# the micro_showcase example (cargo build -p rs-rich-cli -p rs-rich-interact
# --examples, which turns on interact's micro feature): an icon per row, a
# micro-asset badge in the status bar, an icon on the first crumb, and
# (Ctrl+O) icons before the palette's categories. The PTY has no image
# protocol, so the painter leaves each asset's emoji fallback in its cells;
# a Kitty, iTerm2 or Sixel terminal gets real pixels there, and
# RICH_MICRO=blocks colour half-blocks.
# RICH_ANIMATION=0 keeps status/loading on its still frame.
Set Title "rich_interact · micro assets in the chrome"
Set Size 100x24
Set TypingDelay 45ms
Hide
Exec "mkdir -p bin && ln -sf \"$(dirname \"$(command -v rich)\")/examples/micro_showcase\" bin/micro_showcase"
Type "export PATH=\"$PWD/bin:$PATH\" RICH_ANIMATION=0; clear"
Enter
Wait /❯\s*$/
Show
Type "micro_showcase"
Enter
Wait /3 checks passed/
Sleep 500ms
Screenshot chrome
Ctrl+O
Wait /Commands/
Sleep 500ms
Screenshot palette
Type "deploy"
Wait /Mark the deploy done/
Sleep 300ms
Screenshot search
Enter
Wait /3 checks passed/
Escape
Wait /❯\s*$/
