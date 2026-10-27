# Noctiluca on YouTube (live): a watch page (Blender Foundation's Big Buck
# Bunny) and the about page's codec list. When YouTube answers with its
# "confirm you're not a bot" wall, as it did here, the tape shows that:
# Noctiluca never tries to get past one. Where the page plays, add a
# `Type "p"` and a screenshot to show the video in place.
# A live site: it changes every day, so CI never runs or checks this tape
# (its tape job checks only docs/tapes/*.tape). Re-record it by hand, as
# docs/guide/noctiluca/real-sites.md says: as a normal user, with Chrome
# on PATH, from the repository root:
#
#   target/release/rich record --bin-dir target/release \
#     --output docs/media/tapes/live docs/tapes/live/real-youtube.tape
Set Title "Noctiluca · YouTube"
Set Size 120x40
Set TypingDelay 40ms
Set Timeout 90s
Output png
Type "noctiluca --images kitty https://www.youtube.com/watch?v=aqz-KE-bpKQ"
Enter
Wait /NOCTILUCA/
Sleep 15s
Screenshot youtube-watch
Type "?"
Wait /Codecs/
Sleep 1s
Screenshot youtube-codecs
Escape
Sleep 500ms
Type "q"
Wait /❯\s*$/
