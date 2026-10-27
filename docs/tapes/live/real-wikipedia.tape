# Noctiluca on Wikipedia (live): the article on Noctiluca scintillans in
# the projection with Kitty graphics, link hints, and reader mode with the
# article's photograph.
# A live site: it changes every day, so CI never runs or checks this tape
# (its tape job checks only docs/tapes/*.tape). Re-record it by hand, as
# docs/guide/noctiluca/real-sites.md says: as a normal user, with Chrome
# on PATH, from the repository root:
#
#   target/release/rich record --bin-dir target/release \
#     --output docs/media/tapes/live docs/tapes/live/real-wikipedia.tape
Set Title "Noctiluca · Wikipedia"
Set Size 120x40
Set TypingDelay 40ms
Set Timeout 90s
Output png gif
Type "noctiluca --images kitty https://en.wikipedia.org/wiki/Noctiluca_scintillans"
Enter
Wait /Noctiluca - Wikipedia/
Sleep 8s
Screenshot wikipedia-projection
Type "f"
Wait /type a label/
Sleep 1s
Screenshot wikipedia-hints
Escape
Sleep 500ms
Type "r"
Wait /READER/
Sleep 6s
Type "/"
Wait /Find in page/
Type "There are 2 colours"
Enter
Sleep 2s
Type "n"
Sleep 8s
Screenshot wikipedia-reader
Type "q"
Wait /❯\s*$/
