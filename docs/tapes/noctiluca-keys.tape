# Going places with Noctiluca (0.0.22): a keyword typed in the address
# bar (`lamp`, from the tape's config) opening its search, then a key
# bound on the site from inside the app (Ctrl+K: a link chosen by its role
# and name, `n` pressed, the clash with find's next match shown, Enter),
# the new key followed, the site's shortcuts (K) and the F1 overlay with
# the site's key first. It replays Chrome's saved snapshots of the crate's
# fixture pages (docs/tapes/fixtures/noctiluca/make.py gives them
# https://fixture.example/ addresses, so a page has a site), which needs
# the binary built with the test-only `fake-engine` feature.
Set Title "Noctiluca · keywords and site keys"
Set Size 100x30
Set TypingDelay 40ms
Hide
Exec "python3 $REPO/docs/tapes/fixtures/noctiluca/make.py $REPO ."
Type "export NOCTILUCA_FAKE_ENGINE=$PWD/pages XDG_CONFIG_HOME=$PWD/config; clear"
Enter
Wait /❯\s*$/
Show
Type "noctiluca --reader https://fixture.example/reader.html"
Enter
Wait /Tiny drifters of the open sea/
Sleep 800ms
Type "o"
Wait /Open: an address/
Sleep 300ms
Type "lamp shine"
Sleep 800ms
Screenshot keyword
Enter
Wait /A lamp that glows/
Sleep 1s
Screenshot keyword-result
Type "H"
Wait /Tiny drifters/
Sleep 600ms
Ctrl+K
Wait /to what/
Sleep 800ms
Screenshot bind-picker
Down
Sleep 300ms
Enter
Wait /Press the key/
Sleep 300ms
Type "n"
Wait /already does/
Sleep 1s
Screenshot bind-confirm
Enter
Wait /now does/
Sleep 1500ms
Type "n"
Wait /You followed the link/
Sleep 800ms
Type "H"
Wait /Tiny drifters/
Sleep 600ms
Type "K"
Wait /Site shortcuts/
Sleep 800ms
Screenshot site-keys
Escape
Sleep 300ms
F1
Wait /Link hints/
Sleep 1s
Screenshot actions-site
Escape
Sleep 300ms
Type "q"
Wait /❯\s*$/
