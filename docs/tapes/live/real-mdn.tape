# Noctiluca on MDN (live): a JavaScript reference page in the projection
# with Sixel pictures, then its syntax and code in reader mode.
# A live site: it changes every day, so CI never runs or checks this tape
# (its tape job checks only docs/tapes/*.tape). Re-record it by hand, as
# docs/guide/noctiluca/real-sites.md says: as a normal user, with Chrome
# on PATH, from the repository root:
#
#   target/release/rich record --bin-dir target/release \
#     --output docs/media/tapes/live docs/tapes/live/real-mdn.tape
Set Title "Noctiluca · MDN"
Set Size 120x40
Set TypingDelay 40ms
Set Timeout 90s
Output png
Type "noctiluca --images sixel https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Array/map"
Enter
Wait /Array.prototype.map/
Sleep 6s
Screenshot mdn-projection
Type "r"
Wait /READER/
Sleep 2s
Type "/"
Wait /Find in page/
Type "const numbers"
Enter
Sleep 3s
Screenshot mdn-reader
Type "q"
Wait /❯\s*$/
