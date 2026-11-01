#!/usr/bin/env python3
"""Check that every relative URL in a built docs site points at a file in it.

    python scripts/check_site_links.py site [site/python ...]

`zensical build --strict` checks the Markdown links and their anchors, but
not the raw HTML in a page (a `<video>`'s `<source>`, an `<img>` in a
`<figure>`, the parity chart's links). Zensical rebases every `href` and
`src` in a page from the source file to the built page, raw HTML included,
so a raw URL is written as a Markdown link's would be: relative to the
`.md` file (`../media/tapes/x/x.gif` from `docs/guide/x.md`), not to the
page's URL as MkDocs needed. `poster` is not rebased and stays relative to
the page's URL. This walks every built page and fails on a relative `href`,
`src`, `poster` or `data` that resolves to nothing in the site.
"""
import posixpath
import re
import sys
from pathlib import Path
from urllib.parse import unquote

ATTR = re.compile(
    r'<(?:a|img|source|video|audio|track|script|link|iframe|object|embed)\b[^>]*?'
    r'\s(?:href|src|poster|data)="([^"]*)"',
    re.I,
)
# Absolute URLs, other schemes, fragments, root-relative paths and template
# placeholders are not files of this site.
SKIP = re.compile(r"^([a-z][a-z0-9+.-]*:|//|#|/|\{)", re.I)


def dead(root):
    found = []
    for page in sorted(root.rglob("*.html")):
        rel = page.relative_to(root).as_posix()
        for url in ATTR.findall(page.read_text(encoding="utf-8")):
            if not url or SKIP.match(url):
                continue
            path = unquote(re.split(r"[?#]", url, maxsplit=1)[0])
            if not path:
                continue
            target = posixpath.normpath(posixpath.join(posixpath.dirname(rel), path))
            hit = root / target
            if target.startswith("..") or not (hit.is_file() or (hit / "index.html").is_file()):
                found.append(f"{rel}: {url}")
    return found


def main():
    roots = [Path(arg) for arg in sys.argv[1:]] or [Path("site")]
    failed = []
    for root in roots:
        if not (root / "index.html").is_file():
            sys.exit(f"{root} is not a built site (no index.html)")
        failed += [f"{root}/{entry}" for entry in dead(root)]
    if failed:
        print(f"{len(failed)} relative URL(s) point at nothing:\n  " + "\n  ".join(failed[:50]))
        sys.exit(1)
    print("every relative URL resolves")


if __name__ == "__main__":
    main()
