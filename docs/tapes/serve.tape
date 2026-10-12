# `rich serve` (0.0.18, the CLI's off-by-default `serve` feature; CI
# builds it for this tape) from the terminal's side: it serves the `todo`
# example, one copy per browser tab, and prints the address with its
# token. A tape cannot drive a browser, so curl asks what a browser would:
# the page without the token is refused, the page with it is served, and
# `session` (defined out of view) sends the WebSocket upgrade the page
# sends, from the server's own origin and then from another site. The
# first starts a session, the second is refused. Ctrl+C stops the server
# and every program it started. The token and the job's process id differ
# on every run, so they are masked in the text grids --check compares.
Set Title "rich serve · a program in a browser tab"
Set Size 90x16
Set TypingDelay 30ms
Mask /token=[0-9a-f]{32}/ "token=…"
Mask /\[1\] \d+/ "[1] …"
Hide
Exec "mkdir -p bin && ln -sf \"$(dirname \"$(command -v rich)\")/examples/todo\" bin/todo"
Type "export PATH=\"$PWD/bin:$PATH\"; session() { curl -si --max-time 2 -H 'Connection: Upgrade' -H 'Upgrade: websocket' -H 'Sec-WebSocket-Version: 13' -H 'Sec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==' -H \"Origin: $1\" \"${url/\\?/ws?}\" | head -1; }; clear"
Enter
Wait /❯\s*$/
Show
Type "rich serve -- todo | tee serve.log & sleep 1"
Enter
Wait /Serving todo at http/
Wait /❯\s*$/
Sleep 1s
Screenshot started
Type "url=$(grep -o 'http://[^ ]*' serve.log)"
Enter
Wait /❯\s*$/
Type "curl -s http://127.0.0.1:8080/"
Enter
Wait /with its token\./
Wait /❯\s*$/
Type "curl -s \"$url\" | grep -o '<title>.*</title>'"
Enter
Wait /<title>todo<\/title>/
Wait /❯\s*$/
Type "session http://127.0.0.1:8080"
Enter
Wait /101 Switching Protocols/
Wait /❯\s*$/
Type "session https://elsewhere.example"
Enter
Wait /403 Forbidden/
Wait /❯\s*$/
Sleep 1s
Screenshot sessions
Type "fg"
Enter
Wait /tee serve\.log\s*$/
Sleep 500ms
Ctrl+C
Wait /❯\s*$/
Sleep 1s
