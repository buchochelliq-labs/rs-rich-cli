#!/usr/bin/env python3
"""Add a crates.io version to a crate's listing.

    python3 scripts/add_crate_version.py packages/mermaid.json 0.0.9
    python3 scripts/add_crate_version.py packages/dot.json 0.0.6 --published 2026-10-27

The crate is the listing's `crate.name`. The version must be on crates.io
and not yanked there. Its `.crate` file is downloaded from static.crates.io,
and its SHA-256 must equal the checksum crates.io's index records (the one
Cargo writes to Cargo.lock); the hash and size go in the listing. The new
version's `requires`, `capabilities` and `permissions` are the newest listed
version's (`--like VERSION` picks another, `--plugin-api N` sets the plugin
API), so check them when the crate's plugin changed. The listing is written
back newest first, as `rich plugin publish` writes one; validate it with
rs-rich-cli's scripts/validate_marketplace.py before opening the pull request.

Only the standard library is needed.
"""

from __future__ import annotations

import argparse
import copy
import datetime
import json
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import marketplace_common as common  # noqa: E402


class Refused(Exception):
    pass


def add_version(listing: dict, version: str, *, published: str, like: str | None = None,
                plugin_api: int | None = None, fetcher=common.fetch) -> dict:
    """The new version's entry, added to `listing` in place (newest first)."""
    if listing.get("kind") != "crate":
        raise Refused(f"{listing.get('id')} is a {listing.get('kind')}, not a crate")
    name = listing["crate"]["name"]
    common.semver_key(version)
    versions = listing["versions"]
    if any(v["version"] == version for v in versions):
        raise Refused(f"{listing['id']} already lists {version}; a listed version never changes")
    if like is not None:
        template = next((v for v in versions if v["version"] == like), None)
        if template is None:
            raise Refused(f"--like {like}: not listed")
    else:
        template = next((v for v in versions if not v.get("yanked")), None)
        if template is None:
            raise Refused("no listed version to copy requires, capabilities and permissions from; "
                          "pass --like")

    record = common.crate_index_entry(name, version, fetcher)
    if record.get("yanked"):
        raise Refused(f"{name} {version} is yanked on crates.io")
    url = common.crate_url(name, version)
    data = fetcher(url)
    digest = common.sha256(data)
    if digest != record.get("cksum"):
        raise Refused(f"{url} hashes to {digest}, but crates.io's index says {record.get('cksum')}")

    entry = {
        "version": version,
        "published": published,
        "requires": copy.deepcopy(template["requires"]),
        "artifact": {"url": url, "sha256": digest, "size": len(data)},
        "capabilities": copy.deepcopy(template["capabilities"]),
        "permissions": copy.deepcopy(template["permissions"]),
    }
    if plugin_api is not None:
        entry["requires"] = {"plugin_api": plugin_api}
    versions.append(entry)
    versions.sort(key=lambda v: common.semver_key(v["version"]), reverse=True)
    return entry


def main(argv=None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("listing", type=Path, help="packages/ID.json, a crate's listing")
    parser.add_argument("version", help="the crates.io version to add")
    parser.add_argument("--published", default=datetime.datetime.now(datetime.timezone.utc)
                        .strftime("%Y-%m-%d"), help="the listing date (default: today, UTC)")
    parser.add_argument("--like", help="copy requires, capabilities and permissions from this "
                        "listed version (default: the newest not yanked)")
    parser.add_argument("--plugin-api", type=int, help="the plugin API it needs (default: as --like)")
    parser.add_argument("--dry-run", action="store_true", help="print the entry, write nothing")
    args = parser.parse_args(argv)

    listing = json.loads(args.listing.read_bytes())
    try:
        entry = add_version(listing, args.version, published=args.published, like=args.like,
                            plugin_api=args.plugin_api)
    except (Refused, LookupError, ValueError, OSError) as error:
        print(f"add_crate_version: {error}", file=sys.stderr)
        return 1
    if not args.dry_run:
        args.listing.write_text(common.dump_listing(listing), encoding="utf-8")
    print(f"{listing['id']} {entry['version']}: {entry['artifact']['url']}\n"
          f"  sha256 {entry['artifact']['sha256']}, {entry['artifact']['size']} bytes"
          + (" (dry run, nothing written)" if args.dry_run else f"; wrote {args.listing}"))
    return 0


if __name__ == "__main__":
    sys.exit(main())
