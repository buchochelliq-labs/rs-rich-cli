# rich diff --conflicts: a file's merge conflicts, ours, base and theirs
# side by side with the lines around them. The file is the fixture
# docs/tapes/fixtures/data/merge.rs.
Set Title "rich diff --conflicts · merge conflicts, side by side"
Set Size 100x28
Set TypingDelay 40ms
Hide
Exec "cp $REPO/docs/tapes/fixtures/data/merge.rs ."
Type "clear"
Enter
Wait /❯\s*$/
Show
Type "rich diff --conflicts merge.rs"
Enter
Wait "merge.rs: 2 conflicts (1 with a base)"
Sleep 1500ms
Screenshot conflicts
