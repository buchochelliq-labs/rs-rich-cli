"""Regenerate manifest-version tables, or fail CI if they are stale."""

import argparse
from pathlib import Path
import re
import tomllib

ROOT = Path(__file__).resolve().parent.parent
START = "<!-- BEGIN MANIFEST VERSIONS -->"
END = "<!-- END MANIFEST VERSIONS -->"


def table(root):
    workspace = tomllib.loads((root / "Cargo.toml").read_text())
    lines = [START, "| Package | Manifest version |", "|---|---|"]
    for member in workspace["workspace"]["members"]:
        package = tomllib.loads((root / member / "Cargo.toml").read_text())["package"]
        name, version = package["name"], package["version"]
        lines.append(f"| [`{name}`](https://crates.io/crates/{name}) | `{version}` |")
    return "\n".join([*lines, END])


def update(root, check=False):
    rendered = table(root)
    stale = []
    for path in [root / "README.md", root / "docs/index.md"]:
        old = path.read_text(encoding="utf-8")
        if old.count(START) != 1 or old.count(END) != 1:
            raise ValueError(f"{path}: expected one manifest-version table")
        new, count = re.subn(re.escape(START) + r".*?" + re.escape(END),
                            lambda _: rendered, old, flags=re.S)
        if count != 1:
            raise ValueError(f"{path}: invalid manifest-version markers")
        if old != new:
            stale.append(str(path.relative_to(root)))
            if not check:
                path.write_text(new, encoding="utf-8")
    if check and stale:
        print("Stale manifest versions: " + ", ".join(stale))
        print("Run python3 scripts/gen_versions.py")
        return 1
    print("Manifest-version tables match Cargo.toml files")
    return 0


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true")
    raise SystemExit(update(ROOT, parser.parse_args().check))
