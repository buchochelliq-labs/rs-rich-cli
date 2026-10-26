#!/usr/bin/env python3
"""Check the artifacts of new and changed listing versions.

    python3 scripts/check_artifacts.py --base origin/main
    python3 scripts/check_artifacts.py --all --mirror MARKET/artifacts --installs installs.txt

The pull request check runs this after the listings validate. For every
version that `--base` (a Git revision) does not list, or lists differently
(`--all`: every version):

- **A version never changes.** One listed at `--base` may only gain
  `yanked` and `yank_reason`; anything else is refused.
- **The artifact matches.** It is read at most `size` bytes, and its size
  and SHA-256 must equal the listing's. A URL under this repository's
  `main` (`https://raw.githubusercontent.com/buchochelliq-labs/rs-rich-marketplace/main/`)
  is read from the checkout instead, since it is served only once merged;
  every such version is checked on every run, so a committed artifact
  cannot change under a listing.
- **A crate is crates.io's.** Its hash must equal the checksum in
  crates.io's index, and the version must not be yanked there unless the
  listing yanks it too.
- **The tier.** A listing that is new with a tier other than `community`,
  or whose tier rises, is refused unless `--allow-tier` is given (the
  workflow passes it when a maintainer labels the pull request
  `tier-approved`). With `--all` there is no base, and no tier check.

`--mirror DIR` writes each checked artifact to `DIR/HOST/PATH`, the layout a
local marketplace serves to `rich plugin install`; `--installs FILE` writes
`ID@VERSION` for each checked WASM plugin, micro pack and theme, one per
line, for the workflow to install against the host.

Only the standard library and git are needed.
"""

from __future__ import annotations

import argparse
import json
import subprocess
import sys
from pathlib import Path
from urllib.parse import urlsplit

sys.path.insert(0, str(Path(__file__).resolve().parent))

import marketplace_common as common  # noqa: E402

ROOT = Path(__file__).resolve().parent.parent
TIERS = {"community": 0, "reviewed": 1, "official": 2}
MUTABLE = {"yanked", "yank_reason"}


def at_base(root: Path, base: str | None, relative: str):
    """The listing at `base`, or None when it is not there."""
    if base is None:
        return None
    result = subprocess.run(["git", "-C", str(root), "show", f"{base}:{relative}"],
                            capture_output=True)
    if result.returncode != 0:
        return None
    return json.loads(result.stdout)


def frozen(version: dict) -> dict:
    return {k: v for k, v in version.items() if k not in MUTABLE}


