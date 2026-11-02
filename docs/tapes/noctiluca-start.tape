# Noctiluca's start page (0.0.24): started with no address, the
# NOCTILUCA banner with its glow, wave and rising specks (a fixed seed),
# the lanternfish swimming in to settle beside it, the key hints and the
# bookmarks (two, from docs/tapes/fixtures/noctiluca/make.py's data
# directory); then the lanternfish, reached with Tab, opens the about page
# with Enter, and Esc comes back. It replays Chrome's saved snapshots of
# the crate's fixture pages, which needs the binary built with the
# test-only `fake-engine` feature. The animation plays on the real clock,
# so no screenshot is taken mid-animation: the GIF shows it, and the
# screenshots are of the still page. The about page's browser line is
# masked: it names whatever the replay reports.
Set Title "Noctiluca · the start page"
Set Size 120x36
Set TypingDelay 40ms
Mask /Browser: .*/ "Browser: (replayed)"
Hide
Exec "python3 $REPO/docs/tapes/fixtures/noctiluca/make.py $REPO ."
Type "export NOCTILUCA_FAKE_ENGINE=$PWD/pages XDG_CONFIG_HOME=$PWD/config XDG_DATA_HOME=$PWD/data RICH_ANIMATION=1; clear"
Enter
Wait /❯\s*$/
Show
Type "noctiluca"
Enter
Wait /a web browser for the terminal/
Sleep 3s
Screenshot start
Tab
Sleep 400ms
Enter
Wait /Codecs/
Sleep 800ms
Screenshot about
Escape
Wait /a web browser for the terminal/
Sleep 800ms
Screenshot back
Type "q"
Wait /❯\s*$/
