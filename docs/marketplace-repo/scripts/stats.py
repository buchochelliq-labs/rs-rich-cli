#!/usr/bin/env python3
"""Write stats.json: download counts and ratings per package.

    python3 scripts/stats.py --output stats.json

stats.yml runs this nightly and commits the file to the `stats` branch,
where `rich plugin info` and the docs site read it from
`https://raw.githubusercontent.com/buchochelliq-labs/rs-rich-marketplace/stats/stats.json`.
It is outside the signed index and display-only: nothing installs by it.

Per package id (schemas/stats.schema.json):

- `downloads`: per listed version, the release asset's `download_count`
  for a version this repository hosts as a release, crates.io's count for a
  crate, and `null` (not counted) for a version on a raw URL or one whose
  release does not exist yet. `total` is the sum of the counted versions,
  `null` when none is counted.
- `reviews`: the package's discussion in the "Reviews" category (titled
  with its id, opened by the GitHub Actions bot): its URL, the 👍 and 👎
  reactions on its first post, and its comment count. `null` when there is
  none, or Discussions are off.

Only the standard library is needed (jsonschema too for `--jsonschema
require`). GitHub is read with the job's `GITHUB_TOKEN` (`contents: read`,
`discussions: read`); crates.io's public API with a generic User-Agent.
Nobody's identity goes anywhere, and nothing is sent about who installs
what: the counts are the ones GitHub and crates.io keep anyway.
"""

from __future__ import annotations

import argparse
import datetime
import json
import sys
import urllib.parse
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import github_api  # noqa: E402
import marketplace_common as common  # noqa: E402

ROOT = Path(__file__).resolve().parent.parent
SCHEMA = ROOT / "schemas" / "stats.schema.json"
SCHEMA_VERSION = 1
REVIEWS_CATEGORY = "Reviews"
# The logins a review discussion may be opened by: discussions.yml's, with
# the job's token. A discussion of the same title by anyone else is ignored.
BOT_LOGINS = {"github-actions", "github-actions[bot]"}
CRATES_IO = "https://crates.io/api/v1/crates/"
CRATES_USER_AGENT = ("rs-rich-marketplace-stats "
                     "(+https://github.com/buchochelliq-labs/rs-rich-marketplace)")

DISCUSSIONS_QUERY = """
query($owner: String!, $name: String!, $after: String) {
  repository(owner: $owner, name: $name) {
    hasDiscussionsEnabled
    discussionCategories(first: 50) { nodes { id name } }
    discussions(first: 100, after: $after) {
      pageInfo { hasNextPage endCursor }
      nodes {
        title url
        author { login }
        category { name }
        comments { totalCount }
        reactionGroups { content reactors { totalCount } }
      }
    }
  }
}
"""


def listings(root: Path) -> list:
    return [json.loads(p.read_bytes()) for p in sorted((root / "packages").glob("*.json"))]


def release_counts(github) -> dict:
    """`{tag: {asset name: download_count}}` over every release."""
    return {r["tag_name"]: {a["name"]: a["download_count"] for a in r.get("assets", [])}
            for r in github.releases()}


def crate_counts(name: str, fetch_json) -> dict:
    """`{version: downloads}` for a crate, from crates.io's API."""
    counts = {}
    url = f"{CRATES_IO}{urllib.parse.quote(name)}/versions"
    for _ in range(100):
        page = fetch_json(url)
        for record in page.get("versions", []):
            counts[record["num"]] = record["downloads"]
        next_page = (page.get("meta") or {}).get("next_page")
        if not next_page:
            break
        url = f"{CRATES_IO}{urllib.parse.quote(name)}/versions{next_page}"
    return counts


def crates_io_json(url: str) -> dict:
    status, _, body = github_api.urllib_send("GET", url, {"User-Agent": CRATES_USER_AGENT,
                                                          "Accept": "application/json"}, None)
    if status != 200:
        raise github_api.ApiError(status, url, body[:200].decode("utf-8", "replace"))
    return json.loads(body)


