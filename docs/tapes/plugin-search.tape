# rich plugin search, info and browse against a fixture marketplace
# (docs/tapes/fixtures/marketplace: the official listings and three
# examples, signed with a throwaway key that only this tape trusts). The
# tier shows as `✓ official`, `approved` or `community`, a maintainers'
# pick as `★ recommended`, and each package says who authored it.
Set Title "rich plugin search · find a plugin, pack or theme"
Set Size 104x30
Set TypingDelay 35ms
Hide
Type "export RICH_MARKETPLACE=$REPO/docs/tapes/fixtures/marketplace RICH_MARKETPLACE_KEYS=$REPO/docs/tapes/fixtures/marketplace/keys/keys-1.json RICH_PLUGIN_DIR=$PWD/plugins; clear"
Enter
Wait /❯\s*$/
Show
Type "rich plugin search --kind theme"
Enter
Wait /solarized/
Wait /❯\s*$/
Sleep 800ms
Screenshot themes
Hide
Type "clear"
Enter
Wait /❯\s*$/
Show
Type "rich plugin search --recommended"
Enter
Wait /textcase/
Wait /❯\s*$/
Sleep 800ms
Screenshot recommended
Hide
Type "clear"
Enter
Wait /❯\s*$/
Show
Type "rich plugin info shout"
Enter
Wait /Yanked/
Wait /❯\s*$/
Sleep 800ms
Screenshot info
Hide
Type "clear"
Enter
Wait /❯\s*$/
Show
Type "rich plugin browse"
Enter
Wait /Publisher/
Sleep 800ms
Screenshot browse
Type "rot13"
Wait /wordplay/
Sleep 800ms
Screenshot filter
Escape
Wait /❯\s*$/
Sleep 500ms
