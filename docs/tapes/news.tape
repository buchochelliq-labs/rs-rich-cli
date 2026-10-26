# Noctiluca on a news page (0.0.23), to the look of the proof of concept:
# page text as terminal text, real pictures (Sixel) where the page puts
# them with a caption under each, a video's poster with its play mark
# that then plays in its region, and an animated GIF further down. All of
# it is drawn by Noctiluca and intuiTUIve, the pictures by rs-rich-art.
# The tape job has no browser it may run with its sandbox on, so this
# replays what Chrome supplied for the fixture page (NOCTILUCA_FAKE_ENGINE,
# built with the test-only `fake-engine` feature): its saved snapshot, the
# page's own picture files, and Chrome's recorded frames of the video. The
# live tests run against Chrome in CI's browser job.
Set Title "Noctiluca · a news page"
Set Size 100x40
Set TypingDelay 40ms
Mask /\d:\d\d \/ \d:\d\d/ "<time>"
Hide
Type "export NOCTILUCA_FAKE_ENGINE=$REPO/crates/noctiluca/tests/snapshots XDG_CONFIG_HOME=$PWD/config; clear"
Enter
Wait /❯\s*$/
Show
Type "noctiluca --images sixel https://fixture.example/news.html"
Enter
Wait /^(?![\s\S]*░)[\s\S]*PAUSED/ 60s
Sleep 1s
Screenshot news
Type "p"
Wait /PLAYING/
Sleep 3s
Screenshot playing
Type "G"
Wait /^(?![\s\S]*░)[\s\S]*Why the tides/ 60s
Sleep 2s
Screenshot stories
Type "q"
Wait /❯\s*$/
