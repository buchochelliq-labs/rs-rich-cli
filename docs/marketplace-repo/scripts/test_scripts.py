"""Tests for the marketplace's scripts, with no network.

    python3 -m unittest discover -s scripts -p 'test_*.py' -v
"""

from __future__ import annotations

import copy
import json
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import add_crate_version  # noqa: E402
import build_index  # noqa: E402
import build_site  # noqa: E402
import check_artifacts  # noqa: E402
import discussions  # noqa: E402
import github_api  # noqa: E402
import marketplace_common as common  # noqa: E402
import publish_releases  # noqa: E402
import stats  # noqa: E402

REPO_ROOT = Path(__file__).resolve().parent.parent
FIXTURES = Path(__file__).resolve().parent / "fixtures"
# Listings as packages/ has them (a release-hosted textcase 0.2.0, dusk and
# wordplay; raw-hosted textcase 0.1.0 and status; two crates), copied so the
# tests do not move when packages/ does. Their artifacts are the repository's.
LISTINGS = FIXTURES


def fixture(name):
    return json.loads((FIXTURES / name).read_text(encoding="utf-8"))

CRATE = b"pretend .crate bytes"
CKSUM = common.sha256(CRATE)


def crate_listing():
    return {
        "schema_version": 1, "id": "mermaid", "kind": "crate", "name": "Mermaid",
        "description": "d", "tier": "official", "license": "MIT",
        "repository": "https://github.com/buchochelliq-labs/rs-rich-cli",
        "crate": {"name": "rs-rich-mermaid"},
        "versions": [{
            "version": "0.0.8", "published": "2026-10-26", "requires": {"plugin_api": 1},
            "artifact": {"url": common.crate_url("rs-rich-mermaid", "0.0.8"),
                         "sha256": "0" * 64, "size": 1},
            "capabilities": [{"kind": "fence-renderer", "name": "mermaid"}],
            "permissions": ["native-code"],
        }],
    }


def fake_fetch(index_lines, files):
    def fetch(url, limit=common.MAX_DOWNLOAD):
        if url.startswith("https://index.crates.io/"):
            return "\n".join(json.dumps(line) for line in index_lines).encode()
        return files[url]
    return fetch


class Common(unittest.TestCase):
    def test_sparse_index_path(self):
        self.assertEqual(common.sparse_index_path("rs-rich-mermaid"), "rs/-r/rs-rich-mermaid")
        self.assertEqual(common.sparse_index_path("abc"), "3/a/abc")
        self.assertEqual(common.sparse_index_path("ab"), "2/ab")

    def test_semver_order(self):
        versions = ["0.0.9", "0.0.10", "1.0.0-rc.1", "1.0.0"]
        versions.sort(key=common.semver_key, reverse=True)
        self.assertEqual(versions, ["1.0.0", "1.0.0-rc.1", "0.0.10", "0.0.9"])

    def test_only_https(self):
        with self.assertRaises(ValueError):
            common.fetch("http://example.com/x")


class Hosting(unittest.TestCase):
    def test_hosted_paths(self):
        raw = common.RAW_MAIN + "artifacts/a/a-1.0.0.ini"
        self.assertEqual(common.hosted_path("a", "1.0.0", raw), "artifacts/a/a-1.0.0.ini")
        release = common.RELEASES + "high-contrast-v1.0.0/high-contrast-1.0.0.ini"
        self.assertEqual(common.hosted_path("high-contrast", "1.0.0", release),
                         "artifacts/high-contrast/high-contrast-1.0.0.ini")
        self.assertIsNone(common.hosted_path("a", "1.0.0", "https://example.com/a-1.0.0.ini"))
        self.assertEqual(common.release_tag("textcase", "0.2.0"), "textcase-v0.2.0")


