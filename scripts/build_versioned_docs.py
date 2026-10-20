#!/usr/bin/env python3
"""Build the documentation site with one copy of the docs per release.

Each release's docs (a release is named by rs-rich-cli's version; see
release_cohort.py) are built into `<out>/X.Y.Z/` from the last commit on main
that still ships that release: the commit `docs-versions.toml` pins for it,
else its `rs-rich-cli-vX.Y.Z` tag or a later commit up to the one before
`RELEASES.toml` first moves a crate past it. Docs fixed after a release so show
under it, and the docs of the next release's work do not. Main's docs are built into `<out>/dev/`. The theme's version selector reads `versions.json`
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

The docs playground (docs/design/docs-playground.md) needs each version's own
wasm module in `<version>/assets/playground/`. Main's is built by docs.yml into
docs/assets/playground/ before this runs, so it is copied with main's docs. A
release's is the `playground-X.Y.Z.tar.gz` the release workflow attached to its
GitHub release; it is downloaded for a version whose docs have the playground's
script. A release without one keeps plain code blocks: the script finds no
`playground.json` and offers no button.

    python scripts/build_versioned_docs.py                 # dev + newest releases
    python scripts/build_versioned_docs.py --releases 1    # a quick check
    python scripts/build_versioned_docs.py --no-playground # no downloads
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
import tomllib
import urllib.error
import urllib.request
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
OVERRIDES = ROOT / "scripts" / "docs_overrides"
TAG = re.compile(r"rs-rich-cli-v(\d+)\.(\d+)\.(\d+)")
WRAPPER = ".mkdocs-versioned.yml"
PINS = "docs-versions.toml"


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


def versions_at(commit):
    """RELEASES.toml's (name, registry) -> version at `commit`, or None where it
    has none."""
    shown = subprocess.run(["git", "show", f"{commit}:RELEASES.toml"], cwd=ROOT,
                           capture_output=True, text=True, encoding="utf-8")
    if shown.returncode:
        return None
    packages = tomllib.loads(shown.stdout).get("package", [])
    return {(p["name"], p.get("registry")): p["version"] for p in packages}


def pinned(version):
    """The commit `docs-versions.toml` names for `version`'s docs, or None."""
    path = ROOT / PINS
    if not path.exists():
        return None
    return tomllib.loads(path.read_text(encoding="utf-8")).get("docs", {}).get(version)


def docs_commit(version, tag):
    """The commit whose docs stand for release `version`, tagged `tag`.

    The commit `docs-versions.toml` pins for it, when that is behind HEAD: the
    commit that opens the next release's versions records there the last
    commit of this release's docs (its notes come before the version change).
    Otherwise the last commit on HEAD's first-parent line before RELEASES.toml
    moves a version past the tag's, else HEAD. The tag itself when it is not
    behind HEAD or has no RELEASES.toml to compare."""
    pin = pinned(version)
    if pin and not subprocess.run(["git", "merge-base", "--is-ancestor", pin, "HEAD"], cwd=ROOT,
                                  capture_output=True).returncode:
        return git("rev-parse", pin).strip()
    shipped = versions_at(tag)
    ancestor = subprocess.run(["git", "merge-base", "--is-ancestor", tag, "HEAD"], cwd=ROOT)
    if shipped is None or ancestor.returncode:
        return tag
    changes = git("log", "--first-parent", "--reverse", "--format=%H", f"{tag}..HEAD",
                  "--", "RELEASES.toml").split()
    for commit in changes:
        if versions_at(commit) != shipped:
            return git("rev-parse", f"{commit}^1").strip()
    return git("rev-parse", "HEAD").strip()


def site_root():
    """The published site's URL, from mkdocs.yml, ending in a slash."""
    text = (ROOT / "mkdocs.yml").read_text(encoding="utf-8")
    url = re.search(r"^site_url:\s*(\S+)", text, re.M)[1]
    return url if url.endswith("/") else url + "/"


def extract(commit, into):
    """The tree at `commit`, written to `into` (snippets read crates/ too)."""
    archive = subprocess.run(["git", "archive", "--format=tar", commit], cwd=ROOT, check=True,
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


def repo_url():
    """The repository's URL, from mkdocs.yml, without a trailing slash."""
    text = (ROOT / "mkdocs.yml").read_text(encoding="utf-8")
    return re.search(r"^repo_url:\s*(\S+)", text, re.M)[1].rstrip("/")


# The largest playground archive accepted (the modules are about 5 MB).
PLAYGROUND_MAX_BYTES = 40_000_000


def playground_url(repo, version):
    """Where the release workflow attaches release `version`'s module."""
    return f"{repo}/releases/download/release-{version}/playground-{version}.tar.gz"


def fetch(url):
    """The bytes at `url`, or None when there is nothing there (404)."""
    try:
        with urllib.request.urlopen(url, timeout=60) as response:
            data = response.read(PLAYGROUND_MAX_BYTES + 1)
    except urllib.error.HTTPError as error:
        if error.code == 404:
            return None
        raise
    if len(data) > PLAYGROUND_MAX_BYTES:
        raise ValueError(f"{url} is over {PLAYGROUND_MAX_BYTES} bytes")
    return data


def add_playground(site, version, repo, fetch=fetch):
    """Put release `version`'s playground module into its built `site`.

    Only when the site has the playground's script (older releases' docs do
    not), and only when the release has the archive. Returns whether it was
    added. A download that fails for another reason than a 404 leaves the
    version without the playground, with a warning, rather than failing the
    whole site."""
    if not (site / "assets" / "playground.js").exists():
        return False
    url = playground_url(repo, version)
    try:
        data = fetch(url)
    except (OSError, ValueError) as error:
        print(f"  playground for {version} not downloaded ({error}); its blocks stay plain",
              flush=True)
        return False
    if data is None:
        print(f"  no playground for {version} (nothing at {url})", flush=True)
        return False
    target = site / "assets" / "playground"
    if target.exists():
        shutil.rmtree(target)
    target.mkdir(parents=True)
    try:
        with tarfile.open(fileobj=io.BytesIO(data), mode="r:gz") as tar:
            # `data`: no absolute paths, no `..`, no links out, no devices.
            tar.extractall(target, filter="data")
        stamp = json.loads((target / "playground.json").read_text(encoding="utf-8"))
        if stamp.get("format") != 1 or "lean" not in stamp.get("modules", {}):
            raise ValueError("not a playground stamp")
    except (tarfile.TarError, OSError, ValueError) as error:
        shutil.rmtree(target)
        print(f"  playground for {version} rejected ({error}); its blocks stay plain", flush=True)
        return False
    print(f"  playground for {version} added", flush=True)
    return True


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
    parser.add_argument("--no-playground", action="store_true",
                        help="do not download the releases' playground modules")
    args = parser.parse_args()

    url = site_root()
    repo = repo_url()
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
            commit = docs_commit(version, tag)
            print(f"building {version} from {commit[:12]} ({tag})", flush=True)
            source = Path(scratch) / version
            extract(commit, source)
            # An old release's docs are as they were then: warnings do not fail them.
            build(source, out / version, version, url, strict=False)
            shutil.rmtree(source)
            if not args.no_playground:
                add_playground(out / version, version, repo)
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
