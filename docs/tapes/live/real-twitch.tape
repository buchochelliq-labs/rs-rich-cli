# Noctiluca on Twitch (live): the category directory in half blocks, then
# a live channel (the Astronomiemuseum's meteor radar, on the air around
# the clock) in Kitty graphics, its stream playing in place: Twitch starts
# it muted, so no key is pressed.
# A live site: it changes every day, so CI never runs or checks this tape
# (its tape job checks only docs/tapes/*.tape). Re-record it by hand, as
# docs/guide/noctiluca/real-sites.md says: as a normal user, with Chrome
# on PATH, from the repository root:
#
#   target/release/rich record --bin-dir target/release \
#     --output docs/media/tapes/live docs/tapes/live/real-twitch.tape
Set Title "Noctiluca · Twitch"
Set Size 120x40
Set TypingDelay 40ms
Set Timeout 90s
Output png gif
Type "noctiluca --images ansi https://www.twitch.tv/directory"
Enter
Wait /All Categories/
Sleep 15s
Screenshot twitch-directory
Type "q"
Wait /❯\s*$/
Type "noctiluca --images kitty https://www.twitch.tv/astronomiemuseum"
Enter
Wait /NOCTILUCA/
Wait /PLAYING/
Sleep 12s
Screenshot twitch-channel
Type "q"
Wait /❯\s*$/