class AddCrateVersion(unittest.TestCase):
    def fetch(self, yanked=False, cksum=CKSUM):
        url = common.crate_url("rs-rich-mermaid", "0.0.9")
        return fake_fetch([{"vers": "0.0.8", "cksum": "x", "yanked": False},
                           {"vers": "0.0.9", "cksum": cksum, "yanked": yanked}], {url: CRATE})

    def test_adds_newest_first_with_the_index_checksum(self):
        listing = crate_listing()
        entry = add_crate_version.add_version(listing, "0.0.9", published="2026-10-27",
                                              fetcher=self.fetch())
        self.assertEqual([v["version"] for v in listing["versions"]], ["0.0.9", "0.0.8"])
        self.assertEqual(entry["artifact"], {"url": common.crate_url("rs-rich-mermaid", "0.0.9"),
                                             "sha256": CKSUM, "size": len(CRATE)})
        self.assertEqual(entry["capabilities"], listing["versions"][1]["capabilities"])
        self.assertEqual(entry["permissions"], ["native-code"])

    def test_refusals(self):
        for kwargs, fetch in [({}, self.fetch(yanked=True)), ({}, self.fetch(cksum="f" * 64))]:
            with self.assertRaises(add_crate_version.Refused):
                add_crate_version.add_version(crate_listing(), "0.0.9", published="x",
                                              fetcher=fetch, **kwargs)
        with self.assertRaises(add_crate_version.Refused):
            add_crate_version.add_version(crate_listing(), "0.0.8", published="x",
                                          fetcher=self.fetch())
        with self.assertRaises(LookupError):
            add_crate_version.add_version(crate_listing(), "0.1.0", published="x",
                                          fetcher=self.fetch())

    def test_plugin_api_override(self):
        listing = crate_listing()
        entry = add_crate_version.add_version(listing, "0.0.9", published="x", plugin_api=2,
                                              fetcher=self.fetch())
        self.assertEqual(entry["requires"], {"plugin_api": 2})


