# Noctiluca on Hacker News (live): the front page, a table of tables,
# in the projection, and its link hints.
# A live site: it changes every day, so CI never runs or checks this tape
# (its tape job checks only docs/tapes/*.tape). Re-record it by hand, as
# docs/guide/noctiluca/real-sites.md says: as a normal user, with Chrome
# on PATH, from the repository root:
#
#   target/release/rich record --bin-dir target/release \
#     --output docs/media/tapes/live docs/tapes/live/real-hackernews.tape
Set Title "Noctiluca · Hacker News"
Set Size 120x40
Set TypingDelay 40ms
Set Timeout 90s
Output png
Type "noctiluca https://news.ycombinator.com"
Enter
Wait /points by/
Sleep 3s
Screenshot hackernews
Type "f"
Wait /type a label/
Sleep 1s
Screenshot hackernews-hints
Escape
Sleep 300ms
Type "q"
Wait /❯\s*$/
