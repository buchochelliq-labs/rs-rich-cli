"""The pictures on docs/python/logging.md: run it, or scripts/capture_python_docs.py."""

from _shots import Shots

import logging

from rs_rich.logging import RichHandler


def handler(console):
    # --8<-- [start:handler]
    handler = RichHandler(console=console, show_time=False, show_path=False)
    log = logging.getLogger("docs.example")
    log.propagate = False
    log.addHandler(handler)
    log.setLevel(logging.INFO)
    log.info("Listening on port %d", 8080)
    log.warning("GET /index.html took %.1f seconds", 2.5)
    log.error("Lost the connection to %s", "db.internal")
    # --8<-- [end:handler]


shots = Shots("logging")
shots.shot("handler", handler, width=70)
shots.finish()
