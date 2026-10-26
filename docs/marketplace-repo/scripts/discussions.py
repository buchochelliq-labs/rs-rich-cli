#!/usr/bin/env python3
"""Open a Reviews discussion for every listed package that has none.

    python3 scripts/discussions.py
    python3 scripts/discussions.py --dry-run

discussions.yml runs this after a merge that touches packages/, and by
hand. Each package gets one discussion in the "Reviews" category, titled
with its id: people rate it with 👍 or 👎 on the first post and review it
in comments, and stats.py counts both. A package whose discussion exists
(found by its title, among the discussions this workflow's bot opened) is
left alone, so a run can be repeated.

Discussions and the category are set up once, by the maintainer, in the
repository's settings (there is no API for either); without them this
exits 1 with an `::error::` that says so. The token is the job's
`GITHUB_TOKEN` (`discussions: write`). Only the standard library is needed.
"""

from __future__ import annotations

import argparse
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import github_api  # noqa: E402
import marketplace_common as common  # noqa: E402
import stats  # noqa: E402

ROOT = Path(__file__).resolve().parent.parent
SETUP = (
    "enable Discussions (Settings > General > Features) and create a discussion "
    f"category named {stats.REVIEWS_CATEGORY!r} (Discussions > Categories; the "
    "Announcement format, so only maintainers and this workflow open discussions while "
    "anyone may react and comment), then run this workflow again"
)

CATEGORY_QUERY = """
query($owner: String!, $name: String!) {
  repository(owner: $owner, name: $name) {
    id
    hasDiscussionsEnabled
    discussionCategories(first: 50) { nodes { id name } }
  }
}
"""

CREATE = """
mutation($repository: ID!, $category: ID!, $title: String!, $body: String!) {
  createDiscussion(input: {repositoryId: $repository, categoryId: $category,
                           title: $title, body: $body}) {
    discussion { url }
  }
}
"""


class SetupMissing(Exception):
    pass


def newest(listing: dict):
    """The newest version that is not yanked, else the newest."""
    versions = sorted(listing["versions"], key=lambda v: common.semver_key(v["version"]),
                      reverse=True)
    live = [v for v in versions if not v.get("yanked")]
    return (live or versions)[0]


def body(listing: dict) -> str:
    version = newest(listing)
    if listing["kind"] == "crate":
        crate = listing["crate"]
        add = f"cargo add {crate['name']}@={version['version']}"
        if crate.get("features"):
            add += f" --features {','.join(crate['features'])}"
        how = ("It is a Rust crate, listed only: add it to your program with\n\n"
               f"```bash\n{add}\n```")
    else:
        how = (f"Install it with\n\n```bash\nrich plugin install {listing['id']}\n```\n\n"
               f"and see it with `rich plugin info {listing['id']}`.")
    return "\n".join([
        f"**{listing['name']}** ({listing['kind']}, {listing['tier']}): {listing['description']}",
        "",
        how,
        "",
        "👍 / 👎 this post to rate it; comment to review.",
        "",
        f"Listing: [`packages/{listing['id']}.json`]"
        f"(https://github.com/{common.REPOSITORY}/blob/main/packages/{listing['id']}.json)"
        f" · Source: {listing['repository']}",
        "",
        "The counts are shown by `rich plugin info` and on the docs site. They are not "
        "part of the signed index, and nothing installs by them.",
    ]) + "\n"


def category(github) -> tuple[str, str]:
    """`(repository id, Reviews category id)`; SetupMissing without them."""
    repository = github.graphql(CATEGORY_QUERY, {"owner": github.owner,
                                                  "name": github.name})["repository"]
    if not repository.get("hasDiscussionsEnabled"):
        raise SetupMissing(f"Discussions are off in {github.repository}: {SETUP}")
    for node in repository["discussionCategories"]["nodes"]:
        if node["name"] == stats.REVIEWS_CATEGORY:
            return repository["id"], node["id"]
    raise SetupMissing(f"{github.repository} has no {stats.REVIEWS_CATEGORY!r} discussion "
                       f"category: {SETUP}")


def open_missing(github, root: Path, dry_run: bool, out=print) -> list:
    repository_id, category_id = category(github)
    existing, warning = stats.review_discussions(github)
    if warning:
        raise SetupMissing(warning)
    opened = []
    for listing in stats.listings(root):
        if listing["id"] in existing:
            out(f"ok: {listing['id']}: {existing[listing['id']]['url']}")
            continue
        if dry_run:
            out(f"would open a review discussion for {listing['id']}")
            continue
        result = github.graphql(CREATE, {"repository": repository_id, "category": category_id,
                                         "title": listing["id"], "body": body(listing)})
        url = result["createDiscussion"]["discussion"]["url"]
        out(f"opened {listing['id']}: {url}")
        opened.append(listing["id"])
    return opened


def main(argv=None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--root", type=Path, default=ROOT)
    parser.add_argument("--dry-run", action="store_true")
    args = parser.parse_args(argv)
    try:
        open_missing(github_api.GitHub(), args.root, args.dry_run)
    except SetupMissing as error:
        print(f"::error::{error}")
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
