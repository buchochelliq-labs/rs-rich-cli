# Noctiluca on a video watch page (0.0.23): the player's poster, then the
# video playing in its region, the channel's avatar beside its name,
# controls as [ buttons ], a grid of thumbnails with their titles under
# them side by side, and a compact comment list with small avatars. All of
# it is drawn by Noctiluca and intuiTUIve, the pictures by rs-rich-art.
# The tape job has no browser it may run with its sandbox on, so this
# replays what Chrome supplied for the fixture page (NOCTILUCA_FAKE_ENGINE,
# built with the test-only `fake-engine` feature): its saved snapshot, the
# page's own picture files, and Chrome's recorded frames of the video. The
# live tests run against Chrome in CI's browser job.
Set Title "Noctiluca · a video page"
Set Size 100x40
Set TypingDelay 40ms
Mask /\d:\d\d \/ \d:\d\d/ "<time>"
Hide
Type "export NOCTILUCA_FAKE_ENGINE=$REPO/crates/noctiluca/tests/snapshots XDG_CONFIG_HOME=$PWD/config; clear"
Enter
Wait /❯\s*$/
Show
Type "noctiluca --images sixel https://fixture.example/media-grid.html"
Enter
Wait /^(?![\s\S]*░)[\s\S]*PAUSED/ 60s
Sleep 1s
Screenshot watch
Type "p"
Wait /PLAYING/
Sleep 3s
Screenshot playing
Type "G"
Wait /^(?![\s\S]*░)[\s\S]*Harbour time-lapse/ 60s
Sleep 2s
Screenshot grid
Type "q"
Wait /❯\s*$/
