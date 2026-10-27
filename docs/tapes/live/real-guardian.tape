# Noctiluca on The Guardian (live): the international front page in the
# projection, its pictures in Sixel, then in reader mode.
# A live site: it changes every day, so CI never runs or checks this tape
# (its tape job checks only docs/tapes/*.tape). Re-record it by hand, as
# docs/guide/noctiluca/real-sites.md says: as a normal user, with Chrome
# on PATH, from the repository root:
#
#   target/release/rich record --bin-dir target/release \
#     --output docs/media/tapes/live docs/tapes/live/real-guardian.tape
Set Title "Noctiluca · The Guardian"
Set Size 120x40
Set TypingDelay 40ms
Set Timeout 90s
Output png
Type "noctiluca --images sixel https://www.theguardian.com/international"
Enter
Wait /Guardian/
Sleep 8s
Space
Sleep 10s
Screenshot guardian-front
Type "r"
Wait /READER/
Sleep 4s
Space
Sleep 8s
Screenshot guardian-reader
Type "q"
Wait /❯\s*$/
