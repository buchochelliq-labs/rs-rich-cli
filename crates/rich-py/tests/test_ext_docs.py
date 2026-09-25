"""Every example in docs/python/ext/ runs, and prints what the page says.

The same rules as test_docs.py, for the rs_rich.ext pages. After changing an
example, refresh its output with

    python crates/rich-py/tests/test_ext_docs.py --update
"""

from __future__ import annotations

import os
import sys

import pytest
from test_docs import DOCS, examples, run

PAGES = sorted((DOCS / "ext").glob("*.md"))


def test_the_pages_exist():
    names = {page.name for page in PAGES}
    assert {"index.md", "diagnostics.md", "data.md", "diffs.md", "layout.md", "cli-authoring.md"} <= names


@pytest.mark.parametrize("page", PAGES, ids=lambda p: p.name)
def test_examples_print_what_the_page_shows(page):
    namespace: dict = {}
    for number, (code, expected, _) in enumerate(examples(page.read_text(encoding="utf-8")), 1):
        actual = run(code, namespace)
        if expected is not None:
            assert actual == expected, f"{page.name}, example {number}"


def update() -> None:
    for page in PAGES:
        markdown = page.read_text(encoding="utf-8")
        namespace: dict = {}
        edits = []
        for code, expected, span in examples(markdown):
            actual = run(code, namespace)
            if span is not None and actual != expected:
                edits.append((span, actual))
        for (start, end), actual in reversed(edits):
            markdown = markdown[:start] + actual + markdown[end:]
        if edits:
            page.write_text(markdown, encoding="utf-8")
            print(f"updated {len(edits)} output(s) in {page.name}")


if __name__ == "__main__":
    if sys.argv[1:] != ["--update"]:
        sys.exit(__doc__)
    for name in ("COLUMNS", "NO_COLOR", "FORCE_COLOR", "COLORTERM"):
        os.environ.pop(name, None)
    update()
