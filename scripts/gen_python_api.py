#!/usr/bin/env python3
"""Generate the Python docs site's API reference pages (docs/python/api/).

One page per public `rs_rich` module, each a mkdocstrings `:::` directive that
renders the module's `__all__` from the type stubs (`_native.pyi`) and the
Python modules under crates/rich-py/python, so no compiled wheel is needed.

    python scripts/gen_python_api.py          # write the pages
    python scripts/gen_python_api.py --check  # fail if they are out of date
"""
import argparse
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
PACKAGE = ROOT / "crates" / "rich-py" / "python" / "rs_rich"
OUT = ROOT / "docs" / "python" / "api"
# Not API: the entry point and the private native module.
SKIP = {"__main__", "_native"}


def modules():
    """(dotted name, title) for every public module, packages first."""
    found = []
    for path in sorted(PACKAGE.rglob("*.py")):
        rel = path.relative_to(PACKAGE.parent).with_suffix("")
        parts = list(rel.parts)
        if parts[-1] == "__init__":
            parts = parts[:-1]
        if any(part in SKIP or part.startswith("_") for part in parts[1:]):
            continue
        found.append(".".join(parts))
    return found


def page(name):
    return (
        f"# `{name}`\n\n"
        "Generated from the type stubs by `scripts/gen_python_api.py`; do not edit.\n\n"
        f"::: {name}\n"
    )


def expected():
    pages = {}
    names = modules()
    for name in names:
        pages[OUT / (name.replace(".", "/") + ".md")] = page(name)
    index = ["# API reference\n",
             "Every public module of `rs_rich`, with the signatures from its type stubs.",
             "The guides in the other sections explain how to use them.\n"]
    for name in names:
        index.append(f"- [`{name}`]({name.replace('.', '/')}.md)")
    pages[OUT / "index.md"] = "\n".join(index) + "\n"
    return pages


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args()
    pages = expected()
    stale = [path for path, text in pages.items()
             if not path.exists() or path.read_text(encoding="utf-8") != text]
    extra = [path for path in OUT.rglob("*.md") if path not in pages] if OUT.exists() else []
    if args.check:
        if stale or extra:
            for path in stale + extra:
                print(f"out of date: {path.relative_to(ROOT)}")
            print("run: python scripts/gen_python_api.py")
            return 1
        print(f"{len(pages)} API pages up to date")
        return 0
    for path in extra:
        path.unlink()
    for path, text in pages.items():
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(text, encoding="utf-8")
    print(f"wrote {len(pages)} API pages")
    return 0


if __name__ == "__main__":
    sys.exit(main())
