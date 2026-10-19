# A termusic-style music player rebuilt on intuiTUIve (examples/music.rs),
# over a library of short silent WAV files written by
# docs/tapes/fixtures/music/make.py, with lyrics beside one track and a
# cover in its folder: the library tree, both albums added to the
# playlist (`L`), a track played (`l`), paused, sent back to its start
# (`0`) and five seconds on (`f`), so the gauge and the lyrics stand
# still at 00:05, then the help (Ctrl+H) and the question before
# quitting. There is no sound: the example's play head runs on the app's
# clock.
Set Title "music · termusic on intuiTUIve"
Set Size 110x30
Set TypingDelay 40ms
Output gif png svg
Hide
Exec "mkdir -p bin && ln -sf \"$(dirname \"$(command -v rich)\")/examples/music\" bin/music"
Exec "python3 $REPO/docs/tapes/fixtures/music/make.py library && cp $REPO/crates/rich-micro/builtin/fun/coffee/static.png 'library/Orbit Lights/cover.png'"
Type "export PATH=\"$PWD/bin:$PATH\"; clear"
Enter
Wait /❯\s*$/
Show
Type "music library"
Enter
Wait /Harbour Songs/
Sleep 500ms
Down 2
Right
Wait /Perigee/
Sleep 800ms
Screenshot library
Type "L"
Wait /3 tracks added/
Sleep 500ms
Up
Type "L"
Wait /Total 5 tracks/
Sleep 3500ms
Tab
Sleep 300ms
Down
Sleep 300ms
Type "l"
Wait /Lyrics of Nova Hale/
Sleep 1500ms
Space
Wait /Status: Paused/
Type "0"
Sleep 300ms
Type "f"
Wait /00:05/
Sleep 1s
Screenshot playing
Ctrl+H
Wait /Loop mode cycle/
Sleep 800ms
Screenshot help
Escape
Sleep 300ms
Type "q"
Wait /quit\?/
Sleep 500ms
Screenshot quit
Type "y"
Wait /❯\s*$/
