# Noctiluca on BBC News (live): the front page in the projection with its
# pictures as Kitty graphics, then an article in reader mode.
# A live site: it changes every day, so CI never runs or checks this tape
# (its tape job checks only docs/tapes/*.tape). Re-record it by hand, as
# docs/guide/noctiluca/real-sites.md says: as a normal user, with Chrome
# on PATH, from the repository root:
#
#   target/release/rich record --bin-dir target/release \
#     --output docs/media/tapes/live docs/tapes/live/real-bbc.tape
Set Title "Noctiluca · BBC News"
Set Size 120x40
Set TypingDelay 40ms
Set Timeout 90s
Output png
Type "noctiluca --images kitty https://www.bbc.com/news"
Enter
Wait /BBC/
Sleep 12s
Screenshot bbc-front
Space
Sleep 10s
Screenshot bbc-front-more
Type "q"
Wait /❯\s*$/
Type "noctiluca --reader --images kitty https://www.bbc.com/news/articles/ckm23y3mg1yyo"
Enter
Wait /Lula/
Sleep 10s
Screenshot bbc-article
Space
Sleep 6s
Screenshot bbc-article-more
Type "q"
Wait /❯\s*$/
