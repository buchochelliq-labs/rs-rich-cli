# Noctiluca on a live stream page (0.0.23): the stream playing in its
# region, a role=log chat beside it with its authors in their colours and
# its badges and emotes as small pictures, scrolled up and back, the about
# page's terminal adaptations and codecs, and the faithful projection one
# key away. All of it is drawn by Noctiluca and intuiTUIve, the pictures by
# rs-rich-art. The tape job has no browser it may run with its sandbox on,
# so this replays what Chrome supplied for the fixture page
# (NOCTILUCA_FAKE_ENGINE, built with the test-only `fake-engine` feature):
# its saved snapshot, the page's own picture files, and Chrome's recorded
# frames of the video; the log is as Chrome read it, not live. The live
# tests run against Chrome in CI's browser job.
Set Title "Noctiluca · a live log beside a stream"
Set Size 110x32
Set TypingDelay 40ms
Mask /\d:\d\d \/ \d:\d\d/ "<time>"
Mask /\d:\d\d \/ live/ "<time>-live"
Hide
Type "export NOCTILUCA_FAKE_ENGINE=$REPO/crates/noctiluca/tests/snapshots XDG_CONFIG_HOME=$PWD/config; clear"
Enter
Wait /❯\s*$/
Show
Type "noctiluca --images sixel https://fixture.example/stream-with-log.html"
Enter
Wait /hello from the harbour/
# The video's first frame (a debug build takes longer to reach it): a
# frame fills the stream's region with half blocks, a placeholder with ░.
Wait /▀{30}/
Sleep 1s
Screenshot stream
Type "{"
Wait /paused/
Sleep 600ms
Screenshot paused
Type "}"
Wait /following/
Sleep 400ms
Type "?"
Wait /About Noctiluca/
Sleep 800ms
Screenshot about
Escape
Sleep 300ms
Type "a"
Wait /faithful/
Sleep 800ms
Screenshot faithful
Type "q"
Wait /❯\s*$/
