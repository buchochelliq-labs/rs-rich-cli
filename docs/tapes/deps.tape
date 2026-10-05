# rich deps: a Cargo dependency tree (from saved `cargo metadata` output, so
# the recording never changes), what pulls a crate in, and the graph.
Set Title "rich deps · what depends on what"
Set Size 100x30
Set TypingDelay 40ms
Hide
Exec "cp $REPO/crates/rich-ext/tests/fixtures/sources/cargo-metadata.json metadata.json"
Type "clear"
Enter
Wait /❯\s*$/
Show
Type "rich deps --metadata metadata.json --depth 2"
Enter
Wait "duplicate: syn"
Sleep 1s
Screenshot tree
Hide
Type "clear"
Enter
Show
Type "rich deps --metadata metadata.json --why syn"
Enter
Wait "thiserror-impl"
Sleep 1s
Screenshot why
Hide
Type "clear"
Enter
Show
Type "rich deps --metadata metadata.json --graph --depth 1"
Enter
Wait "serde v1.0.210"
Sleep 1500ms
Screenshot graph
