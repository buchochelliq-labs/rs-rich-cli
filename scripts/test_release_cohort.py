"""Tests for scripts/release_cohort.py: RELEASES.toml checks, the tag guard, and tagging."""

from pathlib import Path
import os
import subprocess
import sys
import tempfile
import unittest
from unittest import mock

import release_cohort as cohort

ROOT = Path(__file__).resolve().parent.parent

WORKSPACE = """\
[workspace]
members = ["crates/rich", "crates/rich-art"]
[workspace.dependencies]
rich = { package = "rs-rich", path = "crates/rich", version = "0.1.0" }
"""
RICH = '[package]\nname = "rs-rich"\nversion = "0.1.0"\n'
ART = '[package]\nname = "rs-rich-art"\nversion = "0.2.0"\n[dependencies]\nrich.workspace = true\n'
PYPROJECT = '[project]\nname = "rs-rich"\nversion = "0.3.0"\n'
PYCARGO = '[package]\nname = "rs-rich-py"\nversion = "0.3.0"\n'
CONFIG = """\
[[package]]
name = "rs-rich"
registry = "crates.io"
manifest = "crates/rich/Cargo.toml"
version = "0.1.0"

[[package]]
name = "rs-rich-art"
registry = "crates.io"
manifest = "crates/rich-art/Cargo.toml"
version = "0.2.0"

[[package]]
name = "rs-rich"
registry = "pypi"
manifest = "crates/rich-py/pyproject.toml"
version = "0.3.0"
"""


def write_tree(root, config=CONFIG, art=ART):
    files = {
        "Cargo.toml": WORKSPACE,
        "crates/rich/Cargo.toml": RICH,
        "crates/rich-art/Cargo.toml": art,
        "crates/rich-py/pyproject.toml": PYPROJECT,
        "crates/rich-py/Cargo.toml": PYCARGO,
        "RELEASES.toml": config,
    }
    for path, text in files.items():
        (root / path).parent.mkdir(parents=True, exist_ok=True)
        (root / path).write_text(text, encoding="utf-8")


def git(root, *args):
    return subprocess.run(["git", *args], cwd=root, check=True, text=True, capture_output=True).stdout.strip()


class RepositoryConfig(unittest.TestCase):
    def test_the_repository_config_matches_its_manifests(self):
        self.assertEqual(cohort.check(cohort.files_on_disk(ROOT)), [])

    def test_every_tag_is_derived_from_the_registry(self):
        packages = cohort.load(cohort.files_on_disk(ROOT))
        tags = [p.tag for p in packages]
        self.assertIn("rs-rich-cli-v" + next(p.version for p in packages if p.name == "rs-rich-cli"), tags)
        self.assertEqual(sum(t.startswith("python-v") for t in tags), 1)

    def test_tag_reads_committed_manifests_as_utf8_whatever_the_locale(self):
        # `tag` reads manifests with `git show`. Under a non-UTF-8 locale
        # (Windows' cp1252, or C here) text mode decoded them with that
        # encoding and failed on the non-ASCII in rs-rich's manifest.
        script = ("import sys, release_cohort as c; "
                  "sys.exit('\\n'.join(c.check(c.files_at('HEAD'))) or 0)")
        env = {**os.environ, "LC_ALL": "C", "LANG": "C", "PYTHONUTF8": "0", "PYTHONCOERCECLOCALE": "0"}
        result = subprocess.run([sys.executable, "-c", script], cwd=ROOT / "scripts", env=env,
                                capture_output=True, text=True, encoding="utf-8")
        self.assertEqual(result.returncode, 0, result.stderr)