class CheckArtifacts(unittest.TestCase):
    def setUp(self):
        self.dir = tempfile.TemporaryDirectory()
        self.root = Path(self.dir.name)
        (self.root / "packages").mkdir()
        (self.root / "artifacts/pack").mkdir(parents=True)
        subprocess.run(["git", "init", "-q", str(self.root)], check=True)
        self.pack = b"PK pretend zip"
        (self.root / "artifacts/pack/pack-1.0.0.richmicro").write_bytes(self.pack)

    def tearDown(self):
        self.dir.cleanup()

    def pack_listing(self, tier="community", sha=None):
        return {
            "schema_version": 1, "id": "pack", "kind": "micro-pack", "name": "p",
            "description": "d", "tier": tier, "license": "MIT",
            "repository": "https://github.com/example/pack",
            "versions": [{
                "version": "1.0.0", "published": "2026-10-26", "requires": {"micro_schema": 1},
                "artifact": {"url": common.RAW_MAIN + "artifacts/pack/pack-1.0.0.richmicro",
                             "sha256": sha or common.sha256(self.pack), "size": len(self.pack)},
                "capabilities": [{"kind": "asset", "name": "pack/a"}], "permissions": [],
            }],
        }

    def write(self, name, listing):
        (self.root / "packages" / f"{name}.json").write_text(common.dump_listing(listing))

    def commit(self):
        git = ["git", "-C", str(self.root), "-c", "user.name=t", "-c", "user.email=t@example.invalid"]
        subprocess.run(git + ["add", "-A"], check=True)
        subprocess.run(git + ["commit", "-q", "-m", "base"], check=True)

    def run_checker(self, base="HEAD", allow_tier=False, crate_entry=None, fetch=None):
        checker = check_artifacts.Checker(self.root, fetcher=fetch or fake_fetch([], {}),
                                          crate_entry=crate_entry or (lambda n, v: {}))
        for path in sorted((self.root / "packages").glob("*.json")):
            checker.check_listing(path, base, False, allow_tier)
        return checker

    def test_a_local_artifact_is_read_from_the_checkout(self):
        self.write("pack", self.pack_listing())
        checker = self.run_checker(base=None)
        self.assertEqual(checker.errors, [])
        self.assertEqual(len(checker.checked), 1)

    def release_listing(self, tag="pack-v1.0.0", name="pack-1.0.0.richmicro"):
        listing = self.pack_listing()
        listing["versions"][0]["artifact"]["url"] = f"{common.RELEASES}{tag}/{name}"
        return listing

    def test_a_release_asset_is_read_from_the_checkout(self):
        self.write("pack", self.release_listing())
        checker = self.run_checker(base=None)
        self.assertEqual(checker.errors, [])
        self.assertEqual(len(checker.checked), 1)

    def test_a_release_asset_must_be_in_the_checkout(self):
        (self.root / "artifacts/pack/pack-1.0.0.richmicro").unlink()
        self.write("pack", self.release_listing())
        self.assertIn("not in this branch", self.run_checker(base=None).errors[0])

    def test_a_release_asset_names_its_id_and_version(self):
        for tag, name, expected in [
            ("pack-v1.0.1", "pack-1.0.0.richmicro", "the release must be pack-v1.0.0"),
            ("v1.0.0", "pack-1.0.0.richmicro", "the release must be pack-v1.0.0"),
            ("pack-v1.0.0", "other-1.0.0.richmicro", "must be named pack-1.0.0.EXT"),
            ("pack-v1.0.0", "pack-1.0.1.richmicro", "must be named pack-1.0.0.EXT"),
        ]:
            self.write("pack", self.release_listing(tag, name))
            errors = self.run_checker(base=None).errors
            self.assertEqual(len(errors), 1, (tag, name))
            self.assertIn(expected, errors[0])
        self.write("pack", self.release_listing("pack-v1.0.0/extra", "pack-1.0.0.richmicro"))
        self.assertIn("expected", self.run_checker(base=None).errors[0])

    def test_a_listed_release_version_is_checked_on_every_run(self):
        self.write("pack", self.release_listing())
        self.commit()
        (self.root / "artifacts/pack/pack-1.0.0.richmicro").write_bytes(b"changed")
        self.assertIn("bytes, listed as", self.run_checker().errors[0])

    def test_a_wrong_hash_is_refused(self):
        self.write("pack", self.pack_listing(sha="a" * 64))
        self.assertIn("hashes to", self.run_checker(base=None).errors[0])

    def test_a_raised_tier_needs_approval(self):
        self.write("pack", self.pack_listing())
        self.commit()
        self.write("pack", self.pack_listing(tier="reviewed"))
        self.assertIn("tier-approved", self.run_checker().errors[0])
        self.assertEqual(self.run_checker(allow_tier=True).errors, [])

    def test_a_listed_version_never_changes_but_may_be_yanked(self):
        self.write("pack", self.pack_listing())
        self.commit()
        listing = self.pack_listing()
        listing["versions"][0]["published"] = "2026-10-27"
        self.write("pack", listing)
        self.assertIn("never changes", self.run_checker().errors[0])
        listing = self.pack_listing()
        listing["versions"][0].update(yanked=True, yank_reason="broken")
        self.write("pack", listing)
        self.assertEqual(self.run_checker().errors, [])

    def test_a_listings_identity_never_changes_once_versions_are_listed(self):
        listing = crate_listing()
        self.write("mermaid", listing)
        self.commit()
        renamed = crate_listing()
        renamed["crate"]["name"] = "rs-rich-lumis"
        self.write("mermaid", renamed)
        errors = self.run_checker(allow_tier=True).errors
        self.assertEqual(len(errors), 1)
        self.assertIn("never change once a version is listed", errors[0])
        rekinded = self.pack_listing()
        self.write("pack", rekinded)
        self.commit()
        rekinded["kind"] = "theme"
        self.write("pack", rekinded)
        self.assertTrue(any("never change" in e for e in self.run_checker(allow_tier=True).errors))

    def test_a_crate_must_match_crates_io(self):
        listing = crate_listing()
        url = listing["versions"][0]["artifact"]["url"]
        listing["versions"][0]["artifact"].update(sha256=CKSUM, size=len(CRATE))
        self.write("mermaid", listing)
        fetch = fake_fetch([], {url: CRATE})
        ok = self.run_checker(base=None, fetch=fetch, allow_tier=True,
                              crate_entry=lambda n, v: {"cksum": CKSUM, "yanked": False})
        self.assertEqual(ok.errors, [])
        bad = self.run_checker(base=None, fetch=fetch, allow_tier=True,
                               crate_entry=lambda n, v: {"cksum": "b" * 64, "yanked": False})
        self.assertIn("crates.io's index", bad.errors[0])
        yanked = self.run_checker(base=None, fetch=fetch, allow_tier=True,
                                  crate_entry=lambda n, v: {"cksum": CKSUM, "yanked": True})
        self.assertIn("yanked on crates.io", yanked.errors[0])


