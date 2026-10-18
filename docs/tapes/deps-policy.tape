# rich deps --sizes, --baseline and --policy: what each crate adds to a
# binary (cargo-bloat's JSON), a size regression against a saved baseline
# (exit 5), and a licence policy refusing a crate in an SBOM. Every input is
# a fixture in the repository, so the recording never changes.
Set Title "rich deps · sizes, baselines and licence policy"
Set Size 100x44
Set TypingDelay 40ms
Hide
Exec "cp $REPO/crates/rich-ext/tests/fixtures/deps/bloat.json bloat.json"
Exec "cp $REPO/crates/rich-ext/tests/fixtures/deps/bloat-baseline.json bloat-baseline.json"
Exec "cp $REPO/crates/rich-ext/tests/fixtures/deps/policy.toml policy.toml"
Exec "cp $REPO/crates/rich-ext/tests/fixtures/deps/sbom.cdx.json sbom.cdx.json"
Type "clear"
Enter
Wait /❯\s*$/
Show
Type "rich deps --sizes bloat.json --top 8"
Enter
Wait "and 9 more, 212.0 KiB in all"
Sleep 1500ms
Screenshot sizes
Hide
Type "clear"
Enter
Show
Type "rich deps --sizes bloat.json --baseline bloat-baseline.json --top 6"
Enter
Wait "regex_automata, the total"
Sleep 1500ms
Screenshot baseline
Hide
Type "clear"
Enter
Show
Type "rich deps --policy policy.toml --sbom sbom.cdx.json"
Enter
Wait "2 licence policy violations"
Sleep 1500ms
Screenshot policy
