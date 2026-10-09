"""The counter, served to a web browser (rs-rich-web).

    python crates/rich-py/examples/tui/serve.py

Prints a URL with a token: open it, and each browser tab gets its own
counter, built and run on that session's own thread. Ctrl+C stops the
server. It listens on this computer only (127.0.0.1); to reach it from
another machine, put it behind a reverse proxy with TLS and authentication.
"""

from rs_rich.tui import serve

from counter import counter_app

if __name__ == "__main__":
    serve("127.0.0.1:8080", counter_app)
