# Noctiluca on a live stream page (0.0.23): a role=log chat beside the
# stream, its authors in their colours, scrolled up and back, the about
# page's terminal adaptations, and the faithful projection one key away.
# WIP: recorded before the video and the log's small pictures landed (the
# stream's region and the emotes show as what they are for now); it is
# recorded again once they have. The tape job has no browser it may run
# with its sandbox on, so this replays Chrome's saved snapshot of the
# fixture page (NOCTILUCA_FAKE_ENGINE, built with the test-only
# `fake-engine` feature): the log is as Chrome read it, not live. The
# live tests run against Chrome in CI's browser job.
Set Title "Noctiluca · a live log beside a stream"
Set Size 110x32
Set TypingDelay 40ms
Hide
Type "export NOCTILUCA_FAKE_ENGINE=$REPO/crates/noctiluca/tests/snapshots XDG_CONFIG_HOME=$PWD/config; clear"
Enter
Wait /❯\s*$/
Show
Type "noctiluca --images ansi https://fixture.example/stream-with-log.html"
Enter
Wait /hello from the harbour/
Sleep 1s
Screenshot stream
Type "["
Wait /paused/
Sleep 600ms
Screenshot paused
Type "]"
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