class Names(unittest.TestCase):
    OFFICIAL = {"textcase", "wordplay", "mermaid", "dot", "mono", "high-contrast"}

    def problems(self, listing_id="shout", name="Shout", description="Shouts text"):
        return check_artifacts.name_problems(
            {"id": listing_id, "name": name, "description": description}, self.OFFICIAL)

    def test_a_plain_listing_passes(self):
        for listing_id in ["shout", "richtext", "rsx", "dog", "doc", "dots", "monolith",
                           "case-text", "wordle", "rust-rich-theme"]:
            self.assertEqual(self.problems(listing_id), [], listing_id)

    def test_reserved_ids(self):
        for listing_id in ["rs-rich-extra", "rsrich", "rs_rich.icons", "rich-shout", "rich",
                           "rs", "r-s", "richcli-tools", "official-pack", "Official"]:
            self.assertTrue(any("reserved" in p for p in self.problems(listing_id)), listing_id)

    def test_lookalike_ids(self):
        for listing_id in ["textcase2", "text-case", "text_case", "textcas", "wordp1ay",
                           "word-play", "mermaid.", "mermald", "d-o-t", "mon0",
                           "high_contrast", "highcontrast2"]:
            self.assertTrue(any("looks like the official package" in p
                                for p in self.problems(listing_id)), listing_id)

    def test_non_ascii_ids(self):
        self.assertIn("must be ASCII", self.problems("textc\u0430se")[0])

    def test_reserved_wording(self):
        for name in ["Official shout", "OFFICIAL", "O-f-f-i-c-i-a-l shout", "Shout by rs-rich",
                     "RS_RICH shout", "Unofficial shout"]:
            self.assertTrue(self.problems(name=name), name)
        for description in ["The official shout", "An un-official port", "OFFICIAL!"]:
            self.assertTrue(self.problems(description=description), description)
        self.assertEqual(self.problems(name="Shout for rich", description="Rich text, louder"), [])


class NameChecks(CheckArtifacts):
    def official(self):
        listing = self.pack_listing(tier="official")
        listing["id"] = "textcase"
        listing["repository"] = "https://github.com/buchochelliq-labs/rs-rich-cli"
        return listing

    def test_a_new_lookalike_is_refused_unless_approved(self):
        self.write("textcase", self.official())
        self.commit()
        lookalike = self.pack_listing()
        lookalike["id"] = "text-case"
        self.write("text-case", lookalike)
        errors = self.run_checker().errors
        self.assertEqual(len(errors), 1)
        self.assertIn("looks like the official package 'textcase'", errors[0])
        self.assertIn("name-approved", errors[0])
        checker = check_artifacts.Checker(self.root, fetcher=fake_fetch([], {}),
                                          crate_entry=lambda n, v: {}, allow_names=True)
        for path in sorted((self.root / "packages").glob("*.json")):
            checker.check_listing(path, "HEAD", False, False)
        self.assertEqual(checker.errors, [])

    def test_an_official_listing_may_use_the_words(self):
        listing = self.official()
        listing["name"] = "Text case, official"
        self.write("textcase", listing)
        self.assertEqual(self.run_checker(base=None, allow_tier=True).errors, [])

    def test_a_listed_name_is_checked_again_only_when_it_changes(self):
        listing = self.pack_listing()
        listing["name"] = "Unofficial pack"
        self.write("pack", listing)
        self.assertIn("says 'official'", self.run_checker(base=None).errors[0])
        self.commit()
        # Merged with the label: later pull requests do not trip on it.
        self.assertEqual(self.run_checker().errors, [])
        listing["description"] = "The official pack"
        self.write("pack", listing)
        self.assertTrue(self.run_checker().errors)


