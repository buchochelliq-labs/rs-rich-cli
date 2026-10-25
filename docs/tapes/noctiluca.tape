# Noctiluca, the terminal web browser (0.0.22, the reader phase): a page
# in reader mode, link hints, a followed link, back, and the live F1
# overlay generated from the action registry. The tape job has no
# browser it may run with its sandbox on, so this replays Chrome's saved
# snapshots of the crate's fixture pages (NOCTILUCA_FAKE_ENGINE, as the
# PTY test does); the live tests run against Chrome in CI's browser job.
Set Title "Noctiluca · a terminal web browser"
Set Size 100x30
Set TypingDelay 40ms
Hide
Type "export NOCTILUCA_FAKE_ENGINE=$REPO/crates/noctiluca/tests/snapshots XDG_CONFIG_HOME=$PWD/config; clear"
Enter
Wait /❯\s*$/
Show
Type "noctiluca --reader https://fixture.example/reader.html"
Enter
Wait /Tiny drifters of the open sea/
Sleep 1s
Screenshot reader
Type "f"
Wait /\[a\]open sea/
Sleep 600ms
Screenshot hints
Type "a"
Wait /You followed the link/
Sleep 800ms
Type "H"
Wait /Tiny drifters/
Sleep 400ms
F1
Wait /Link hints/
Sleep 1s
Screenshot actions
Escape
Sleep 300ms
Type "r"
Wait /projection/
Sleep 1s
Screenshot projection
Type "q"
Wait /❯\s*$/
