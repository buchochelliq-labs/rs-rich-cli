# rich deps --features: which features a crate has enabled, what each
# turns on, and who asked for them (from saved `cargo metadata` output, so
# the recording never changes).
Set Title "rich deps --features · who turned that on"
Set Size 100x44
Set TypingDelay 40ms
Hide
Exec "cp $REPO/crates/rich-ext/tests/fixtures/supply-chain/cargo-metadata.json metadata.json"
Type "clear"
Enter
Wait /❯\s*$/
Show
Type "rich deps --metadata metadata.json --features --package syn"
Enter
Wait "thiserror-impl v1.0.64: default features"
Sleep 1500ms
Screenshot features
