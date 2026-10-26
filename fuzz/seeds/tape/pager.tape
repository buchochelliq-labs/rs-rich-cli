# rich view pages a file through less: scroll and search with real keys.
Set Title "rich view --pager · scroll and search"
Set Size 100x24
Set Env PAGER "less"
Set Env LESS "-R"
# A frozen copy, so editing the real file does not change this recording.
Exec "cp $REPO/docs/tapes/fixtures/rule.rs rule.rs"
Type "rich view --pager rule.rs"
Enter
Wait "Port of upstream"
Sleep 800ms
Screenshot top
PageDown
Sleep 800ms
Screenshot scrolled
Type "/"
Sleep 300ms
Type "characters"
Sleep 300ms
Enter
Wait "characters"
Sleep 800ms
Screenshot search
Type "q"
Wait /❯\s*$/
