"""Write the Noctiluca tapes' pages and config into a directory.

The pages are Chrome's saved snapshots of the crate's fixture pages
(crates/noctiluca/tests/snapshots), which the binary built with the
test-only `fake-engine` feature replays when NOCTILUCA_FAKE_ENGINE names
their directory. The snapshots were taken of local files; here their
addresses become https://fixture.example/..., so a page has a host and the
tapes can show site keys and keywords. The config adds a keyword, `lamp`,
whose search opens the docs page.

    python3 make.py REPO OUT
"""

import pathlib
import sys

CONFIG = """\
# The Noctiluca tapes' config (docs/tapes/fixtures/noctiluca/make.py).
[shortcuts]
lamp = { search = "https://fixture.example/docs.html?q={}", home = "https://fixture.example/docs.html" }
"""


def main() -> None:
    repo, out = pathlib.Path(sys.argv[1]), pathlib.Path(sys.argv[2])
    pages = out / "pages"
    pages.mkdir(parents=True, exist_ok=True)
    for snapshot in sorted((repo / "crates/noctiluca/tests/snapshots").glob("*.json")):
        text = snapshot.read_text(encoding="utf-8")
        text = text.replace("file:///noctiluca/fixtures/", "https://fixture.example/")
        (pages / snapshot.name).write_text(text, encoding="utf-8")
    config = out / "config" / "noctiluca"
    config.mkdir(parents=True, exist_ok=True)
    (config / "config.toml").write_text(CONFIG, encoding="utf-8")


if __name__ == "__main__":
    main()
