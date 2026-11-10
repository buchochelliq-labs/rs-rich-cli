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
# Keep decoder-dependent frame pixels out of the deterministic text grid,
# in the player's rows only (from under the page's address to the video's
# title): the thumbnails, avatars and the rest stay checked, and the PNG
# and SVG screenshots still capture the actual image.
Mask /(?s)(Search videos \[_\][^\n]*\n).*?(\n The bloom from the air)/ "${1}<player rows>${2}"
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
# Capture the first playing frame. Waiting several seconds made this text
# screenshot depend on the decoded video's playback clock on each runner.
Screenshot playing
# Freeze the decoder's frame before switching to the gallery so the grid
# capture cannot race video playback.
Type "p"
Wait /PAUSED/
Type "G"
Wait /^(?![\s\S]*░)[\s\S]*Harbour time-lapse/ 60s
Screenshot grid
Type "q"
Wait /❯\s*$/
