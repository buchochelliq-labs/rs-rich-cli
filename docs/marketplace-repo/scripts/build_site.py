#!/usr/bin/env python3
"""Assemble the marketplace's web site: site/ plus the signed index.

This file belongs to the rs-rich-marketplace repository (its home in
rs-rich-cli is docs/marketplace-repo/, a copy to hand over). The pages
workflow (.github/workflows/pages.yml) runs it, and so does the site's
check (scripts/check_site.mjs, through site-check.yml).

    python3 scripts/build_site.py --output _site

The output is what GitHub Pages serves at
https://buchochelliq-labs.github.io/rs-rich-marketplace/:

- everything in site/ (the search page, its script, styles and fonts);
- index.json and index.json.minisig as committed, and keys/;
- for each listing the index names, packages/ID.json as the index signed
  it: the working tree's file when its SHA-256 is the index's, else the
  newest committed version of it that is. A listing merged after the last
  signing is in the working tree but not yet in the index, so the site
  keeps serving the signed one until the signer commits a new index;
- packages/index.html, a plain list of those listings with links to their
  JSON, for a browser without JavaScript (the search page's <noscript>).

A listing no committed version of which matches the index is an error:
the page would leave it out, and the run says so instead. Nothing here
signs or checks a signature; `rich plugin install` does that.

Only the standard library and git are needed.
"""

from __future__ import annotations

import argparse
import hashlib
import html
import json
import re
import shutil
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
ID = re.compile(r"^[a-z0-9][a-z0-9._-]{0,63}$")


def sha256(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def git(root: Path, *args: str) -> bytes:
    return subprocess.run(["git", "-C", str(root), *args], check=True, capture_output=True).stdout


def committed_versions(root: Path, path: str):
    """Every committed version of `path`, newest first, as bytes."""
    try:
        revisions = git(root, "log", "--format=%H", "--", path).decode().split()
    except subprocess.CalledProcessError:
        return
    for revision in revisions:
        try:
            yield git(root, "show", f"{revision}:{path}")
        except subprocess.CalledProcessError:
            continue  # the commit that deleted it


def signed_listing(root: Path, path: str, expected: str, history=committed_versions):
    """The bytes of `path` whose SHA-256 is `expected`, or None."""
    current = root / path
    if current.is_file():
        data = current.read_bytes()
        if sha256(data) == expected:
            return data
    for data in history(root, path):
        if sha256(data) == expected:
            return data
    return None


TIERS = {"official": "Official", "reviewed": "Approved", "community": "Community"}
KINDS = {"crate": "Crate", "wasm-plugin": "WASM plugin", "micro-pack": "Micro pack", "theme": "Theme"}


def packages_page(listings: list[dict], csp: str) -> str:
    """packages/index.html: every listing, as escaped text and links."""
    rows = []
    for listing in listings:
        listing_id = listing["id"]
        name = html.escape(str(listing.get("name") or listing_id))
        tier = TIERS.get(listing.get("tier"), "Community")
        if listing.get("tier") == "official":
            tier = "✓ Official · by rs-rich"
        kind = html.escape(KINDS.get(listing.get("kind"), str(listing.get("kind"))))
        description = html.escape(str(listing.get("description") or ""))
        rows.append(
            f'<li><a href="{html.escape(listing_id)}.json">{name}</a> '
            f"<code>{html.escape(listing_id)}</code> ({html.escape(tier)}, {kind}): {description}</li>"
        )
    return f"""<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<meta http-equiv="Content-Security-Policy" content="{html.escape(csp)}">
<meta name="referrer" content="no-referrer">
<meta name="color-scheme" content="light dark">
<title>Listings · rs-rich marketplace</title>
<link rel="icon" href="../favicon.svg" type="image/svg+xml">
<link rel="stylesheet" href="../assets/site.css">
</head>
<body>
<header class="site-header">
  <div class="wrap site-header-inner">
    <a class="brand" href="../"><img src="../assets/mark.svg" width="32" height="32" alt=""><span>rs-rich <span class="brand-sub">marketplace</span></span></a>
  </div>
</header>
<main id="main" class="wrap prose">
<h1>Every listing</h1>
<p>The listings the signed <a href="../index.json">index</a> names, as JSON.
<a href="../">The search page</a> filters and sorts them;
<code>rich plugin search</code> lists them in a terminal, and
<code>rich plugin install</code> verifies the index's signature before it
installs anything.</p>
<ul class="static-list">
{chr(10).join(rows)}
</ul>
</main>
</body>
</html>
"""


def page_csp(root: Path) -> str:
    """The search page's Content-Security-Policy, for the pages written here."""
    match = re.search(r'http-equiv="Content-Security-Policy" content="([^"]+)"', (root / "site" / "index.html").read_text(encoding="utf-8"))
    return match.group(1) if match else "default-src 'none'; style-src 'self'; img-src 'self'; font-src 'self'"


def build(root: Path, output: Path, history=committed_versions) -> list[str]:
    """Write the site to `output`; return the problems (empty when none)."""
    if output.exists():
        shutil.rmtree(output)
    shutil.copytree(root / "site", output)
    problems = []
    index_path = root / "index.json"
    if not index_path.is_file():
        # Before the first signing the page says the index is being signed.
        return problems
    index_bytes = index_path.read_bytes()
    index = json.loads(index_bytes)
    (output / "index.json").write_bytes(index_bytes)
    signature = root / "index.json.minisig"
    if signature.is_file():
        shutil.copyfile(signature, output / "index.json.minisig")
    else:
        problems.append("index.json has no index.json.minisig")
    if (root / "keys").is_dir():
        shutil.copytree(root / "keys", output / "keys")
    (output / "packages").mkdir(exist_ok=True)
    served = []
    for listing_id, entry in sorted(index.get("listings", {}).items()):
        path = entry.get("path") if isinstance(entry, dict) else None
        expected = entry.get("sha256") if isinstance(entry, dict) else None
        if not ID.match(listing_id) or path != f"packages/{listing_id}.json" or not isinstance(expected, str):
            problems.append(f"{listing_id}: not an index entry this site serves")
            continue
        data = signed_listing(root, path, expected, history)
        if data is None:
            problems.append(f"{path}: no committed version has the index's SHA-256 {expected}")
            continue
        (output / path).write_bytes(data)
        try:
            listing = json.loads(data)
        except ValueError:
            problems.append(f"{path}: not JSON")
            continue
        if isinstance(listing, dict) and listing.get("id") == listing_id:
            served.append(listing)
    served.sort(key=lambda l: (l.get("tier") != "official", str(l.get("name") or l["id"]).lower()))
    (output / "packages" / "index.html").write_text(packages_page(served, page_csp(root)), encoding="utf-8")
    return problems


def main(argv=None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("--root", type=Path, default=ROOT, help="the marketplace checkout (default: this one)")
    parser.add_argument("--output", type=Path, required=True, help="the directory to write (replaced)")
    args = parser.parse_args(argv)
    problems = build(args.root.resolve(), args.output.resolve())
    for problem in problems:
        print(f"::error::{problem}", file=sys.stderr)
    if problems:
        return 1
    served = sorted(p.name for p in (args.output / "packages").glob("*.json")) if (args.output / "packages").is_dir() else []
    print(f"site written to {args.output}: {len(served)} listing(s)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