def review_discussions(github) -> tuple[dict, str | None]:
    """`({title: discussion}, warning)` for the Reviews category's
    discussions opened by the bot."""
    found = {}
    after = None
    for _ in range(100):
        data = github.graphql(DISCUSSIONS_QUERY,
                              {"owner": github.owner, "name": github.name, "after": after})
        repository = data["repository"]
        if not repository.get("hasDiscussionsEnabled"):
            return {}, "Discussions are off in this repository: no reviews counted"
        names = {c["name"] for c in repository["discussionCategories"]["nodes"]}
        if REVIEWS_CATEGORY not in names:
            return {}, f"there is no {REVIEWS_CATEGORY!r} discussion category: no reviews counted"
        page = repository["discussions"]
        for node in page["nodes"]:
            author = (node.get("author") or {}).get("login")
            if node["category"]["name"] != REVIEWS_CATEGORY or author not in BOT_LOGINS:
                continue
            found.setdefault(node["title"], node)
        if not page["pageInfo"]["hasNextPage"]:
            break
        after = page["pageInfo"]["endCursor"]
    return found, None


def reviews_entry(node: dict | None):
    if node is None:
        return None
    reactions = {g["content"]: g["reactors"]["totalCount"] for g in node["reactionGroups"]}
    return {
        "url": node["url"],
        "up": reactions.get("THUMBS_UP", 0),
        "down": reactions.get("THUMBS_DOWN", 0),
        "comments": node["comments"]["totalCount"],
    }


def build(root: Path, github, fetch_json=crates_io_json, now=None, warn=print) -> dict:
    releases = release_counts(github)
    discussions, warning = review_discussions(github)
    if warning:
        warn(f"::warning::{warning}")
    crates = {}
    packages = {}
    for listing in listings(root):
        per_version = {}
        for version in listing["versions"]:
            number = version["version"]
            url = version["artifact"]["url"]
            count = None
            if listing["kind"] == "crate":
                name = listing["crate"]["name"]
                if name not in crates:
                    crates[name] = crate_counts(name, fetch_json)
                count = crates[name].get(number)
            else:
                asset = common.release_asset(url)
                if asset is not None:
                    count = releases.get(asset[0], {}).get(asset[1])
            per_version[number] = count
        counted = [c for c in per_version.values() if c is not None]
        packages[listing["id"]] = {
            "downloads": {"total": sum(counted) if counted else None, "versions": per_version},
            "reviews": reviews_entry(discussions.get(listing["id"])),
        }
    now = now or datetime.datetime.now(datetime.timezone.utc)
    return {
        "schema_version": SCHEMA_VERSION,
        "generated_at": now.strftime("%Y-%m-%dT%H:%M:%SZ"),
        "repository": github.repository,
        "packages": packages,
    }


def validate(stats: dict, mode: str = "auto") -> list:
    """Errors against schemas/stats.schema.json (with jsonschema), plus the
    rules a schema cannot say."""
    errors = []
    try:
        import jsonschema  # noqa: PLC0415
    except ImportError:
        if mode == "require":
            return ["jsonschema is not installed"]
        jsonschema = None
    if jsonschema is not None and mode != "off":
        schema = json.loads(SCHEMA.read_text(encoding="utf-8"))
        validator = jsonschema.Draft202012Validator(schema)
        errors += [f"{'/'.join(map(str, e.path)) or '(root)'}: {e.message}"
                   for e in validator.iter_errors(stats)]
    for package_id, entry in stats.get("packages", {}).items():
        downloads = entry.get("downloads", {})
        counted = [c for c in downloads.get("versions", {}).values() if c is not None]
        expected = sum(counted) if counted else None
        if downloads.get("total") != expected:
            errors.append(f"{package_id}: total {downloads.get('total')} is not {expected}")
    return errors


def main(argv=None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--root", type=Path, default=ROOT)
    parser.add_argument("--output", type=Path, default=Path("stats.json"))
    parser.add_argument("--jsonschema", choices=["auto", "require", "off"], default="auto")
    args = parser.parse_args(argv)
    stats = build(args.root, github_api.GitHub())
    errors = validate(stats, args.jsonschema)
    for message in errors:
        print(f"::error::stats.json: {message}")
    if errors:
        return 1
    args.output.write_text(json.dumps(stats, indent=2, ensure_ascii=False, sort_keys=False) + "\n",
                           encoding="utf-8")
    print(f"wrote {args.output}: {len(stats['packages'])} packages")
    return 0


if __name__ == "__main__":
    sys.exit(main())