class Check(unittest.TestCase):
    def setUp(self):
        self.dir = tempfile.TemporaryDirectory()
        self.root = Path(self.dir.name)

    def tearDown(self):
        self.dir.cleanup()

    def errors(self, **tree):
        write_tree(self.root, **tree)
        return cohort.check(cohort.files_on_disk(self.root))

    def test_a_matching_config_passes(self):
        self.assertEqual(self.errors(), [])

    def test_a_version_that_drifts_from_its_manifest_fails(self):
        errors = self.errors(art=ART.replace("0.2.0", "0.2.1"))
        self.assertTrue(any("rs-rich-art" in e and "0.2.1" in e for e in errors), errors)

    def test_a_crate_before_its_dependency_fails(self):
        first, art, py = CONFIG.split("\n\n")
        errors = self.errors(config="\n\n".join([art, first, py]))
        self.assertIn("rs-rich-art is listed before rs-rich, which it depends on", errors)

    def test_a_missing_crate_fails(self):
        errors = self.errors(config=CONFIG.split("\n\n", 1)[1])
        self.assertTrue(any("rs-rich: a publishable workspace crate missing" in e for e in errors), errors)

    def test_the_pypi_cargo_manifest_must_agree(self):
        write_tree(self.root)
        (self.root / "crates/rich-py/Cargo.toml").write_text(PYCARGO.replace("0.3.0", "0.3.1"))
        errors = cohort.check(cohort.files_on_disk(self.root))
        self.assertTrue(any("crates/rich-py/Cargo.toml says 0.3.1" in e for e in errors), errors)

    def test_the_pypi_name_must_be_the_project_published(self):
        errors = self.errors(config=CONFIG.replace('name = "rs-rich"\nregistry = "pypi"',
                                                   'name = "rs-rich-typo"\nregistry = "pypi"'))
        self.assertTrue(any("names the PyPI project 'rs-rich'" in e for e in errors), errors)

    def test_sync_rewrites_versions_from_the_manifests(self):
        write_tree(self.root, art=ART.replace("0.2.0", "0.2.5"))
        changed = cohort.sync(self.root / "RELEASES.toml")
        self.assertEqual(changed, ["rs-rich-art (crates.io) -> 0.2.5"])
        self.assertEqual(cohort.check(cohort.files_on_disk(self.root)), [])


class Unreleased(unittest.TestCase):
    packages = cohort.load(cohort.files_on_disk(ROOT))

    def status(self, published):
        return lambda package: 200 if (package.registry, package.name) in published else 404

    def cli(self):
        return next(p for p in self.packages if p.name == "rs-rich-cli")

    def test_an_unpublished_crate_tag_passes(self):
        cohort.unreleased(self.cli().tag, self.packages, self.status(set()))

    def test_a_published_crate_tag_fails(self):
        with self.assertRaisesRegex(RuntimeError, "already released"):
            cohort.unreleased(self.cli().tag, self.packages, self.status({("crates.io", "rs-rich-cli")}))

    def test_a_published_python_tag_fails(self):
        python = next(p for p in self.packages if p.registry == "pypi")
        with self.assertRaisesRegex(RuntimeError, "already released"):
            cohort.unreleased(python.tag, self.packages, self.status({("pypi", "rs-rich")}))

    def test_the_crate_and_pypi_packages_named_rs_rich_are_distinct(self):
        python = next(p for p in self.packages if p.registry == "pypi")
        cohort.unreleased(python.tag, self.packages, self.status({("crates.io", "rs-rich")}))

    def test_a_tag_naming_another_version_fails(self):
        with self.assertRaisesRegex(ValueError, "not 9.9.9"):
            cohort.unreleased("rs-rich-cli-v9.9.9", self.packages, self.status(set()))

    def test_an_unknown_registry_answer_fails(self):
        with self.assertRaisesRegex(RuntimeError, "HTTP 503"):
            cohort.unreleased(self.cli().tag, self.packages, lambda package: 503)

    def test_both_release_workflows_run_the_guard_before_anything_else(self):
        crates = (ROOT / ".github/workflows/release.yml").read_text()
        self.assertLess(crates.index("release_cohort.py unreleased"), crates.index("uses: ./.github/workflows/ci.yml"))
        pypi = (ROOT / ".github/workflows/pypi-release.yml").read_text()
        self.assertLess(pypi.index("release_cohort.py unreleased"), pypi.index("  wheels:"))

    def test_the_release_tests_install_what_ci_tests_install(self):
        # pypi-release.yml runs the same suite as python.yml against the
        # built wheel; a dependency only CI installs (Pillow, for the micro
        # page) fails the release after every wheel is built.
        def test_deps(workflow):
            text = (ROOT / ".github/workflows" / workflow).read_text()
            line = next(l for l in text.splitlines() if "pip install" in l and "pytest" in l)
            return set(line.split("pip install", 1)[1].split()[1:])  # all but the package itself
        self.assertLessEqual(test_deps("python.yml"), test_deps("pypi-release.yml"))


