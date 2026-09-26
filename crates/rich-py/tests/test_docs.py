"""Every example in docs/python/ (and its subfolders) runs, and prints what the page says.

A ```python block is run; the ```text block that directly follows it (with only
blank lines between) is its expected standard output. Blocks on a page share
one namespace, in order. After changing an example, refresh its output with

    python crates/rich-py/tests/test_docs.py --update
"""

from __future__ import annotations

import contextlib
import io
import os
import re
import sys
from pathlib import Path

import pytest

DOCS = Path(__file__).resolve().parents[3] / "docs" / "python"
FENCE = re.compile(r"^```(\w*)[^\n]*\n(.*?)^```[ \t]*$", re.MULTILINE | re.DOTALL)


def blocks(markdown: str):
    """(language, body, start, end) for every fenced block."""
    return [(m.group(1), m.group(2), m.start(2), m.end(2)) for m in FENCE.finditer(markdown)]


def examples(markdown: str):
    """(code, expected output or None, output span) per python block."""
    found = blocks(markdown)
    result = []
    for index, (language, body, _, end) in enumerate(found):
        if language != "python":
            continue
        following = found[index + 1] if index + 1 < len(found) else None
        expected, span = None, None
        if following and following[0] == "text":
            between = markdown[end : following[2]]
            # Only the closing fence, blank lines and the opening fence.
            if re.fullmatch(r"```[ \t]*\n\s*```text[^\n]*\n", between):
                expected, span = following[1], (following[2], following[3])
        result.append((body, expected, span))
    return result


def run(code: str, namespace: dict) -> str:
    out = io.StringIO()
    with contextlib.redirect_stdout(out):
        exec(compile(code, "<docs>", "exec"), namespace)
    return out.getvalue()


PAGES = sorted(DOCS.rglob("*.md"))


def name(page: Path) -> str:
    return page.relative_to(DOCS).as_posix()


def test_the_pages_exist():
    names = {name(page) for page in PAGES}
    assert {"index.md", "console.md", "text.md", "style.md", "table.md", "panel.md", "protocol.md"} <= names
    assert {"ext/index.md", "ext/diagnostics.md", "ext/data.md", "ext/diffs.md", "ext/layout.md"} <= names


@pytest.mark.parametrize("page", PAGES, ids=name)
def test_examples_print_what_the_page_shows(page):
    namespace: dict = {}
    for number, (code, expected, _) in enumerate(examples(page.read_text(encoding="utf-8")), 1):
        actual = run(code, namespace)
        if expected is not None:
            assert actual == expected, f"{name(page)}, example {number}"


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
            print(f"updated {len(edits)} output(s) in {name(page)}")


if __name__ == "__main__":
    if sys.argv[1:] != ["--update"]:
        sys.exit(__doc__)
    for variable in ("COLUMNS", "NO_COLOR", "FORCE_COLOR", "COLORTERM"):
        os.environ.pop(variable, None)
    update()
