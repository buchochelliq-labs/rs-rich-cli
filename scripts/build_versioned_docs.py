#!/usr/bin/env python3
"""Build the documentation site with one copy of the docs per release.

Each release's docs are built from its `rs-rich-cli-vX.Y.Z` tag (a release is
named by rs-rich-cli's version; see release_cohort.py) into `<out>/X.Y.Z/`, and
main's into `<out>/dev/`. The theme's version selector reads `versions.json`
at the site's root, the format `mike` writes, so no `gh-pages` branch is needed:
the whole site is rebuilt and deployed as one Pages artifact.

    <out>/versions.json   the selector's list, newest first
    <out>/index.html      redirects to the newest release
    <out>/404.html        sends an unversioned link (an old bookmark) there too
    <out>/latest/...      one redirect page per page of the newest release
    <out>/dev/...         main
    <out>/X.Y.Z/...       each release kept, with the Python site at python/

GitHub Pages takes at most 1 GB, so the oldest releases are left out until the
site fits under --budget-mb.

    python scripts/build_versioned_docs.py                 # dev + newest releases
    python scripts/build_versioned_docs.py --releases 1    # a quick check
"""
import argparse
import html
import io
import json
import re
import shutil
import subprocess
import sys
import tarfile
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
OVERRIDES = ROOT / "scripts" / "docs_overrides"
TAG = re.compile(r"rs-rich-cli-v(\d+)\.(\d+)\.(\d+)")
WRAPPER = ".mkdocs-versioned.yml"


def git(*args):
    return subprocess.run(["git", *args], cwd=ROOT, check=True, capture_output=True, text=True).stdout


def releases():
    """(version, tag) for every release tag that has a docs site, newest first."""
    found = []
    for tag in git("tag", "--list", "rs-rich-cli-v*").split():
        match = TAG.fullmatch(tag)
        if not match:
            continue
        if subprocess.run(["git", "cat-file", "-e", f"{tag}:mkdocs.yml"], cwd=ROOT,
                          capture_output=True).returncode:
            continue
        found.append((tuple(int(part) for part in match.groups()), tag))
    found.sort(reverse=True)
    return [(".".join(map(str, key)), tag) for key, tag in found]


def site_root():
    """The published site's URL, from mkdocs.yml, ending in a slash."""
    text = (ROOT / "mkdocs.yml").read_text(encoding="utf-8")
    url = re.search(r"^site_url:\s*(\S+)", text, re.M)[1]
    return url if url.endswith("/") else url + "/"


def extract(tag, into):
    """The tree at `tag`, written to `into` (snippets read crates/ too)."""
    archive = subprocess.run(["git", "archive", "--format=tar", tag], cwd=ROOT, check=True,
                             capture_output=True).stdout
    with tarfile.open(fileobj=io.BytesIO(archive)) as tar:
        tar.extractall(into, filter="data")


def build(source, out, version, url, strict):
    """One version's site: the main docs at `out`, the Python site at out/python."""
    wrapper = source / WRAPPER
    # INHERIT merges this over the version's own mkdocs.yml; the wrapper sits
    # beside it so the relative docs_dir resolves the same.
    wrapper.write_text(
        "INHERIT: mkdocs.yml\n"
        f"site_url: {url}{version}/\n"
        "theme:\n"
        f"  custom_dir: {json.dumps(str(OVERRIDES))}\n"
        "extra:\n"
        "  version:\n"
        "    provider: mike\n"
        "    default: latest\n"
        "    alias: true\n",
        encoding="utf-8",
    )
    try:
        command = [sys.executable, "-m", "mkdocs", "build", "-q", "-f", WRAPPER, "-d", str(out)]
        subprocess.run(command + (["--strict"] if strict else []), cwd=source, check=True)
    finally:
        wrapper.unlink()
    if (source / "mkdocs-python.yml").exists():
        command = [sys.executable, "-m", "mkdocs", "build", "-q", "-f", "mkdocs-python.yml",
                   "-d", str(out / "python")]
        subprocess.run(command + (["--strict"] if strict else []), cwd=source, check=True)


