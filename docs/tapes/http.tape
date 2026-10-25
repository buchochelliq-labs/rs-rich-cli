# A Slumber-style HTTP client rebuilt on intuiTUIve (examples/http.rs),
# over the collection docs/tapes/fixtures/http/slumber.yml, its requests
# answered by Python's http.server on a free port of 127.0.0.1 (no
# network): the recipe's query parameters, the request sent and its JSON
# body, the body filtered by a JSONPath (`/`), and the profiles (`p`). The
# port and the request's time are masked.
Set Title "http · Slumber on intuiTUIve"
Set Size 100x30
Set TypingDelay 40ms
Output gif png svg
# The time's digits change the line's width, so the mask also takes the
# padding up to the panel's border: "Nms", one space, then the border.
Mask /\d+ms ([^│║]*?) +([│║])/ "Nms ${1} ${2}"
Mask /\d+ms / "Nms "
Mask /127\.0\.0\.1:\d+/ "127.0.0.1:PORT"
Hide
Exec "mkdir -p bin && ln -sf \"$(dirname \"$(command -v rich)\")/examples/http\" bin/http"
Exec "cp -r $REPO/docs/tapes/fixtures/http/. ."
Exec "port=$(python3 -c 'import socket; s = socket.socket(); s.bind((\"127.0.0.1\", 0)); print(s.getsockname()[1])') && sed -i \"s/8765/$port/\" slumber.yml && (timeout 120 python3 -m http.server $port --bind 127.0.0.1 --directory api >/dev/null 2>&1 & echo $! > server.pid)"
Type "export PATH=\"$PWD/bin:$PATH\"; clear"
Enter
Wait /❯\s*$/
Show
Type "http slumber.yml"
Enter
Wait /List Fish/
Sleep 800ms
Right
Wait /say/
Sleep 800ms
Screenshot recipe
Enter
Wait /200 OK/
Wait /"Barry"/
Sleep 1s
Screenshot response
Type "2"
Sleep 300ms
Type "/"
Sleep 300ms
Type "$.fish[*].name"
Enter
Wait /"Wanda"/
Sleep 1s
Screenshot filtered
Escape
Sleep 300ms
Type "p"
Wait /Staging/
Sleep 800ms
Screenshot profiles
Escape
Sleep 300ms
Type "q"
Wait /❯\s*$/
Exec "kill $(cat server.pid)"
