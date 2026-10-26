#!/usr/bin/env python3
"""Make the release of every hosted version that has none yet.

    python3 scripts/publish_releases.py --target "$GITHUB_SHA"
    python3 scripts/publish_releases.py --dry-run

An artifact this repository hosts is a release asset:
`https://github.com/buchochelliq-labs/rs-rich-marketplace/releases/download/ID-vVERSION/ID-VERSION.EXT`,
one release per package version. The listing's pull request carries the
file as `artifacts/ID/ID-VERSION.EXT`, and check_artifacts.py checks it
there. After the merge, publish-artifacts.yml runs this: for every listed
version with such a URL,

- the file in the checkout must still have the listing's size and SHA-256;
- the release `ID-vVERSION` is made if it is missing, at `--target` (the
  merge commit), never marked latest;
- the file is uploaded if the release has no asset of that name;
- the asset is read back through the API, and its size and SHA-256 must
  equal the listing's.

It is idempotent: a release and asset already there are only checked. An
existing asset is never replaced or deleted; one that does not match is an
error for the maintainer, and the install of that version fails its hash
check until it is fixed by hand.

Versions on the old raw URLs are left alone. Only the standard library is
needed; the token is the job's `GITHUB_TOKEN` (`contents: write`).
"""

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import github_api  # noqa: E402
import marketplace_common as common  # noqa: E402

ROOT = Path(__file__).resolve().parent.parent


def hosted_versions(root: Path):
    """`(listing, version, tag, name, path)` for every version that is a
    release asset of this repository."""
    for path in sorted((root / "packages").glob("*.json")):
        listing = json.loads(path.read_bytes())
        for version in listing.get("versions", []):
            url = version["artifact"]["url"]
            asset = common.release_asset(url)
            if asset is None:
                continue
            relative = common.hosted_path(listing["id"], version["version"], url)
            yield listing, version, asset[0], asset[1], root / relative


def release_body(listing: dict, version: dict) -> str:
    artifact = version["artifact"]
    lines = [
        f"{listing['name']} {version['version']} ({listing['kind']}), "
        "served by the rs-rich plugin marketplace.",
        "",
        f"Install it with `rich plugin install {listing['id']}@{version['version']}`, "
        "which checks the file against the signed index.",
        "",
        f"- SHA-256: `{artifact['sha256']}`",
        f"- Size: {artifact['size']} bytes",
        f"- Listing: `packages/{listing['id']}.json`",
    ]
    if version.get("source_revision"):
        lines.append(f"- Source: {listing['repository']} at `{version['source_revision']}`")
    return "\n".join(lines) + "\n"


def check_bytes(label: str, data: bytes, artifact: dict) -> str | None:
    if len(data) != artifact["size"]:
        return f"{label} is {len(data)} bytes, listed as {artifact['size']}"
    digest = common.sha256(data)
    if digest != artifact["sha256"]:
        return f"{label} hashes to {digest}, listed as {artifact['sha256']}"
    return None


def publish(github, root: Path, target: str | None, dry_run: bool, out=print) -> list:
    errors = []
    for listing, version, tag, name, path in hosted_versions(root):
        label = f"{listing['id']} {version['version']}"
        artifact = version["artifact"]
        if not path.is_file():
            errors.append(f"{label}: {path.relative_to(root)} is not in the checkout")
            continue
        data = path.read_bytes()
        problem = check_bytes(str(path.relative_to(root)), data, artifact)
        if problem:
            errors.append(f"{label}: {problem}")
            continue
        release = github.release_by_tag(tag)
        if release is None:
            if dry_run:
                out(f"would create release {tag} and upload {name}")
                continue
            if not target:
                errors.append(f"{label}: no --target commit to tag")
                continue
            release = github.create_release(tag, target, f"{listing['id']} {version['version']}",
                                            release_body(listing, version))
            out(f"created release {tag}")
        assets = [a for a in release.get("assets", []) if a.get("name") == name]
        if not assets:
            if dry_run:
                out(f"would upload {name} to {tag}")
                continue
            assets = [github.upload_asset(release, name, data)]
            out(f"uploaded {name} to {tag}")
        asset = assets[0]
        if asset.get("size") != artifact["size"]:
            errors.append(f"{label}: the release asset {tag}/{name} is {asset.get('size')} bytes, "
                          f"listed as {artifact['size']}; it is never replaced, so fix it by hand")
            continue
        problem = check_bytes(f"the release asset {tag}/{name}", github.download_asset(asset),
                              artifact)
        if problem:
            errors.append(f"{label}: {problem}; it is never replaced, so fix it by hand")
            continue
        out(f"ok: {tag}/{name}, {artifact['size']} bytes, sha256 {artifact['sha256']}")
    return errors


def main(argv=None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--root", type=Path, default=ROOT)
    parser.add_argument("--target", help="the commit a new release tags (the merge commit)")
    parser.add_argument("--dry-run", action="store_true", help="change nothing; say what is missing")
    args = parser.parse_args(argv)
    errors = publish(github_api.GitHub(), args.root, args.target, args.dry_run)
    for message in errors:
        print(f"::error::{message}")
    return 1 if errors else 0


if __name__ == "__main__":
    sys.exit(main())
