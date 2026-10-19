# A diskonaut-style disk-space treemap rebuilt on intuiTUIve
# (examples/disk.rs), over a folder the tape makes of sparse files (sized
# by their apparent size, `-a`, so the map is the same on every disk): the
# map once the scan is done, a tile selected with the arrows, a folder
# entered, and the question before a delete (answered no). The work
# folder's path is masked, and so is the total's line before it: which of
# its forms fits depends on the path's length, and it says "(root)" when
# the tape runs as root.
Set Title "disk · diskonaut on intuiTUIve"
Set Size 90x24
Set TypingDelay 40ms
Output gif png svg
Mask /Total: [^|]*\|/ "Total: …|"
Mask /\/\S*\/space/ "<tmp>/space"
Hide
Exec "mkdir -p bin && ln -sf \"$(dirname \"$(command -v rich)\")/examples/disk\" bin/disk"
Exec "mkdir -p space/target/debug space/target/release space/assets/video space/assets/audio space/src space/docs"
Exec "cd space && truncate -s 900M target/debug/app && truncate -s 300M target/debug/deps.rlib && truncate -s 400M target/release/app && truncate -s 600M assets/video/intro.mp4 && truncate -s 250M assets/audio/theme.wav && truncate -s 40M assets/audio/click.wav && truncate -s 120M docs/manual.pdf && truncate -s 2M src/main.rs && truncate -s 1M src/lib.rs && truncate -s 30M notes.tar && for i in 1 2 3 4 5 6; do truncate -s ${i}00K small-$i.txt; done"
Type "export PATH=\"$PWD/bin:$PATH\"; clear"
Enter
Wait /❯\s*$/
Show
Type "disk -a space"
Enter
Wait /Total: /
Sleep 1s
Screenshot map
Right
Wait /SELECTED: target/
Right
Wait /SELECTED: assets/
Down
Wait /SELECTED: docs/
Sleep 500ms
Screenshot selected
Left
Wait /SELECTED: target/
Enter
Wait /release/
Sleep 800ms
Screenshot folder
Escape
Sleep 500ms
Right
Wait /SELECTED: assets/
Sleep 300ms
Backspace
Wait /\(y\/n\)/
Sleep 500ms
Screenshot delete
Type "n"
Sleep 500ms
Type "q"
Wait /quit/
Type "y"
Wait /❯\s*$/
