#!/usr/bin/env python3
"""Rebuild the marketplace's index.json from packages/*.json.

This file belongs to the rs-rich-marketplace repository (its home in
rs-rich-cli is docs/marketplace-repo/, a copy to hand over). The format is
rs-rich-cli's docs/design/marketplace.md, "The index".

    python3 scripts/build_index.py            # writes index.json, prints the serial
    python3 scripts/build_index.py --check    # only says whether a rebuild is needed

The new index records every listing's SHA-256, a serial one above the
current index.json's (1 when there is none), `generated` now and `expires`
30 days from now (`--days N`). Each listing must be `packages/<id>.json`
with that `id`. Nothing is signed here: the workflow signs the bytes this
writes, with the trusted comment `rs-rich-marketplace index serial=N`.

Only the standard library is needed.
"""

from __future__ import annotations

import argparse
import datetime
import hashlib
import json
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent


def listings(root: Path) -> dict:
    entries = {}
    for path in sorted((root / "packages").glob("*.json")):
        data = path.read_bytes()
        try:
            listing = json.loads(data)
        except ValueError as error:
            sys.exit(f"{path}: not JSON: {error}")
        listing_id = listing.get("id") if isinstance(listing, dict) else None
        if listing_id != path.stem:
            sys.exit(f"{path}: its id is {listing_id!r}, but it is filed as {path.stem!r}")
        entries[path.stem] = {
            "path": f"packages/{path.stem}.json",
            "sha256": hashlib.sha256(data).hexdigest(),
        }
    return entries


def main(argv=None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--root", type=Path, default=ROOT)
    parser.add_argument("--days", type=int, default=30, help="days until it expires (30)")
    parser.add_argument("--check", action="store_true",
                        help="exit 1 when index.json does not list packages/ as it is")
    args = parser.parse_args(argv)
    if not 1 <= args.days <= 90:
        parser.error("--days must be 1 to 90")

    index_path = args.root / "index.json"
    previous = json.loads(index_path.read_bytes()) if index_path.exists() else None
    entries = listings(args.root)
    if args.check:
        same = previous is not None and previous.get("listings") == entries
        print("index.json is current" if same else "index.json needs a rebuild")
        return 0 if same else 1

    serial = 1
    if previous is not None:
        old = previous.get("serial")
        if not isinstance(old, int) or isinstance(old, bool) or old < 1:
            sys.exit("index.json: serial is not a positive integer")
        serial = old + 1
    now = datetime.datetime.now(datetime.timezone.utc).replace(microsecond=0)
    stamp = "%Y-%m-%dT%H:%M:%SZ"
    index = {
        "schema_version": 1,
        "serial": serial,
        "generated": now.strftime(stamp),
        "expires": (now + datetime.timedelta(days=args.days)).strftime(stamp),
        "listings": entries,
    }
    index_path.write_text(json.dumps(index, indent=2) + "\n", encoding="utf-8")
    print(serial)
    return 0


if __name__ == "__main__":
    sys.exit(main())