def size_mb(path):
    return sum(f.stat().st_size for f in path.rglob("*") if f.is_file()) / 1e6


def redirect(target):
    """A page that sends the reader to `target` (relative or absolute)."""
    target = html.escape(target, quote=True)
    return ("<!doctype html><meta charset=utf-8><title>Redirecting</title>"
            f'<meta http-equiv="refresh" content="0; url={target}">'
            f'<link rel="canonical" href="{target}">'
            f'<a href="{target}">Redirecting to {target}</a>\n')


def aliases(out, latest):
    """latest/<page> redirects to <latest>/<page>, keeping the query and anchor."""
    for page in (out / latest).rglob("*.html"):
        rel = page.relative_to(out / latest)
        stub = out / "latest" / rel
        stub.parent.mkdir(parents=True, exist_ok=True)
        up = "../" * (len(rel.parts))
        stub.write_text(redirect(f"{up}{latest}/{rel.as_posix()}"), encoding="utf-8")


def not_found(out, url, versions, latest):
    """A 404 page that moves an unversioned path under the newest release."""
    base = re.sub(r"^https?://[^/]+", "", url)
    known = json.dumps(sorted(set(versions) | {"latest"}))
    (out / "404.html").write_text(
        "<!doctype html><meta charset=utf-8><title>Not found</title>\n"
        "<script>\n"
        f"const base = {json.dumps(base)}, known = {known}, latest = {json.dumps(latest)};\n"
        "const path = location.pathname.startsWith(base) ? location.pathname.slice(base.length) : '';\n"
        "const first = path.split('/')[0];\n"
        "if (path && !known.includes(first)) {\n"
        "  location.replace(base + latest + '/' + path + location.search + location.hash);\n"
        "}\n"
        "</script>\n"
        f'<p>This page does not exist. <a href="{html.escape(base)}">Go to the documentation.</a></p>\n',
        encoding="utf-8",
    )


def main():
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("--out", type=Path, default=ROOT / "site")
    parser.add_argument("--releases", type=int, default=6, help="the most releases to keep")
    parser.add_argument("--budget-mb", type=float, default=950, help="the site's size limit")
    parser.add_argument("--no-dev", action="store_true", help="leave main's docs out")
    args = parser.parse_args()

    url = site_root()
    out = args.out.resolve()
    if out.exists():
        shutil.rmtree(out)
    out.mkdir(parents=True)

    if not args.no_dev:
        print("building dev (main)", flush=True)
        build(ROOT, out / "dev", "dev", url, strict=True)
        dev = [{"version": "dev", "title": "dev (main)", "aliases": []}]
    else:
        dev = []

    kept = []
    with tempfile.TemporaryDirectory() as scratch:
        for version, tag in releases()[: args.releases]:
            print(f"building {version} from {tag}", flush=True)
            source = Path(scratch) / version
            extract(tag, source)
            # An old tag's docs are as they were released: warnings do not fail them.
            build(source, out / version, version, url, strict=False)
            shutil.rmtree(source)
            if size_mb(out) > args.budget_mb:
                print(f"  {version} leaves the site over {args.budget_mb:.0f} MB; it and older "
                      "releases are left out", flush=True)
                shutil.rmtree(out / version)
                break
            kept.append(version)

    if kept:
        latest = kept[0]
        entries = [{"version": v, "title": v, "aliases": ["latest"] if v == latest else []}
                   for v in kept]
        aliases(out, latest)
        target = f"{latest}/"
    else:
        entries, latest, target = [], "dev", "dev/"
    # The selector lists them in this order: the releases, newest first, then main.
    entries += dev
    (out / "versions.json").write_text(json.dumps(entries, indent=2) + "\n", encoding="utf-8")
    (out / "index.html").write_text(redirect(target), encoding="utf-8")
    not_found(out, url, [e["version"] for e in entries], latest)
    (out / ".nojekyll").write_text("", encoding="utf-8")
    print(f"site: {', '.join(e['version'] for e in entries)}; {size_mb(out):.0f} MB at {out}")


if __name__ == "__main__":
    main()