class Recommended(CheckArtifacts):
    def check(self, allow=False):
        checker = check_artifacts.Checker(self.root, fetcher=fake_fetch([], {}),
                                          crate_entry=lambda n, v: {}, allow_recommended=allow)
        for path in sorted((self.root / "packages").glob("*.json")):
            checker.check_listing(path, "HEAD", False, False)
        return checker.errors

    def test_adding_changing_or_removing_needs_the_label(self):
        listing = self.pack_listing()
        self.write("pack", listing)
        self.commit()
        self.assertEqual(self.check(), [])
        listing["recommended"] = {"since": "2026-10-27", "note": "A good pack"}
        self.write("pack", listing)
        self.assertEqual(len(self.check()), 1)
        self.assertIn("`recommended` added needs a maintainer's `recommended-approved` label",
                      self.check()[0])
        self.assertEqual(self.check(allow=True), [])
        self.commit()
        self.assertEqual(self.check(), [], "unchanged needs nothing")
        listing["recommended"]["note"] = "A better pack"
        self.write("pack", listing)
        self.assertIn("`recommended` changed", self.check()[0])
        del listing["recommended"]
        self.write("pack", listing)
        self.assertIn("`recommended` removed", self.check()[0])

    def test_tier_approval_is_not_enough(self):
        listing = self.pack_listing()
        self.write("pack", listing)
        self.commit()
        listing["recommended"] = {"since": "2026-10-27", "note": "n"}
        self.write("pack", listing)
        self.assertIn("recommended-approved", self.run_checker(allow_tier=True).errors[0])

    def test_a_new_listing_that_is_recommended_needs_it_too(self):
        listing = self.pack_listing()
        listing["recommended"] = {"since": "2026-10-27", "note": "n"}
        self.write("pack", listing)
        self.assertIn("`recommended` added", self.check()[0])


class BuildIndex(unittest.TestCase):
    def test_serial_grows_and_ids_match(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "packages").mkdir()
            listing = crate_listing()
            (root / "packages/mermaid.json").write_text(common.dump_listing(listing))
            self.assertEqual(build_index.main(["--root", str(root)]), 0)
            self.assertEqual(build_index.main(["--root", str(root)]), 0)
            index = json.loads((root / "index.json").read_text())
            self.assertEqual(index["serial"], 2)
            self.assertEqual(list(index["listings"]), ["mermaid"])
            self.assertEqual(build_index.main(["--root", str(root), "--check"]), 0)
            other = copy.deepcopy(listing)
            other["id"] = "other"
            (root / "packages/wrong.json").write_text(common.dump_listing(other))
            with self.assertRaises(SystemExit):
                build_index.main(["--root", str(root)])


