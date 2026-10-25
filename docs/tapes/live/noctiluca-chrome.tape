# Noctiluca against a real Chrome (0.0.22): the crate's fixture pages
# served from 127.0.0.1, read in the projection and in reader mode, a link
# followed by its hint, a form filled in and sent (the field committed,
# the check box ticked, the echo page reading the query back), and an
# article with its picture as Kitty graphics.
#
# CI does not run this tape: its tape job has no browser, and it checks
# only docs/tapes/*.tape. Re-record it by hand, as a normal user (Chrome's
# sandbox does not run as root), with a Chrome or Chromium on PATH under
# one of the names Noctiluca looks for (`chrome`, `chromium`,
# `google-chrome`, ...), from the repository's root:
#
#   cargo build -p rs-rich-cli -p noctiluca --bins
#   PATH=/path/to/chrome-dir:$PATH target/debug/rich record \
#     --bin-dir target/debug --output docs/media/tapes \
#     docs/tapes/live/noctiluca-chrome.tape
#
# A live page's timing varies, so its screenshots are not checked.
Set Title "Noctiluca · a real Chrome, local pages"
Set Size 100x30
Set TypingDelay 40ms
Set Timeout 30s
Hide
Type "python3 -m http.server 8765 --bind 127.0.0.1 -d $REPO/crates/noctiluca/tests/fixtures >/dev/null 2>&1 & SERVER=$!; sleep 1; clear"
Enter
Wait /❯\s*$/
Show
Type "noctiluca http://127.0.0.1:8765/page.html"
Enter
Wait /Where it lives/
Sleep 1500ms
Screenshot live-projection
Type "o"
Wait /Open: an address/
Type "127.0.0.1:8765/reader.html"
Enter
Wait /Tiny drifters/
Sleep 1s
Type "r"
Wait /reader/
Sleep 1s
Type "f"
Wait /type a label/
Sleep 600ms
Screenshot live-hints
Type "a"
Wait /You followed the link/
Sleep 1s
Type "H"
Wait /Tiny drifters/
Sleep 800ms
Type "o"
Wait /Open: an address/
Type "127.0.0.1:8765/form.html"
Enter
Wait /Schedule remote update/
Sleep 1s
Type "f"
Wait /type a label/
Sleep 400ms
Type "a"
Sleep 500ms
Type " Lovelace"
Wait /edited/
Sleep 800ms
Enter
Sleep 1500ms
Tab 2
Sleep 300ms
Space
Wait /\[[x✓✔■]\] Send news/
Sleep 1s
Screenshot live-form
Type "f"
Wait /type a label/
Sleep 400ms
Type "k"
Wait /Query:/
Sleep 1500ms
Screenshot live-echo
Type "q"
Wait /❯\s*$/
Type "noctiluca --reader --images kitty http://127.0.0.1:8765/article.html"
Enter
Wait /The glowing coast/
Sleep 2500ms
Screenshot live-image
Type "q"
Wait /❯\s*$/
Hide
Type "kill $SERVER"
Enter
