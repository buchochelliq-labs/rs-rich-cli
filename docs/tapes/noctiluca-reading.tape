# Reading with Noctiluca (0.0.22): a two-column page in the projection,
# an article with a header, a figure and an aside in the projection and
# in reader mode, find in page, and an audio element's controls (the
# replay has no playback, so they say it is not ready). It replays Chrome's saved snapshots
# of the crate's fixture pages (docs/tapes/fixtures/noctiluca/make.py),
# which needs the binary built with the test-only `fake-engine` feature.
# The replay has no real pictures: an image's still is a flat box.
Set Title "Noctiluca · reading a page"
Set Size 100x30
Set TypingDelay 40ms
Hide
Exec "python3 $REPO/docs/tapes/fixtures/noctiluca/make.py $REPO ."
Type "export NOCTILUCA_FAKE_ENGINE=$PWD/pages XDG_CONFIG_HOME=$PWD/config; clear"
Enter
Wait /❯\s*$/
Show
Type "noctiluca https://fixture.example/page.html"
Enter
Wait /Where it lives/
Sleep 1s
Screenshot two-columns
Type "o"
Wait /Open: an address/
Sleep 300ms
Type "https://fixture.example/article.html"
Enter
Wait /The glowing coast/
Sleep 1s
Screenshot article-projection
Type "r"
Wait /reader/
Sleep 1s
Screenshot article-reader
Type "/"
Wait /Find in page/
Sleep 300ms
Type "bloom"
Enter
Wait /match 1 of 2/
Sleep 800ms
Screenshot find
Type "n"
Wait /match 2 of 2/
Sleep 800ms
Type "o"
Wait /Open: an address/
Sleep 300ms
Type "https://fixture.example/audio.html"
Enter
Wait /A test tone/
Sleep 1s
Screenshot audio
Type "q"
Wait /❯\s*$/
