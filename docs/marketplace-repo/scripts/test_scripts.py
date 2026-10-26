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
import check_artifacts  # noqa: E402
import marketplace_common as common  # noqa: E402

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


if __name__ == "__main__":
    unittest.main()
