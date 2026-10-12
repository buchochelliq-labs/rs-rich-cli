# intuiTUIve's stylesheets (0.0.18): rs-rich-intuituive's `styled`
# example, a status page whose colours, borders, sizes and docking come
# from crates/rich-intuituive/examples/styled.tcss, copied here as
# app.tcss. Selectors by kind (`table`), name (`#title`) and class
# (`.card`, and `.down` from `class_when`), with `:focus` and `:hover`.
# Then the sheet is edited from outside the terminal while the app runs:
# the app reads it again, and a line it cannot parse is reported in a
# toast while the last good sheet stays.
Set Title "intuiTUIve · a stylesheet, reloaded live"
Set Size 90x20
Set TypingDelay 40ms
Hide
Exec "mkdir -p bin && ln -sf \"$(dirname \"$(command -v rich)\")/examples/styled\" bin/styled"
Exec "cp $REPO/crates/rich-intuituive/examples/styled.tcss app.tcss"
Type "export PATH=\"$PWD/bin:$PATH\"; clear"
Enter
Wait /❯\s*$/
Show
Type "styled app.tcss"
Enter
Wait /shop · production/
Sleep 1s
Screenshot styled
Tab
Sleep 600ms
MouseMove 50 3
Sleep 1s
Screenshot states
Exec "sed -i 's/round grey50/heavy grey50/; s/background: $accent; color: white/background: magenta; color: white/' app.tcss"
Wait /┏━ checkout/
Sleep 1s
Screenshot reloaded
Exec "echo '#status { colour: grey50; }' >> app.tcss"
Wait /unknown property/
Sleep 1s
Screenshot error
Type "q"
Wait /❯\s*$/
