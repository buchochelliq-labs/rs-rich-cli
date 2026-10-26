"""Helpers the marketplace's scripts share: crates.io, downloads, SemVer.

Only the standard library is needed. Every request carries a generic
User-Agent and nothing else about who runs it.
"""

from __future__ import annotations

import hashlib
import json
import re
import urllib.request

USER_AGENT = "rs-rich-marketplace-tools"
# Where the marketplace serves its own artifacts (`artifacts/` on main).
RAW_MAIN = "https://raw.githubusercontent.com/buchochelliq-labs/rs-rich-marketplace/main/"
# The largest artifact the format allows (a WASM module, 16 MiB) and a
# generous bound for a .crate file.
MAX_DOWNLOAD = 64 << 20

_PRE = r"(?:0|[1-9]\d*|\d*[A-Za-z-][0-9A-Za-z-]*)"
SEMVER = re.compile(
    r"^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)"
    rf"(?:-({_PRE}(?:\.{_PRE})*))?(?:\+[0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*)?$"
)


def semver_key(version: str):
    """A sort key with SemVer 2.0 precedence (build metadata ignored); the
    same as rs-rich-cli's scripts/validate_marketplace.py."""
    match = SEMVER.match(version)
    if not match:
        raise ValueError(f"{version!r} is not SemVer")
    major, minor, patch, pre = match.groups()
    if pre is None:
        pre_key = (1,)
    else:
        parts = []
        for part in pre.split("."):
            parts.append((0, int(part), "") if part.isdigit() else (1, 0, part))
        pre_key = (0, tuple(parts))
    return (int(major), int(minor), int(patch), pre_key)


def fetch(url: str, limit: int = MAX_DOWNLOAD) -> bytes:
    """GET an https:// URL, reading at most `limit` bytes (more is an error)."""
    if not url.startswith("https://"):
        raise ValueError(f"{url}: only https:// is fetched")
    request = urllib.request.Request(url, headers={"User-Agent": USER_AGENT})
    with urllib.request.urlopen(request, timeout=60) as response:
        if not response.geturl().startswith("https://"):
            raise ValueError(f"{url}: redirected off https://")
        data = response.read(limit + 1)
    if len(data) > limit:
        raise ValueError(f"{url}: larger than {limit} bytes")
    return data


def sparse_index_path(name: str) -> str:
    """The crate's file in crates.io's sparse index."""
    name = name.lower()
    if len(name) <= 2:
        return f"{len(name)}/{name}"
    if len(name) == 3:
        return f"3/{name[0]}/{name}"
    return f"{name[:2]}/{name[2:4]}/{name}"


def crate_index_entry(name: str, version: str, fetcher=fetch) -> dict:
    """crates.io's index record for one version of a crate (its `cksum`,
    `yanked`, `features` and dependencies)."""
    text = fetcher(f"https://index.crates.io/{sparse_index_path(name)}").decode("utf-8")
    for line in text.splitlines():
        if not line.strip():
            continue
        record = json.loads(line)
        if record.get("vers") == version:
            return record
    raise LookupError(f"{name} {version} is not on crates.io")


def crate_url(name: str, version: str) -> str:
    return f"https://static.crates.io/crates/{name}/{name}-{version}.crate"


def sha256(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def dump_listing(listing: dict) -> str:
    """A listing as `rich plugin publish` writes it: two-space JSON, UTF-8,
    a final newline."""
    return json.dumps(listing, indent=2, ensure_ascii=False) + "\n"
