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
  and SHA-256 must equal the listing's. An artifact this repository hosts
  is read from the checkout instead, since it is served only once merged:
  a URL under its `main`
  (`https://raw.githubusercontent.com/buchochelliq-labs/rs-rich-marketplace/main/PATH`,
  read from `PATH`) or a release asset of it
  (`https://github.com/buchochelliq-labs/rs-rich-marketplace/releases/download/ID-vVERSION/ID-VERSION.EXT`,
  read from `artifacts/ID/ID-VERSION.EXT`; the tag and the file name must
  match the listing's id and version). Every such version is checked on
  every run, so a committed artifact cannot change under a listing.
- **A crate is crates.io's.** Its hash must equal the checksum in
  crates.io's index, and the version must not be yanked there unless the
  listing yanks it too.
- **The tier.** A listing that is new with a tier other than `community`,
  or whose tier rises, is refused unless `--allow-tier` is given (the
  workflow passes it when a maintainer labels the pull request
  `tier-approved`). With `--all` there is no base, and no tier check.
- **Recommended.** Adding, changing or removing a listing's
  `recommended` (`{"since": DATE, "note": TEXT}`, shown as ★ Recommended)
  is refused unless `--allow-recommended` is given (the workflow passes
  it when a maintainer labels the pull request `recommended-approved`, a
  separate label from `tier-approved`). With `--all` there is no base, and
  no check.
- **Names that pass for ours.** A listing that is not `official`, when it
  is new or its id, name or description changes, may not:
  - use a reserved id: one starting with `rs-rich`, `rich-`, `richcli` or
    `official` (ignoring `-`, `_` and `.`, except in `rich-`), or `rich` or
    `rs`;
  - say "official" or "rs-rich" in its name, or "official" in its
    description, in any case and ignoring separators ("by rs-rich" too);
  - look like an official package's id: after dropping `-`, `_` and `.`,
    equal to one, or (for an official id of 4 characters or more) one
    insertion, deletion or substitution away (`textcase2`, `text-case`,
    `wordp1ay`);
  - use anything but ASCII in its id (`textcаse` with a Cyrillic а).
  `--allow-names` lifts these (the workflow passes it when a maintainer
  labels the pull request `name-approved`), for genuine cases such as an
  "unofficial" port.

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
RESERVED_PREFIXES = ("rsrich", "richcli", "official")
RESERVED_IDS = {"rich", "rs"}
SEPARATORS = "-_."


def squeeze(text: str) -> str:
    """Lowercase, letters and digits only: what a name says, whatever its
    case and separators."""
    return "".join(c for c in text.lower() if c.isalnum())


def plain_id(listing_id: str) -> str:
    return "".join(c for c in listing_id.lower() if c not in SEPARATORS)


def within_one_edit(a: str, b: str) -> bool:
    """Whether `a` becomes `b` with at most one insertion, deletion or
    substitution."""
    if abs(len(a) - len(b)) > 1:
        return False
    if len(a) > len(b):
        a, b = b, a
    i = j = edits = 0
    while i < len(a) and j < len(b):
        if a[i] == b[j]:
            i += 1
            j += 1
            continue
        edits += 1
        if edits > 1:
            return False
        if len(a) == len(b):
            i += 1
        j += 1
    return edits + (len(b) - j) + (len(a) - i) <= 1


def name_problems(listing: dict, official_ids) -> list:
    """Why a listing that is not official could pass for one of ours."""
    problems = []
    listing_id = str(listing.get("id", ""))
    if not listing_id.isascii():
        problems.append(f"the id {listing_id!r} must be ASCII")
    plain = plain_id(listing_id)
    if (plain in RESERVED_IDS or plain.startswith(RESERVED_PREFIXES)
            or listing_id.lower().startswith("rich-")):
        problems.append(f"the id {listing_id!r} is reserved for rs-rich's own packages "
                        "(rs-rich*, rich-*, richcli*, official*, rich, rs)")
    for official in sorted(official_ids):
        if official == listing_id:
            continue
        target = plain_id(official)
        if plain == target or (len(target) >= 4 and within_one_edit(plain, target)):
            problems.append(f"the id {listing_id!r} looks like the official package "
                            f"{official!r}")
    name = squeeze(str(listing.get("name", "")))
    for word in ("official", "rsrich"):
        if word in name:
            problems.append(f"the name {listing.get('name')!r} says {word!r}, which only "
                            "rs-rich's own packages may")
    if "official" in squeeze(str(listing.get("description", ""))):
        problems.append("the description says 'official', which only rs-rich's own packages may")
    return problems


def official_ids(root: Path, base: str | None) -> set:
    """Every official listing's id, at `base` and in the checkout."""
    ids = set()
    for path in (root / "packages").glob("*.json"):
        try:
            listing = json.loads(path.read_bytes())
        except ValueError:
            continue
        if listing.get("tier") == "official":
            ids.add(listing.get("id"))
    if base is not None:
        names = subprocess.run(["git", "-C", str(root), "ls-tree", "--name-only", base,
                                "packages/"], capture_output=True, text=True).stdout.split()
        for relative in names:
            old = at_base(root, base, relative)
            if old and old.get("tier") == "official":
                ids.add(old.get("id"))
    return {i for i in ids if isinstance(i, str)}
MUTABLE = {"yanked", "yank_reason"}


def identity(listing: dict) -> tuple:
    """What a listed version's artifact and checksums belong to: the id, the
    kind and, for a crate, the crate's name. It never changes once a version
    is listed, or the versions already checked would be credited to another
    package without being checked again."""
    crate = listing.get("crate")
    return (listing.get("id"), listing.get("kind"),
            crate.get("name") if isinstance(crate, dict) else None)


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
    def __init__(self, root: Path, fetcher=common.fetch, crate_entry=common.crate_index_entry,
                 allow_names: bool = False, allow_recommended: bool = False):
        self.root = root
        self.allow_names = allow_names
        self.allow_recommended = allow_recommended
        self.official_ids = None
        self.fetch = fetcher
        self.crate_entry = crate_entry
        self.errors: list[str] = []
        self.checked: list[tuple[dict, dict, bytes]] = []

    def error(self, message: str):
        self.errors.append(message)

    def read_artifact(self, listing_id: str, version: str, url: str, size: int) -> bytes:
        relative = common.hosted_path(listing_id, version, url)
        if relative is not None:
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

        if (base is not None and not self.allow_recommended
                and (old or {}).get("recommended") != listing.get("recommended")):
            change = ("added" if "recommended" not in (old or {}) else
                      "removed" if "recommended" not in listing else "changed")
            self.error(f"{name}: `recommended` {change} needs a maintainer's "
                       "`recommended-approved` label")

        if listing.get("tier") != "official" and not self.allow_names:
            identity_changed = old is None or any(
                old.get(key) != listing.get(key) for key in ("id", "name", "description"))
            if identity_changed:
                if self.official_ids is None:
                    self.official_ids = official_ids(self.root, base)
                for problem in name_problems(listing, self.official_ids):
                    self.error(f"{name}: {problem}; a maintainer's `name-approved` label "
                               "allows it")

        old_versions = {v["version"]: v for v in old.get("versions", [])} if old else {}
        if old_versions and identity(old) != identity(listing):
            was, now = identity(old), identity(listing)
            self.error(f"{name}: a listing's id, kind and crate name never change once a "
                       f"version is listed (was {was}, now {now}); add a new listing instead")
            return
        for version in listing.get("versions", []):
            number = version["version"]
            previous = old_versions.get(number)
            if previous is not None and frozen(previous) != frozen(version):
                self.error(f"{name} {number}: a listed version never changes "
                           "(only yanked and yank_reason may be added)")
                continue
            url = version["artifact"]["url"]
            local = url.startswith((common.RAW_MAIN, common.RELEASES))
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
            data = self.read_artifact(listing["id"], version["version"], artifact["url"],
                                      artifact["size"])
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
    parser.add_argument("--allow-recommended", action="store_true",
                        help="a maintainer approved adding, changing or removing `recommended`")
    parser.add_argument("--allow-names", action="store_true",
                        help="a maintainer approved a name that looks like rs-rich's own")
    parser.add_argument("--mirror", type=Path, help="write each checked artifact to DIR/HOST/PATH")
    parser.add_argument("--installs", type=Path,
                        help="write ID@VERSION for each checked installable version")
    args = parser.parse_args(argv)

    checker = Checker(args.root, allow_names=args.allow_names,
                      allow_recommended=args.allow_recommended)
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