class Checker:
    def __init__(self, root: Path, fetcher=common.fetch, crate_entry=common.crate_index_entry):
        self.root = root
        self.fetch = fetcher
        self.crate_entry = crate_entry
        self.errors: list[str] = []
        self.checked: list[tuple[dict, dict, bytes]] = []

    def error(self, message: str):
        self.errors.append(message)

    def read_artifact(self, url: str, size: int) -> bytes:
        if url.startswith(common.RAW_MAIN):
            relative = url[len(common.RAW_MAIN):]
            path = (self.root / relative).resolve()
            if self.root.resolve() not in path.parents:
                raise ValueError(f"{url}: outside the repository")
            if not path.is_file():
                raise ValueError(f"{url}: {relative} is not in this branch")
            data = path.read_bytes()
        else:
            data = self.fetch(url, size)
        return data

    def check_listing(self, path: Path, base: str | None, all_versions: bool, allow_tier: bool):
        relative = path.relative_to(self.root).as_posix()
        listing = json.loads(path.read_bytes())
        old = at_base(self.root, base, relative)
        name = listing.get("id", path.stem)

        old_tier = TIERS.get(old.get("tier"), 0) if old else 0
        new_tier = TIERS.get(listing.get("tier"), 0)
        if base is not None and new_tier > old_tier and not allow_tier:
            self.error(f"{name}: tier {listing.get('tier')!r} "
                       f"({'raised' if old else 'set'}) needs a maintainer's `tier-approved` label")

        old_versions = {v["version"]: v for v in old.get("versions", [])} if old else {}
        for version in listing.get("versions", []):
            number = version["version"]
            previous = old_versions.get(number)
            if previous is not None and frozen(previous) != frozen(version):
                self.error(f"{name} {number}: a listed version never changes "
                           "(only yanked and yank_reason may be added)")
                continue
            local = version["artifact"]["url"].startswith(common.RAW_MAIN)
            if not (all_versions or previous is None or local):
                continue
            self.check_version(listing, version)
        for number in old_versions:
            if all(v["version"] != number for v in listing.get("versions", [])):
                self.error(f"{name} {number}: a listed version is never removed; yank it instead")

    def check_version(self, listing: dict, version: dict):
        name = f"{listing['id']} {version['version']}"
        artifact = version["artifact"]
        try:
            data = self.read_artifact(artifact["url"], artifact["size"])
        except Exception as error:  # noqa: BLE001 - any failure is reported
            self.error(f"{name}: cannot read {artifact['url']}: {error}")
            return
        if len(data) != artifact["size"]:
            self.error(f"{name}: {artifact['url']} is {len(data)} bytes, listed as {artifact['size']}")
            return
        digest = common.sha256(data)
        if digest != artifact["sha256"]:
            self.error(f"{name}: {artifact['url']} hashes to {digest}, listed as {artifact['sha256']}")
            return
        if listing["kind"] == "crate":
            crate = listing["crate"]["name"]
            try:
                record = self.crate_entry(crate, version["version"])
            except Exception as error:  # noqa: BLE001
                self.error(f"{name}: crates.io: {error}")
                return
            if record.get("cksum") != digest:
                self.error(f"{name}: crates.io's index records {record.get('cksum')}, "
                           f"the listing {digest}")
                return
            if record.get("yanked") and not version.get("yanked"):
                self.error(f"{name}: {crate} {version['version']} is yanked on crates.io; "
                           "yank it in the listing too")
                return
        print(f"ok: {name} ({listing['kind']}), {len(data)} bytes, sha256 {digest}")
        self.checked.append((listing, version, data))


def main(argv=None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--root", type=Path, default=ROOT)
    group = parser.add_mutually_exclusive_group(required=True)
    group.add_argument("--base", help="the Git revision the pull request merges into")
    group.add_argument("--all", action="store_true", help="check every version")
    parser.add_argument("--allow-tier", action="store_true",
                        help="a maintainer approved setting or raising a tier")
    parser.add_argument("--mirror", type=Path, help="write each checked artifact to DIR/HOST/PATH")
    parser.add_argument("--installs", type=Path,
                        help="write ID@VERSION for each checked installable version")
    args = parser.parse_args(argv)

    checker = Checker(args.root)
    for path in sorted((args.root / "packages").glob("*.json")):
        try:
            checker.check_listing(path, args.base, args.all, args.allow_tier)
        except (ValueError, KeyError, TypeError) as error:
            checker.error(f"{path.name}: {error}")
    if args.base:
        for relative in subprocess.run(
                ["git", "-C", str(args.root), "ls-tree", "--name-only", args.base, "packages/"],
                capture_output=True, text=True).stdout.split():
            if not (args.root / relative).exists():
                checker.error(f"{relative}: a listing is never removed; yank its versions instead")

    installs = []
    for listing, version, data in checker.checked:
        if args.mirror:
            url = urlsplit(version["artifact"]["url"])
            target = args.mirror / url.hostname / url.path.lstrip("/")
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_bytes(data)
        if listing["kind"] != "crate" and not version.get("yanked"):
            installs.append(f"{listing['id']}@{version['version']}")
    if args.installs:
        args.installs.write_text("".join(f"{line}\n" for line in installs), encoding="utf-8")

    for message in checker.errors:
        print(f"::error::{message}" if not sys.stdout.isatty() else f"error: {message}")
    print(f"{len(checker.checked)} version(s) checked, {len(checker.errors)} error(s)")
    return 1 if checker.errors else 0


if __name__ == "__main__":
    sys.exit(main())
