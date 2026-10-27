# Noctiluca on GitHub (live): this repository's page, read in reader mode
# down to its README.
# A live site: it changes every day, so CI never runs or checks this tape
# (its tape job checks only docs/tapes/*.tape). Re-record it by hand, as
# docs/guide/noctiluca/real-sites.md says: as a normal user, with Chrome
# on PATH, from the repository root:
#
#   target/release/rich record --bin-dir target/release \
#     --output docs/media/tapes/live docs/tapes/live/real-github.tape
Set Title "Noctiluca · GitHub"
Set Size 120x40
Set TypingDelay 40ms
Set Timeout 90s
Output png
Type "noctiluca --reader --images kitty https://github.com/buchochelliq-labs/rs-rich-cli"
Enter
Wait /rs-rich-cli/
Sleep 8s
Type "/"
Wait /Find in page/
Type "Install and try"
Enter
Sleep 4s
Screenshot github-readme
Type "q"
Wait /❯\s*$/