class Tag(unittest.TestCase):
    """`tag` against a throwaway repository and its bare `origin`."""

    def setUp(self):
        self.dir = tempfile.TemporaryDirectory()
        base = Path(self.dir.name)
        self.remote, self.work = base / "remote.git", base / "work"
        git(base, "init", "--quiet", "--bare", str(self.remote))
        self.work.mkdir()
        git(self.work, "init", "--quiet", "-b", "main")
        git(self.work, "config", "user.email", "test@example.com")
        git(self.work, "config", "user.name", "Test")
        write_tree(self.work)
        git(self.work, "add", ".")
        git(self.work, "commit", "--quiet", "-m", "release")
        git(self.work, "remote", "add", "origin", str(self.remote))
        git(self.work, "push", "--quiet", "origin", "main")
        self.sha = git(self.work, "rev-parse", "HEAD")
        patcher = mock.patch.object(cohort, "ROOT", self.work)
        patcher.start()
        self.addCleanup(patcher.stop)
        self.published = {("crates.io", "rs-rich")}
        self.waited = []

    def tearDown(self):
        self.dir.cleanup()

    def status(self, package):
        published = {(package.registry, package.name), (package.registry, package.name, package.version)}
        return 200 if published & self.published else 404

    def wait(self, package, timeout, status):
        self.waited.append(package.tag)
        self.published.add((package.registry, package.name))

    def remote_tags(self):
        return sorted(cohort.remote_tags("origin"))

    def run_tag(self, **options):
        cohort.tag(options.get("commit", "HEAD"), "origin", 60, options.get("dry_run", False),
                   status=self.status, wait=self.wait, registry=options.get("registry"),
                   run=options.get("run", lambda package: None), start=options.get("start"),
                   release_wanted=options.get("release"))

    def test_tags_only_what_is_unpublished_in_order_and_waits_for_each(self):
        self.run_tag()
        self.assertEqual(self.waited, ["rs-rich-art-v0.2.0", "python-v0.3.0"])
        self.assertEqual(self.remote_tags(), ["python-v0.3.0", "rs-rich-art-v0.2.0"])
        self.assertEqual(git(self.work, "cat-file", "-t", "rs-rich-art-v0.2.0"), "tag")
        self.assertEqual(git(self.work, "rev-parse", "rs-rich-art-v0.2.0^{commit}"), self.sha)

    def test_a_registry_tags_only_its_packages(self):
        self.run_tag(registry="crates.io")
        self.assertEqual(self.waited, ["rs-rich-art-v0.2.0"])
        self.assertEqual(self.remote_tags(), ["rs-rich-art-v0.2.0"])

    def test_a_second_run_tags_nothing(self):
        self.run_tag()
        self.waited.clear()
        self.run_tag()
        self.assertEqual(self.waited, [])

    def test_from_leaves_the_packages_before_it(self):
        self.run_tag(start="python-v0.3.0")
        self.assertEqual(self.waited, ["python-v0.3.0"])
        self.assertEqual(self.remote_tags(), ["python-v0.3.0"])

    def test_from_takes_a_crate_name(self):
        self.run_tag(start="rs-rich-art")
        self.assertEqual(self.waited, ["rs-rich-art-v0.2.0", "python-v0.3.0"])

    def test_from_refuses_to_leave_an_unpublished_crate_a_later_one_needs(self):
        # rs-rich-art depends on rs-rich: starting after an unpublished
        # rs-rich would make rs-rich-art's publish fail.
        self.published.clear()
        with self.assertRaisesRegex(RuntimeError, r"rs-rich-art needs rs-rich@0\.1\.0"):
            self.run_tag(start="rs-rich-art")
        self.assertEqual(self.remote_tags(), [])

    def test_from_an_unknown_package_is_refused(self):
        with self.assertRaisesRegex(ValueError, "no RELEASES.toml entry"):
            self.run_tag(start="rs-rich-nope")

    def test_a_dry_run_tags_nothing(self):
        self.run_tag(dry_run=True)
        self.assertEqual(self.remote_tags(), [])
        self.assertEqual(self.waited, [])

    def test_an_existing_unpublished_tag_is_waited_on_not_recreated(self):
        git(self.work, "tag", "-a", "rs-rich-art-v0.2.0", "-m", "x")
        git(self.work, "push", "--quiet", "origin", "rs-rich-art-v0.2.0")
        self.run_tag()
        self.assertEqual(self.waited, ["rs-rich-art-v0.2.0", "python-v0.3.0"])

    def tag_a_later_commit(self, change=True):
        """rs-rich-art's tag on a later commit of main, its version unpublished;
        with `change`, that commit also changes what rs-rich-art ships."""
        if change:
            (self.work / "crates/rich-art/src").mkdir(parents=True, exist_ok=True)
            (self.work / "crates/rich-art/src/lib.rs").write_text("// later\n")
            git(self.work, "add", ".")
        git(self.work, "commit", "--quiet", "--allow-empty", "-m", "later")
        git(self.work, "push", "--quiet", "origin", "main")
        git(self.work, "tag", "-a", "rs-rich-art-v0.2.0", "-m", "x")
        git(self.work, "push", "--quiet", "origin", "rs-rich-art-v0.2.0")
        return git(self.work, "rev-parse", "HEAD")

    def run_on(self, commit, status="completed", conclusion="cancelled", publish_started=False):
        return lambda package: {"status": status, "conclusion": conclusion, "url": "https://example/run",
                                "head_sha": commit, "publish_started": publish_started}

    def test_a_tag_with_a_run_going_on_another_commit_that_ships_other_files_stops(self):
        later = self.tag_a_later_commit()
        with self.assertRaisesRegex(RuntimeError, rf"tagged at {later[:12]}: rs-rich-art@0\.2\.0.*src/lib\.rs"):
            self.run_tag(commit=self.sha, run=self.run_on(later, status="in_progress", conclusion=""))
        self.assertEqual(cohort.remote_tags("origin")["rs-rich-art-v0.2.0"], later)

    def test_a_tag_with_a_run_going_on_another_commit_that_ships_the_same_files_is_waited_on(self):
        later = self.tag_a_later_commit(change=False)
        self.run_tag(commit=self.sha, run=self.run_on(later, status="in_progress", conclusion=""))
        self.assertEqual(cohort.remote_tags("origin")["rs-rich-art-v0.2.0"], later)
        self.assertEqual(self.waited, ["rs-rich-art-v0.2.0", "python-v0.3.0"])

    def test_a_tag_on_another_commit_with_no_run_says_how_to_clear_it(self):
        self.tag_a_later_commit()
        with self.assertRaisesRegex(RuntimeError, r"git push origin :refs/tags/"):
            self.run_tag(commit=self.sha)

    def test_a_tag_whose_run_was_cancelled_is_moved_to_this_commit(self):
        # The cancelled run never started publishing, so the tag is taken
        # back and put on the commit being released, locally and remotely.
        later = self.tag_a_later_commit()
        self.run_tag(commit=self.sha, run=self.run_on(later))
        self.assertEqual(cohort.remote_tags("origin")["rs-rich-art-v0.2.0"], self.sha)
        self.assertEqual(git(self.work, "rev-parse", "rs-rich-art-v0.2.0^{commit}"), self.sha)
        self.assertEqual(self.waited, ["rs-rich-art-v0.2.0", "python-v0.3.0"])

    def test_a_cancelled_run_that_started_publishing_holds_the_release_to_its_commit(self):
        # The upload may have landed before the cancel; the registry can
        # take a while to show it, so the tag stays.
        later = self.tag_a_later_commit()
        with self.assertRaisesRegex(RuntimeError, f"tagged at {later[:12]}"):
            self.run_tag(commit=self.sha, run=self.run_on(later, publish_started=True))
        self.assertEqual(cohort.remote_tags("origin")["rs-rich-art-v0.2.0"], later)

    def test_a_failed_run_of_another_commit_does_not_condemn_the_tag(self):
        # The latest run seen is an older one, of the tag before it moved:
        # it says nothing about the tag as it is now.
        later = self.tag_a_later_commit()
        with self.assertRaisesRegex(RuntimeError, f"tagged at {later[:12]}"):
            self.run_tag(commit=self.sha, run=self.run_on(self.sha, conclusion="failure"))
        self.assertEqual(cohort.remote_tags("origin")["rs-rich-art-v0.2.0"], later)

    def test_a_dry_run_only_says_it_would_move_a_dead_tag(self):
        later = self.tag_a_later_commit()
        self.run_tag(commit=self.sha, dry_run=True, run=self.run_on(later, conclusion="failure"))
        self.assertEqual(cohort.remote_tags("origin")["rs-rich-art-v0.2.0"], later)

    def test_a_leftover_local_tag_on_another_commit_is_replaced(self):
        # A remote tag deleted by hand leaves the local one behind, on its
        # old commit; it was never released, so the run replaces it.
        git(self.work, "commit", "--quiet", "--allow-empty", "-m", "later")
        git(self.work, "push", "--quiet", "origin", "main")
        git(self.work, "tag", "-a", "rs-rich-art-v0.2.0", "-m", "x")
        self.run_tag(commit=self.sha)
        self.assertEqual(git(self.work, "rev-parse", "rs-rich-art-v0.2.0^{commit}"), self.sha)
        self.assertEqual(cohort.remote_tags("origin")["rs-rich-art-v0.2.0"], self.sha)

    def test_it_tags_the_local_checkout_not_a_newer_main(self):
        # main moves on (a later version of rs-rich-art) in another clone; a
        # run from this checkout, which has not pulled, still releases what
        # it has: nothing is fetched.
        other = Path(self.dir.name) / "other"
        git(Path(self.dir.name), "clone", "--quiet", "-b", "main", str(self.remote), str(other))
        git(other, "config", "user.email", "test@example.com")
        git(other, "config", "user.name", "Test")
        write_tree(other, config=CONFIG.replace('version = "0.2.0"', 'version = "0.2.1"'),
                   art=ART.replace("0.2.0", "0.2.1"))
        git(other, "commit", "--quiet", "-am", "later")
        git(other, "push", "--quiet", "origin", "main")
        tag = cohort.tag
        with mock.patch.object(sys, "argv", ["release_cohort.py", "tag", "--registry", "crates.io"]), \
                mock.patch.object(cohort, "tag", lambda *a, **k: tag(*a, status=self.status, wait=self.wait, **k)):
            self.assertEqual(cohort.main(), 0)
        self.assertEqual(self.waited, ["rs-rich-art-v0.2.0"])
        self.assertEqual(git(self.work, "rev-parse", "rs-rich-art-v0.2.0^{commit}"), self.sha)
        # Nothing was fetched: the checkout's view of main is where it was.
        self.assertEqual(git(self.work, "rev-parse", "origin/main"), self.sha)

    def test_an_earlier_commit_on_main_is_released_as_it_was(self):
        # main has moved on (rs-rich-art 0.2.1); --commit picks the earlier,
        # known-good commit, and its RELEASES.toml and manifests decide.
        write_tree(self.work, config=CONFIG.replace('version = "0.2.0"', 'version = "0.2.1"'),
                   art=ART.replace("0.2.0", "0.2.1"))
        git(self.work, "commit", "--quiet", "-am", "later")
        git(self.work, "push", "--quiet", "origin", "main")
        self.run_tag(commit=self.sha, registry="crates.io")
        self.assertEqual(self.waited, ["rs-rich-art-v0.2.0"])
        self.assertEqual(git(self.work, "rev-parse", "rs-rich-art-v0.2.0^{commit}"), self.sha)

    def publish_rs_rich_here(self):
        """rs-rich 0.1.0, published, tagged on this commit."""
        git(self.work, "tag", "-a", "rs-rich-v0.1.0", "-m", "x", self.sha)
        git(self.work, "push", "--quiet", "origin", "rs-rich-v0.1.0")

    def test_a_published_crate_changed_under_its_version_stops_the_cohort(self):
        # The split cohort: rs-rich 0.1.0 went out from this commit, and a
        # later one changes it without a bump. Releasing the later commit
        # would publish the rest beside an rs-rich of other code.
        self.publish_rs_rich_here()
        (self.work / "crates/rich/src").mkdir(parents=True)
        (self.work / "crates/rich/src/lib.rs").write_text("// changed\n")
        git(self.work, "add", ".")
        git(self.work, "commit", "--quiet", "-m", "change rs-rich without a bump")
        git(self.work, "push", "--quiet", "origin", "main")
        with self.assertRaisesRegex(RuntimeError, rf"rs-rich@0\.1\.0.*src/lib\.rs(.|\n)*--commit {self.sha[:12]}"):
            self.run_tag()
        self.assertEqual(self.remote_tags(), ["rs-rich-v0.1.0"])

    def test_a_published_crate_left_alone_does_not_stop_the_cohort(self):
        self.publish_rs_rich_here()
        (self.work / "NOTES.md").write_text("elsewhere\n")
        git(self.work, "add", ".")
        git(self.work, "commit", "--quiet", "-m", "outside the crate")
        git(self.work, "push", "--quiet", "origin", "main")
        self.run_tag(registry="crates.io")
        self.assertEqual(self.waited, ["rs-rich-art-v0.2.0"])

    def change_rs_rich(self, message="change rs-rich without a bump"):
        (self.work / "crates/rich/src").mkdir(parents=True, exist_ok=True)
        (self.work / "crates/rich/src/lib.rs").write_text(message + "\n")
        git(self.work, "add", ".")
        git(self.work, "commit", "--quiet", "-m", message)
        git(self.work, "push", "--quiet", "origin", "main")
        return git(self.work, "rev-parse", "HEAD")

    def bump_art(self):
        """main moves on to the next release: rs-rich-art (the last crate) 0.2.1."""
        write_tree(self.work, config=CONFIG.replace('version = "0.2.0"', 'version = "0.2.1"'),
                   art=ART.replace("0.2.0", "0.2.1"))
        git(self.work, "commit", "--quiet", "-am", "next release")
        git(self.work, "push", "--quiet", "origin", "main")
        return git(self.work, "rev-parse", "HEAD")

    def test_a_release_in_progress_is_resumed_on_the_commit_it_started_from(self):
        # The split release, prevented: rs-rich went out from the first
        # commit; main has moved on and changed it. With no --commit, the
        # rest of the release is tagged where it started, not at HEAD.
        self.publish_rs_rich_here()
        self.change_rs_rich()
        self.run_tag(commit=None)
        self.assertEqual(self.waited, ["rs-rich-art-v0.2.0", "python-v0.3.0"])
        self.assertEqual(git(self.work, "rev-parse", "rs-rich-art-v0.2.0^{commit}"), self.sha)
        self.assertEqual(git(self.work, "rev-parse", "python-v0.3.0^{commit}"), self.sha)

    def test_with_nothing_in_progress_the_checkout_is_released(self):
        head = self.change_rs_rich("unreleased work")
        self.published.clear()
        self.run_tag(commit=None, registry="crates.io")
        self.assertEqual(git(self.work, "rev-parse", "rs-rich-art-v0.2.0^{commit}"), head)

    def test_a_finished_release_is_not_resumed(self):
        # Everything of release 0.2.0 is out; HEAD names 0.2.1, and is released.
        self.publish_rs_rich_here()
        self.published |= {("crates.io", "rs-rich-art", "0.2.0"), ("pypi", "rs-rich")}
        git(self.work, "tag", "-a", "rs-rich-art-v0.2.0", "-m", "x", self.sha)
        git(self.work, "push", "--quiet", "origin", "rs-rich-art-v0.2.0")
        head = self.bump_art()
        self.run_tag(commit=None, registry="crates.io")
        self.assertEqual(git(self.work, "rev-parse", "rs-rich-art-v0.2.1^{commit}"), head)

    def test_release_finds_the_last_commit_on_main_naming_it(self):
        self.bump_art()
        self.run_tag(commit=None, release="v0.2.0", registry="crates.io")
        self.assertEqual(self.waited, ["rs-rich-art-v0.2.0"])
        self.assertEqual(git(self.work, "rev-parse", "rs-rich-art-v0.2.0^{commit}"), self.sha)

    def test_release_takes_the_commit_its_tags_are_on(self):
        self.publish_rs_rich_here()
        self.change_rs_rich("not a bump")
        self.run_tag(commit=None, release="0.2.0", registry="crates.io")
        self.assertEqual(git(self.work, "rev-parse", "rs-rich-art-v0.2.0^{commit}"), self.sha)

    def test_an_unknown_release_is_refused(self):
        with self.assertRaisesRegex(RuntimeError, "names release 9.9.9"):
            self.run_tag(commit=None, release="9.9.9")
        self.assertEqual(self.remote_tags(), [])

    def test_a_tag_records_its_release_and_commit(self):
        self.run_tag(registry="crates.io")
        self.assertEqual(cohort.tag_trailers("rs-rich-art-v0.2.0"), {"Release": "0.2.0", "Commit": self.sha})

    def test_same_commit_passes_a_tag_on_its_release_commit(self):
        self.publish_rs_rich_here()
        self.run_tag(registry="crates.io")
        summary = cohort.same_commit("rs-rich-art-v0.2.0", "origin")
        self.assertIn(f"-> {self.sha} (release 0.2.0)", summary)
        self.assertIn("rs-rich-v0.1.0", summary)

    def test_same_commit_refuses_a_tag_that_splits_its_release(self):
        # What release.yml runs: rs-rich-art tagged on a commit that ships an
        # rs-rich other than the one released under rs-rich 0.1.0.
        self.publish_rs_rich_here()
        later = self.change_rs_rich()
        git(self.work, "tag", "-a", "rs-rich-art-v0.2.0", "-m", "x", later)
        with self.assertRaisesRegex(RuntimeError, r"would split release 0\.2\.0(.|\n)*rs-rich@0\.1\.0"):
            cohort.same_commit("rs-rich-art-v0.2.0", "origin")

    def test_same_commit_refuses_a_tag_moved_from_its_recorded_commit(self):
        later = self.change_rs_rich("elsewhere")
        git(self.work, "tag", "-a", "rs-rich-art-v0.2.0", "-m", f"rs-rich-art 0.2.0\n\nCommit: {self.sha}", later)
        with self.assertRaisesRegex(RuntimeError, "records commit"):
            cohort.same_commit("rs-rich-art-v0.2.0", "origin")

    def test_both_release_workflows_refuse_a_split_release_before_building(self):
        crates = (ROOT / ".github/workflows/release.yml").read_text()
        self.assertLess(crates.index("release_cohort.py same-commit"), crates.index("uses: ./.github/workflows/ci.yml"))
        pypi = (ROOT / ".github/workflows/pypi-release.yml").read_text()
        self.assertLess(pypi.index("release_cohort.py same-commit"), pypi.index("  wheels:"))

    def test_cargo_globs(self):
        cases = [("src/style.rs", "src/**/*", True), ("src/a/b.rs", "src/**/*", True),
                 ("tests/t.rs", "src/**/*", False), ("README.md", "README.md", True),
                 ("docs/README.md", "README.md", True), ("Cargo.toml", "Cargo.toml", True)]
        for path, pattern, expected in cases:
            self.assertEqual(cohort.glob_match(path, pattern), expected, (path, pattern))

    def test_a_commit_not_on_main_is_refused(self):
        git(self.work, "commit", "--quiet", "--allow-empty", "-m", "unmerged")
        with self.assertRaisesRegex(RuntimeError, "not on origin/main"):
            self.run_tag(commit="HEAD")
        self.assertEqual(self.remote_tags(), [])

    def test_a_config_that_disagrees_is_refused_before_tagging(self):
        (self.work / "crates/rich-art/Cargo.toml").write_text(ART.replace("0.2.0", "0.2.1"))
        git(self.work, "commit", "--quiet", "-am", "bump without sync")
        git(self.work, "push", "--quiet", "origin", "main")
        with self.assertRaisesRegex(RuntimeError, "disagrees"):
            self.run_tag()
        self.assertEqual(self.remote_tags(), [])

    def test_a_published_version_still_waits_for_its_run_to_finish(self):
        package = cohort.Package("rs-rich-art", "crates.io", "x", "0.2.0")
        runs = iter([("in_progress", "", "u"), ("in_progress", "", "u"), ("completed", "success", "u")])
        sleeps = []
        cohort.wait_for(package, 60, status=lambda p: 200, conclusion=lambda p: next(runs),
                        sleep=sleeps.append)
        self.assertEqual(len(sleeps), 2)

    def test_a_run_that_fails_after_uploading_stops_the_wait(self):
        package = cohort.Package("rs-rich-art", "crates.io", "x", "0.2.0")
        with self.assertRaisesRegex(RuntimeError, "ended failure"):
            cohort.wait_for(package, 60, status=lambda p: 200,
                            conclusion=lambda p: ("completed", "failure", "https://example/run"),
                            sleep=lambda s: None)

    def test_without_gh_the_registry_decides(self):
        package = cohort.Package("rs-rich-art", "crates.io", "x", "0.2.0")
        cohort.wait_for(package, 60, status=lambda p: 200, conclusion=lambda p: None, sleep=lambda s: None)

    def test_a_failed_release_run_stops_the_wait(self):
        package = cohort.Package("rs-rich-art", "crates.io", "x", "0.2.0")
        with self.assertRaisesRegex(RuntimeError, "ended failure"):
            cohort.wait_for(package, 60, status=lambda p: 404,
                            conclusion=lambda p: ("completed", "failure", "https://example/run"),
                            sleep=lambda s: None)


if __name__ == "__main__":
    unittest.main()
