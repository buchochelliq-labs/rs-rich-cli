"""Tests for scripts/build_versioned_docs.py's playground download: a release's
module goes into its docs when the release has one, and nothing breaks when it
does not."""

import io
import json
import sys
import tarfile
import tempfile
import unittest
import urllib.error
from pathlib import Path
from unittest import mock

sys.path.insert(0, str(Path(__file__).resolve().parent))
import build_versioned_docs as docs  # noqa: E402

REPO = "https://github.com/example/rs-rich-cli"
STAMP = {"format": 1, "modules": {"lean": {"js": "lean/rich_playground.js", "kinds": ["markup"]}}}


def archive(files):
    """A .tar.gz of {name: bytes}, or of TarInfo-making callables."""
    buffer = io.BytesIO()
    with tarfile.open(fileobj=buffer, mode="w:gz") as tar:
        for name, data in files.items():
            if callable(data):
                tar.addfile(data())
                continue
            info = tarfile.TarInfo(name)
            info.size = len(data)
            tar.addfile(info, io.BytesIO(data))
    return buffer.getvalue()


def good_archive():
    return archive({
        "playground.json": json.dumps(STAMP).encode(),
        "lean/rich_playground.js": b"export default 1;",
        "lean/rich_playground_bg.wasm": b"\0asm",
    })


class AddPlayground(unittest.TestCase):
    def setUp(self):
        self.scratch = tempfile.TemporaryDirectory()
        self.site = Path(self.scratch.name) / "0.0.21"
        (self.site / "assets").mkdir(parents=True)
        (self.site / "assets" / "playground.js").write_text("// the page script")

    def tearDown(self):
        self.scratch.cleanup()

    def add(self, fetch):
        with mock.patch("sys.stdout", new=io.StringIO()):
            return docs.add_playground(self.site, "0.0.21", REPO, fetch=fetch)

    def test_url_is_the_release_tag_s_asset(self):
        self.assertEqual(
            docs.playground_url(REPO, "0.0.21"),
            f"{REPO}/releases/download/release-0.0.21/playground-0.0.21.tar.gz")

    def test_release_archive_is_unpacked_into_the_version(self):
        urls = []
        self.assertTrue(self.add(lambda url: urls.append(url) or good_archive()))
        self.assertEqual(urls, [docs.playground_url(REPO, "0.0.21")])
        target = self.site / "assets" / "playground"
        self.assertEqual(json.loads((target / "playground.json").read_text()), STAMP)
        self.assertTrue((target / "lean" / "rich_playground_bg.wasm").exists())

    def test_release_without_archive_keeps_plain_blocks(self):
        self.assertFalse(self.add(lambda url: None))
        self.assertFalse((self.site / "assets" / "playground").exists())

    def test_docs_without_the_script_download_nothing(self):
        (self.site / "assets" / "playground.js").unlink()
        self.assertFalse(self.add(lambda url: self.fail("fetched for docs without the playground")))

    def test_network_failure_is_a_warning_not_a_failed_site(self):
        def fail(url):
            raise urllib.error.URLError("offline")
        self.assertFalse(self.add(fail))
        self.assertFalse((self.site / "assets" / "playground").exists())

    def test_archive_escaping_its_directory_is_rejected(self):
        bad = archive({"../../evil.js": b"alert(1)", "playground.json": json.dumps(STAMP).encode()})
        self.assertFalse(self.add(lambda url: bad))
        self.assertFalse((self.site / "assets" / "playground").exists())
        self.assertFalse((Path(self.scratch.name) / "evil.js").exists())

    def test_link_out_of_the_directory_is_rejected(self):
        def link():
            info = tarfile.TarInfo("lean/link")
            info.type = tarfile.SYMTYPE
            info.linkname = "/etc/passwd"
            return info
        bad = archive({"playground.json": json.dumps(STAMP).encode(), "lean/link": link})
        self.assertFalse(self.add(lambda url: bad))
        self.assertFalse((self.site / "assets" / "playground").exists())

    def test_archive_without_a_stamp_is_rejected(self):
        self.assertFalse(self.add(lambda url: archive({"lean/x.js": b"1"})))
        self.assertFalse((self.site / "assets" / "playground").exists())

    def test_not_an_archive_is_rejected(self):
        self.assertFalse(self.add(lambda url: b"<html>not found</html>"))
        self.assertFalse((self.site / "assets" / "playground").exists())


class Fetch(unittest.TestCase):
    def test_404_is_none_and_other_errors_raise(self):
        def opener(status):
            def urlopen(url, timeout):
                raise urllib.error.HTTPError(url, status, "x", {}, None)
            return urlopen
        with mock.patch("urllib.request.urlopen", opener(404)):
            self.assertIsNone(docs.fetch("https://example/x"))
        with mock.patch("urllib.request.urlopen", opener(500)):
            with self.assertRaises(urllib.error.HTTPError):
                docs.fetch("https://example/x")

    def test_oversized_download_is_refused(self):
        class Response(io.BytesIO):
            def __enter__(self):
                return self

            def __exit__(self, *exc):
                return False

        big = Response(b"x" * (docs.PLAYGROUND_MAX_BYTES + 10))
        with mock.patch("urllib.request.urlopen", lambda url, timeout: big):
            with self.assertRaises(ValueError):
                docs.fetch("https://example/x")


class RepoUrl(unittest.TestCase):
    def test_reads_mkdocs_repo_url(self):
        self.assertTrue(docs.repo_url().startswith("https://github.com/"))
        self.assertFalse(docs.repo_url().endswith("/"))


if __name__ == "__main__":
    unittest.main()