class BuildSite(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.root = Path(self.tmp.name) / "repo"
        (self.root / "site").mkdir(parents=True)
        (self.root / "site" / "index.html").write_text(
            '<meta http-equiv="Content-Security-Policy" content="default-src \'none\'">', encoding="utf-8")
        (self.root / "packages").mkdir()
        (self.root / "keys").mkdir()
        (self.root / "keys" / "keys-1.json").write_text("{}", encoding="utf-8")
        self.signed = json.dumps({"id": "a", "name": "A <b>", "tier": "official", "kind": "theme"}).encode()
        index = {"schema_version": 1, "listings": {"a": {"path": "packages/a.json", "sha256": common.sha256(self.signed)}}}
        (self.root / "index.json").write_text(json.dumps(index), encoding="utf-8")
        (self.root / "index.json.minisig").write_text("sig", encoding="utf-8")
        self.out = Path(self.tmp.name) / "out"

    def tearDown(self):
        self.tmp.cleanup()

    def test_serves_the_listing_the_index_signed(self):
        (self.root / "packages" / "a.json").write_bytes(self.signed)
        self.assertEqual(build_site.build(self.root, self.out, history=lambda root, path: iter(())), [])
        self.assertEqual((self.out / "packages" / "a.json").read_bytes(), self.signed)
        for name in ("index.html", "index.json", "index.json.minisig", "keys/keys-1.json"):
            self.assertTrue((self.out / name).is_file(), name)
        page = (self.out / "packages" / "index.html").read_text(encoding="utf-8")
        self.assertIn('href="a.json"', page)
        self.assertIn("A &lt;b&gt;", page)
        self.assertNotIn("<script", page)

    def test_a_listing_merged_after_the_signing_is_served_as_signed(self):
        (self.root / "packages" / "a.json").write_bytes(b'{"id": "a", "changed": true}')
        history = lambda root, path: iter([b"older", self.signed])
        self.assertEqual(build_site.build(self.root, self.out, history=history), [])
        self.assertEqual((self.out / "packages" / "a.json").read_bytes(), self.signed)

    def test_a_listing_no_version_of_which_matches_is_an_error(self):
        (self.root / "packages" / "a.json").write_bytes(b"{}")
        problems = build_site.build(self.root, self.out, history=lambda root, path: iter([b"x"]))
        self.assertEqual(len(problems), 1)
        self.assertIn("packages/a.json", problems[0])


class GitHubClient(unittest.TestCase):
    def test_requests_carry_the_token_and_a_generic_user_agent_only(self):
        seen = []

        def send(method, url, headers, body):
            seen.append((method, url, headers))
            return 200, {}, b"[]"

        github_api.GitHub(token="t0k", send=send).releases()
        method, url, headers = seen[0]
        self.assertEqual((method, url), ("GET", github_api.API + "repos/"
                                         f"{common.REPOSITORY}/releases?per_page=100&page=1"))
        self.assertEqual(headers["Authorization"], "Bearer t0k")
        self.assertEqual(headers["User-Agent"], github_api.USER_AGENT)

    def test_an_asset_download_follows_one_redirect_without_the_token(self):
        seen = []

        def send(method, url, headers, body):
            seen.append((url, headers))
            if url.startswith(github_api.API):
                return 302, {"Location": "https://objects.example.test/asset"}, b""
            return 200, {}, b"bytes"

        data = github_api.GitHub(token="t0k", send=send).download_asset({"id": 7})
        self.assertEqual(data, b"bytes")
        self.assertEqual(seen[0][1]["Accept"], "application/octet-stream")
        self.assertNotIn("Authorization", seen[1][1])

    def test_errors(self):
        github = github_api.GitHub(token="", send=lambda *a: (200, {}, b'{"errors": [{"message": "no"}]}'))
        with self.assertRaises(github_api.ApiError):
            github.graphql("query {}", {})
        github = github_api.GitHub(token="", send=lambda *a: (404, {}, b"{}"))
        self.assertIsNone(github.release_by_tag("x-v1.0.0"))
        with self.assertRaises(github_api.ApiError):
            github.releases()


class FakeReleases:
    """GitHub's releases, in memory."""

    def __init__(self):
        self.releases = {}
        self.calls = []

    def release_by_tag(self, tag):
        return self.releases.get(tag)

    def create_release(self, tag, target, name, body):
        self.calls.append(("create", tag, target))
        release = {"id": len(self.releases) + 1, "tag_name": tag, "assets": [], "body": body}
        self.releases[tag] = release
        return release

    def upload_asset(self, release, name, data):
        self.calls.append(("upload", release["tag_name"], name))
        asset = {"id": name, "name": name, "size": len(data), "data": data}
        release["assets"].append(asset)
        return asset

    def download_asset(self, asset):
        return asset["data"]


class PublishReleases(unittest.TestCase):
    def setUp(self):
        if not (REPO_ROOT / "artifacts").is_dir():
            self.skipTest("needs the marketplace's artifacts/ (not in rs-rich-cli's copy)")
        self.dir = tempfile.TemporaryDirectory()
        self.root = Path(self.dir.name)
        (self.root / "packages").mkdir()
        for name in ["textcase", "dusk", "status"]:
            listing = json.loads((LISTINGS / "packages" / f"{name}.json").read_bytes())
            (self.root / "packages" / f"{name}.json").write_text(common.dump_listing(listing))
            for version in listing["versions"]:
                relative = common.hosted_path(name, version["version"], version["artifact"]["url"])
                target = self.root / relative
                source = REPO_ROOT / relative
                if not source.is_file():
                    # The fixtures name the official files; a checkout
                    # before they are merged lacks some of them.
                    self.dir.cleanup()
                    self.skipTest(f"needs {relative} from the marketplace's artifacts/")
                target.parent.mkdir(parents=True, exist_ok=True)
                target.write_bytes(source.read_bytes())

    def tearDown(self):
        self.dir.cleanup()

    def test_makes_each_missing_release_once(self):
        github = FakeReleases()
        out = []
        self.assertEqual(publish_releases.publish(github, self.root, "abc123", False, out.append), [])
        # Only the release-hosted versions: textcase 0.2.0 and dusk 1.0.0,
        # not textcase 0.1.0 or status 1.0.0 on raw URLs.
        self.assertEqual(sorted(github.releases), ["dusk-v1.0.0", "textcase-v0.2.0"])
        self.assertIn(("create", "dusk-v1.0.0", "abc123"), github.calls)
        self.assertIn("rich plugin install dusk@1.0.0", github.releases["dusk-v1.0.0"]["body"])
        calls = len(github.calls)
        self.assertEqual(publish_releases.publish(github, self.root, "def456", False, out.append), [])
        self.assertEqual(len(github.calls), calls, "a second run changes nothing")

    def test_an_existing_asset_that_does_not_match_is_never_replaced(self):
        github = FakeReleases()
        release = github.create_release("dusk-v1.0.0", "abc", "dusk 1.0.0", "")
        github.upload_asset(release, "dusk-1.0.0.ini", b"x" * 4524)
        github.calls.clear()
        errors = publish_releases.publish(github, self.root, "abc", False, lambda _: None)
        self.assertEqual(len(errors), 1)
        self.assertIn("never replaced", errors[0])
        self.assertNotIn(("upload", "dusk-v1.0.0", "dusk-1.0.0.ini"), github.calls)

    def test_a_release_without_its_asset_gets_it(self):
        github = FakeReleases()
        github.create_release("dusk-v1.0.0", "abc", "dusk 1.0.0", "")
        publish_releases.publish(github, self.root, "abc", False, lambda _: None)
        self.assertEqual(github.releases["dusk-v1.0.0"]["assets"][0]["name"], "dusk-1.0.0.ini")

    def test_a_changed_file_in_the_checkout_is_refused(self):
        (self.root / "artifacts/dusk/dusk-1.0.0.ini").write_text("[styles]\n")
        github = FakeReleases()
        errors = publish_releases.publish(github, self.root, "abc", False, lambda _: None)
        self.assertIn("listed as 4524", errors[0])
        self.assertNotIn("dusk-v1.0.0", github.releases)

    def test_a_dry_run_changes_nothing(self):
        github = FakeReleases()
        out = []
        self.assertEqual(publish_releases.publish(github, self.root, None, True, out.append), [])
        self.assertEqual(github.calls, [])
        self.assertIn("would create release dusk-v1.0.0 and upload dusk-1.0.0.ini", out)


class FakeApi:
    """stats.py's and discussions.py's view of GitHub, from the fixtures."""

    def __init__(self, discussions_fixture="github-discussions.json"):
        self.repository = common.REPOSITORY
        self.owner, self.name = self.repository.split("/")
        self.discussions = fixture(discussions_fixture)["data"]
        self.created = []

    def releases(self):
        return fixture("github-releases.json")

    def graphql(self, query, variables):
        if "createDiscussion" in query:
            self.created.append(variables)
            number = 10 + len(self.created)
            return {"createDiscussion": {"discussion": {
                "url": f"https://github.com/{self.repository}/discussions/{number}"}}}
        repository = dict(self.discussions["repository"])
        repository.setdefault("id", "R_marketplace")
        return {"repository": repository}


def crates_io(url):
    name = url.split("/crates/")[1].split("/")[0]
    return fixture(f"crates-io-{name}-versions.json")


class Stats(unittest.TestCase):
    NOW = __import__("datetime").datetime(2026, 10, 27, 3, 41, 7,
                                          tzinfo=__import__("datetime").timezone.utc)

    def build(self, api=None):
        warnings = []
        result = stats.build(LISTINGS, api or FakeApi(), fetch_json=crates_io, now=self.NOW,
                             warn=warnings.append)
        return result, warnings

    def test_downloads_per_version(self):
        result, warnings = self.build()
        self.assertEqual(warnings, [])
        self.assertEqual(result["schema_version"], 1)
        self.assertEqual(result["generated_at"], "2026-10-27T03:41:07Z")
        packages = result["packages"]
        self.assertEqual(sorted(packages),
                         ["dot", "dusk", "mermaid", "status", "textcase", "wordplay"])
        # A release asset is counted; the raw URL of 0.1.0 is not.
        self.assertEqual(packages["textcase"]["downloads"]["versions"]["0.2.0"], 17)
        self.assertIsNone(packages["textcase"]["downloads"]["versions"]["0.1.0"])
        self.assertEqual(packages["textcase"]["downloads"]["total"], 17)
        # A release not made yet, and a raw-URL pack: not counted at all.
        self.assertEqual(packages["wordplay"]["downloads"], {"total": None, "versions": {"0.1.0": None}})
        self.assertEqual(packages["status"]["downloads"]["total"], None)
        # A crate: crates.io's count for each listed version.
        self.assertEqual(packages["mermaid"]["downloads"], {"total": 20, "versions": {"0.0.8": 20}})
        self.assertEqual(packages["dot"]["downloads"]["versions"]["0.0.5"], 25)
        self.assertEqual(stats.validate(result, "auto"), [])

    def test_reviews_only_from_the_bots_discussions_in_the_category(self):
        packages = self.build()[0]["packages"]
        self.assertEqual(packages["textcase"]["reviews"], {
            "url": f"https://github.com/{common.REPOSITORY}/discussions/3",
            "up": 4, "down": 1, "comments": 2})
        self.assertEqual(packages["status"]["reviews"]["up"], 0)
        # Someone else's "dusk" and a "mermaid" in General are not reviews.
        self.assertIsNone(packages["dusk"]["reviews"])
        self.assertIsNone(packages["mermaid"]["reviews"])

    def test_without_discussions_reviews_are_null_with_a_warning(self):
        result, warnings = self.build(FakeApi("github-discussions-off.json"))
        self.assertTrue(all(p["reviews"] is None for p in result["packages"].values()))
        self.assertIn("Discussions are off", warnings[0])

    def test_a_wrong_total_is_caught(self):
        result = self.build()[0]
        result["packages"]["textcase"]["downloads"]["total"] = 3
        self.assertIn("textcase: total 3 is not 17", stats.validate(result, "off"))

    def test_the_schema_matches_the_output(self):
        try:
            import jsonschema  # noqa: F401
        except ImportError:
            self.skipTest("jsonschema is not installed")
        result = self.build()[0]
        self.assertEqual(stats.validate(result, "require"), [])
        result["packages"]["textcase"]["downloads"]["versions"]["0.2.0"] = -1
        result["packages"]["textcase"]["reviews"]["up"] = "many"
        self.assertEqual(len(stats.validate(result, "require")), 3)

    def test_crates_io_pages(self):
        pages = {
            f"{stats.CRATES_IO}c/versions": {"versions": [{"num": "1.0.0", "downloads": 3}],
                                             "meta": {"next_page": "?page=2"}},
            f"{stats.CRATES_IO}c/versions?page=2": {"versions": [{"num": "0.9.0", "downloads": 1}],
                                                    "meta": {"next_page": None}},
        }
        self.assertEqual(stats.crate_counts("c", pages.__getitem__), {"1.0.0": 3, "0.9.0": 1})


class Discussions(unittest.TestCase):
    def test_opens_one_per_package_without_one(self):
        api = FakeApi()
        out = []
        opened = discussions.open_missing(api, LISTINGS, False, out.append)
        listed = sorted(p.stem for p in (LISTINGS / "packages").glob("*.json"))
        # textcase and status have theirs; someone else's dusk does not count.
        self.assertEqual(opened, [i for i in listed if i not in ("textcase", "status")])
        self.assertIn("dusk", opened)
        created = {c["title"]: c for c in api.created}
        self.assertEqual(created["dusk"]["category"], "DIC_reviews")
        self.assertEqual(created["dusk"]["repository"], "R_marketplace")
        body = created["wordplay"]["body"]
        self.assertIn("rich plugin install wordplay", body)
        self.assertIn("👍 / 👎 this post to rate it; comment to review", body)
        self.assertIn("cargo add rs-rich-mermaid@=", created["mermaid"]["body"])
        self.assertIn("--features plugin", created["dot"]["body"])

    def test_a_dry_run_opens_nothing(self):
        api = FakeApi()
        out = []
        self.assertEqual(discussions.open_missing(api, LISTINGS, True, out.append), [])
        self.assertEqual(api.created, [])
        self.assertIn("would open a review discussion for wordplay", out)

    def test_missing_setup_says_what_to_do(self):
        with self.assertRaises(discussions.SetupMissing) as caught:
            discussions.open_missing(FakeApi("github-discussions-off.json"), LISTINGS, False)
        self.assertIn("enable Discussions", str(caught.exception))
        api = FakeApi()
        api.discussions["repository"]["discussionCategories"]["nodes"] = [
            {"id": "DIC_general", "name": "General"}]
        with self.assertRaises(discussions.SetupMissing) as caught:
            discussions.open_missing(api, LISTINGS, False)
        self.assertIn("no 'Reviews' discussion category", str(caught.exception))


if __name__ == "__main__":
    unittest.main()
