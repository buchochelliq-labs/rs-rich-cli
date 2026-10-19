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

WORKFLOW = 'on:\n  push:\n    tags: ["release-*"]\n'


def write_tree(root, config=CONFIG, art=ART):
    files = {
        "Cargo.toml": WORKSPACE,
        "crates/rich/Cargo.toml": RICH,
        "crates/rich-art/Cargo.toml": art,
        "crates/rich-py/pyproject.toml": PYPROJECT,
        "crates/rich-py/Cargo.toml": PYCARGO,
        "RELEASES.toml": config,
        ".github/workflows/release.yml": WORKFLOW,
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


class ReleaseTags(unittest.TestCase):
    """A `release-X.Y.Z` tag selects the release's crates not yet out."""

    packages = [cohort.Package("rs-rich", "crates.io", "m", "0.0.10"),
                cohort.Package("rs-rich-ext", "crates.io", "m", "0.0.18"),
                cohort.Package("rs-rich-cli", "crates.io", "m", "0.0.20"),
                cohort.Package("rs-rich", "pypi", "m", "0.0.8")]
    tags = {"rs-rich-v0.0.10": "c", "rs-rich-ext-v0.0.18": "c", "rs-rich-cli-v0.0.20": "c"}

    def status(self, *out):
        return lambda package: 200 if (package.registry, package.name) in out else 404

    def test_a_release_tag_selects_every_crate_of_its_release(self):
        self.assertEqual([p.name for p in cohort.selected_by("release-0.0.20", self.packages)],
                         ["rs-rich", "rs-rich-ext", "rs-rich-cli"])
        with self.assertRaisesRegex(ValueError, "release 0.0.20, not 0.0.19"):
            cohort.selected_by("release-0.0.19", self.packages)

    def test_the_selection_is_the_release_minus_what_is_published_in_order(self):
        due = cohort.unreleased("release-0.0.20", self.packages, self.status(("crates.io", "rs-rich")),
                                self.tags, "c")
        self.assertEqual([p.name for p in due], ["rs-rich-ext", "rs-rich-cli"])

    def test_pypi_is_never_in_a_release_tags_selection(self):
        due = cohort.unreleased("release-0.0.20", self.packages, self.status(), self.tags, "c")
        self.assertEqual({p.registry for p in due}, {"crates.io"})

    def test_an_empty_selection_is_refused(self):
        out = self.status(("crates.io", "rs-rich"), ("crates.io", "rs-rich-ext"), ("crates.io", "rs-rich-cli"))
        with self.assertRaisesRegex(RuntimeError, "nothing to publish"):
            cohort.unreleased("release-0.0.20", self.packages, out, self.tags, "c")

    def test_every_selected_crate_must_be_tagged_on_the_release_commit(self):
        tags = {**self.tags, "rs-rich-ext-v0.0.18": "elsewhere000000"}
        del tags["rs-rich-cli-v0.0.20"]
        with self.assertRaisesRegex(RuntimeError, r"rs-rich-ext-v0\.0\.18 \(at elsewhere000\), "
                                                  r"rs-rich-cli-v0\.0\.20 \(missing\)"):
            cohort.unreleased("release-0.0.20", self.packages, self.status(), tags, "c")
        # A published crate's tag may be anywhere: it is not selected.
        cohort.unreleased("release-0.0.20", self.packages[:2] + self.packages[2:],
                          self.status(("crates.io", "rs-rich-ext"), ("crates.io", "rs-rich-cli")), tags, "c")

    def test_verify_only_checks_what_the_release_tagged_on_its_commit(self):
        tags = {**self.tags, "rs-rich-v0.0.10": "older"}
        everything = self.status(("crates.io", "rs-rich"), ("crates.io", "rs-rich-ext"), ("crates.io", "rs-rich-cli"))
        chosen = cohort.published("release-0.0.20", self.packages, everything, tags, "c")
        self.assertEqual([p.name for p in chosen], ["rs-rich-ext", "rs-rich-cli"])
        with self.assertRaisesRegex(RuntimeError, "cannot complete a partial upload"):
            cohort.published("release-0.0.20", self.packages, self.status(("crates.io", "rs-rich-ext")), tags, "c")

    def test_the_selection_is_handed_on_as_json_in_order(self):
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory) / "out"
            with mock.patch.dict(os.environ, {"GITHUB_OUTPUT": str(output)}):
                cohort.write_selection(self.packages[::-1])
            self.assertEqual(output.read_text(),
                             'selection={"rs-rich-cli":"0.0.20","rs-rich-ext":"0.0.18","rs-rich":"0.0.10"}\n')


class Workflow(unittest.TestCase):
    """release.yml: one release tag, one gate, one CI, one publish."""

    text = (ROOT / ".github/workflows/release.yml").read_text()

    def test_only_a_release_tag_push_or_a_dispatch_starts_it(self):
        on = self.text[self.text.index("\non:"):self.text.index("\npermissions:")]
        self.assertIn('  push:\n    tags: ["release-*"]\n  workflow_dispatch:', on)
        self.assertEqual(on.count("tags:"), 1)
        self.assertNotIn('"v*"', on)
        self.assertNotIn("-v*", on)
        self.assertIn("      tag:", on)
        self.assertIn("      verify_only:", on)

    def test_ci_runs_once_on_the_validated_commit(self):
        self.assertEqual(self.text.count("uses: ./.github/workflows/ci.yml"), 1)
        self.assertIn("checkout_ref: ${{ needs.gate.outputs.sha }}", self.text)
        self.assertEqual(self.text.count("runs-on:"), 2)         # the gate and the publish job

    def test_the_gate_checks_the_release_before_ci(self):
        ci = self.text.index("uses: ./.github/workflows/ci.yml")
        steps = ["release.py validate-tag", "release_cohort.py check", "release_cohort.py same-commit",
                 "release_cohort.py unreleased", "release.py plan"]
        positions = [self.text.index(step) for step in steps]
        self.assertEqual(positions, sorted(positions))
        self.assertLess(positions[-1], ci)
        self.assertIn("RELEASE_SELECTION: ${{ steps.due.outputs.selection || steps.done.outputs.selection }}",
                      self.text)

    def test_one_publish_job_uploads_the_whole_selection(self):
        publish = self.text[self.text.index("  publish:"):]
        self.assertEqual(publish.count("run: python3 scripts/release.py publish\n"), 1)
        self.assertEqual(publish.count("run: python3 scripts/release.py publish --dry-run\n"), 1)
        self.assertEqual(publish.count("run: python3 scripts/release.py verify\n"), 1)
        self.assertIn("needs: [gate, ci]", publish)
        self.assertIn("RELEASE_SELECTION: ${{ needs.gate.outputs.selection }}", publish)
        # ...with one multi-package `cargo publish -p a -p b`, dry run and upload.
        with mock.patch.object(cohort.release.subprocess, "run") as run:
            cohort.release.publish({"rs-rich": "1.0.0", "rs-rich-ext": "1.0.0"}, dry_run=True)
            cohort.release.publish({"rs-rich": "1.0.0", "rs-rich-ext": "1.0.0"}, dependencies={},
                                   environ={"RELEASE_TRUSTED_TOKEN": "t"})
        self.assertEqual([c.args[0] for c in run.call_args_list], [
            ["cargo", "publish", "-p", "rs-rich", "-p", "rs-rich-ext", "--locked", "--dry-run"],
            ["cargo", "publish", "-p", "rs-rich", "-p", "rs-rich-ext", "--locked"]])

    def test_releases_never_cancel_each_other(self):
        self.assertIn("group: crates-io-release\n  cancel-in-progress: false", self.text)

    def test_pypi_does_not_start_on_a_release_tag(self):
        pypi = (ROOT / ".github/workflows/pypi-release.yml").read_text()
        self.assertIn('tags: ["python-v*"]', pypi)
        self.assertNotIn("release-*\"", pypi)


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
        self.stale = []

    def tearDown(self):
        self.dir.cleanup()

    def status(self, package):
        published = {(package.registry, package.name), (package.registry, package.name, package.version)}
        return 200 if published & self.published else 404

    def wait(self, tag_name, packages, timeout, status, stale=None):
        self.waited.append(tag_name)
        self.stale.append(stale)
        self.published |= {(p.registry, p.name) for p in packages}

    def remote_tags(self):
        return sorted(cohort.remote_tags("origin"))

    def run_tag(self, **options):
        cohort.tag(options.get("commit", "HEAD"), "origin", 60, options.get("dry_run", False),
                   status=self.status, wait=self.wait, registry=options.get("registry"),
                   run=options.get("run", lambda tag_name: None), release_wanted=options.get("release"))

    def test_tags_what_is_unpublished_and_the_release_and_waits_for_each_run(self):
        self.run_tag()
        self.assertEqual(self.waited, ["release-0.2.0", "python-v0.3.0"])
        self.assertEqual(self.remote_tags(), ["python-v0.3.0", "release-0.2.0", "rs-rich-art-v0.2.0"])
        for name in ("rs-rich-art-v0.2.0", "release-0.2.0", "python-v0.3.0"):
            self.assertEqual(git(self.work, "cat-file", "-t", name), "tag")
            self.assertEqual(git(self.work, "rev-parse", f"{name}^{{commit}}"), self.sha)

    def test_the_release_tag_is_named_by_the_release_and_records_it(self):
        self.run_tag(registry="crates.io")
        self.assertEqual(cohort.tag_trailers("release-0.2.0"), {"Release": "0.2.0", "Commit": self.sha})
        message = git(self.work, "for-each-ref", "refs/tags/release-0.2.0", "--format=%(contents)")
        self.assertTrue(message.startswith("Release 0.2.0\n\nrs-rich-art 0.2.0\n"), message)
        self.assertNotIn("rs-rich 0.1.0", message)       # already published: not in this run
        self.assertEqual(cohort.release_tag("0.0.20"), "release-0.0.20")

    def pushes(self, **options):
        """Run `tag`, returning the refs of every `git push` it made."""
        made, real = [], cohort.git

        def recording(*args, capture=True):
            if args[0] == "push":
                made.append([a for a in args[1:] if a != "origin"])
            return real(*args, capture=capture)
        with mock.patch.object(cohort, "git", recording):
            self.run_tag(**options)
        return made

    def test_record_tags_go_up_in_one_atomic_push_then_the_tags_that_start_runs(self):
        # GitHub starts no workflow for a push of more than three tags: the
        # records (which start nothing) go first, together, then the release
        # and python tags.
        self.published.clear()
        self.assertEqual(self.pushes(), [
            ["--atomic", "refs/tags/rs-rich-v0.1.0", "refs/tags/rs-rich-art-v0.2.0"],
            ["--atomic", "refs/tags/release-0.2.0", "refs/tags/python-v0.3.0"],
        ])
        self.assertEqual(self.waited, ["release-0.2.0", "python-v0.3.0"])

    def test_an_atomic_push_that_fails_pushes_nothing(self):
        # Someone else's rs-rich-art tag lands on the remote first: the record
        # push is refused whole, and no run is started.
        self.published.clear()
        other = Path(self.dir.name) / "other"
        git(Path(self.dir.name), "clone", "--quiet", "-b", "main", str(self.remote), str(other))
        git(other, "-c", "user.name=T", "-c", "user.email=t@e", "tag", "-a", "rs-rich-art-v0.2.0", "-m", "x")
        git(other, "push", "--quiet", "origin", "rs-rich-art-v0.2.0")
        real = cohort.remote_tags
        with mock.patch.object(cohort, "remote_tags", lambda remote: {}), \
                self.assertRaises(subprocess.CalledProcessError):
            self.run_tag()     # it saw the remote before the other tag landed
        self.assertEqual(sorted(real("origin")), ["rs-rich-art-v0.2.0"])
        self.assertEqual(self.waited, [])

    def test_a_registry_tags_only_its_packages(self):
        self.run_tag(registry="crates.io")
        self.assertEqual(self.waited, ["release-0.2.0"])
        self.assertEqual(self.remote_tags(), ["release-0.2.0", "rs-rich-art-v0.2.0"])

    def test_pypi_alone_leaves_the_crates_and_the_release_tag(self):
        self.run_tag(registry="pypi")
        self.assertEqual(self.waited, ["python-v0.3.0"])
        self.assertEqual(self.remote_tags(), ["python-v0.3.0"])

    def test_a_commit_whose_workflow_predates_release_tags_is_refused(self):
        # Release 0.0.19's commit has the per-crate release.yml: a release
        # tag there would start nothing.
        (self.work / ".github/workflows/release.yml").write_text(
            'on:\n  push:\n    tags: ["v*", "rs-rich-v*", "rs-rich-art-v*"]\n')
        git(self.work, "commit", "--quiet", "-am", "the old workflow")
        git(self.work, "push", "--quiet", "origin", "main")
        with self.assertRaisesRegex(RuntimeError, "predates one-run releases"):
            self.run_tag()
        self.assertEqual(self.remote_tags(), [])
        self.run_tag(registry="pypi")          # PyPI's workflow is unchanged
        self.assertEqual(self.remote_tags(), ["python-v0.3.0"])

    def test_the_repositorys_release_workflow_starts_on_release_tags(self):
        self.assertTrue(cohort.starts_on_release_tags(cohort.files_on_disk(ROOT)))

    def test_nothing_due_on_crates_io_pushes_no_release_tag(self):
        self.published.add(("crates.io", "rs-rich-art"))
        self.run_tag()
        self.assertEqual(self.remote_tags(), ["python-v0.3.0"])

    def test_a_second_run_tags_nothing(self):
        self.run_tag()
        self.waited.clear()
        self.run_tag()
        self.assertEqual(self.waited, [])

    def test_a_dry_run_tags_nothing(self):
        self.run_tag(dry_run=True)
        self.assertEqual(self.remote_tags(), [])
        self.assertEqual(self.waited, [])

    def test_an_existing_record_tag_is_kept_not_recreated(self):
        git(self.work, "tag", "-a", "rs-rich-art-v0.2.0", "-m", "by hand")
        git(self.work, "push", "--quiet", "origin", "rs-rich-art-v0.2.0")
        self.assertEqual(self.pushes(), [["--atomic", "refs/tags/release-0.2.0", "refs/tags/python-v0.3.0"]])
        self.assertEqual(self.waited, ["release-0.2.0", "python-v0.3.0"])
        self.assertEqual(git(self.work, "for-each-ref", "refs/tags/rs-rich-art-v0.2.0", "--format=%(contents)"),
                         "by hand")

    def test_a_release_whose_run_is_going_is_waited_on(self):
        self.run_tag(registry="crates.io")
        self.published.discard(("crates.io", "rs-rich-art"))
        self.waited.clear()
        going = self.run_on(self.sha, status="in_progress", conclusion="")
        self.assertEqual(self.pushes(registry="crates.io", run=going), [])
        self.assertEqual(self.waited, ["release-0.2.0"])

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
        return lambda tag_name: {"status": status, "conclusion": conclusion, "url": "https://example/run",
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
        self.assertEqual(self.waited, ["release-0.2.0", "python-v0.3.0"])

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
        self.assertEqual(self.waited, ["release-0.2.0"])
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
        self.assertEqual(self.waited, ["release-0.2.0"])
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
        self.assertEqual(self.waited, ["release-0.2.0"])

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
        self.assertEqual(self.waited, ["release-0.2.0", "python-v0.3.0"])
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
        self.assertEqual(self.waited, ["release-0.2.0"])
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

    def test_same_commit_passes_a_release_tag_on_its_commit(self):
        self.run_tag(registry="crates.io")
        summary = cohort.same_commit("release-0.2.0", "origin")
        self.assertIn(f"release-0.2.0 -> {self.sha} (release 0.2.0)", summary)
        self.assertIn("rs-rich-art-v0.2.0", summary)

    def test_same_commit_refuses_a_release_tag_without_its_record(self):
        git(self.work, "tag", "-a", "release-0.2.0", "-m", "by hand")
        with self.assertRaisesRegex(RuntimeError, "must record its release and commit"):
            cohort.same_commit("release-0.2.0", "origin")
        git(self.work, "tag", "-f", "-a", "release-0.2.0", "-m", f"x\n\nRelease: 0.2.0\nCommit: {self.sha}")
        cohort.same_commit("release-0.2.0", "origin")

    def test_same_commit_refuses_a_release_tag_of_another_release(self):
        git(self.work, "tag", "-a", "release-0.1.0", "-m", f"x\n\nRelease: 0.1.0\nCommit: {self.sha}")
        with self.assertRaisesRegex(RuntimeError, "records release 0.1.0"):
            cohort.same_commit("release-0.1.0", "origin")

    def test_the_gate_selects_what_tag_left_due(self):
        # What release.yml's gate computes from what `tag` pushed: the
        # unpublished crates, each tagged on the release commit.
        self.run_tag(registry="crates.io")
        self.published = {("crates.io", "rs-rich")}
        due = cohort.unreleased("release-0.2.0", cohort.load(cohort.files_at(self.sha)), self.status,
                                cohort.remote_tags("origin"), self.sha)
        self.assertEqual([p.tag for p in due], ["rs-rich-art-v0.2.0"])

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

    def test_same_commit_checks_each_crate_tag_against_an_all_crates_tag(self):
        # `v0.1.0` selects every crate; a `rs-rich-art-v0.1.0` on a commit that
        # ships another rs-rich-art must still be caught.
        write_tree(self.work, config=CONFIG.replace('version = "0.2.0"', 'version = "0.1.0"'),
                   art=ART.replace("0.2.0", "0.1.0"))
        git(self.work, "commit", "--quiet", "-am", "one version for every crate")
        first = git(self.work, "rev-parse", "HEAD")
        (self.work / "crates/rich-art/src").mkdir(parents=True)
        (self.work / "crates/rich-art/src/lib.rs").write_text("// other\n")
        git(self.work, "add", ".")
        git(self.work, "commit", "--quiet", "-m", "change art")
        git(self.work, "push", "--quiet", "origin", "main")
        git(self.work, "tag", "-a", "rs-rich-art-v0.1.0", "-m", "x")
        git(self.work, "push", "--quiet", "origin", "rs-rich-art-v0.1.0")
        git(self.work, "tag", "-a", "v0.1.0", "-m", "x", first)
        with self.assertRaisesRegex(RuntimeError, r"rs-rich-art@0\.1\.0.*src/lib\.rs"):
            cohort.same_commit("v0.1.0", "origin")

    def test_a_release_held_only_by_a_dead_tag_is_not_resumed(self):
        # rs-rich-art was tagged here and its run failed before publishing;
        # main moved on and changed it. Nothing of the release went out, so
        # HEAD is released and the dead tag moves there.
        self.published.clear()
        git(self.work, "tag", "-a", "rs-rich-art-v0.2.0", "-m", "x", self.sha)
        git(self.work, "push", "--quiet", "origin", "rs-rich-art-v0.2.0")
        (self.work / "crates/rich-art/src").mkdir(parents=True)
        (self.work / "crates/rich-art/src/lib.rs").write_text("// fixed\n")
        git(self.work, "add", ".")
        git(self.work, "commit", "--quiet", "-m", "fix the failure")
        git(self.work, "push", "--quiet", "origin", "main")
        head = git(self.work, "rev-parse", "HEAD")
        self.run_tag(commit=None, registry="crates.io", run=self.run_on(self.sha, conclusion="failure"))
        self.assertEqual(cohort.remote_tags("origin")["rs-rich-art-v0.2.0"], head)
        self.assertEqual(cohort.remote_tags("origin")["release-0.2.0"], head)

    def released_here(self, published=()):
        """`tag` pushed release 0.2.0 here; its run ended. `published` names
        the crates it got out before failing."""
        self.published.clear()
        self.run_tag(registry="crates.io")
        self.published = {("crates.io", name) for name in published}
        self.waited.clear()
        self.stale.clear()

    def test_a_dead_release_on_the_release_commit_is_pushed_again(self):
        # Re-running after a run that failed before publishing deletes and
        # pushes the release tag again; the records stay; the wait ignores
        # the failed run.
        self.released_here()
        pushes = self.pushes(registry="crates.io", run=self.run_on(self.sha, conclusion="failure"))
        self.assertEqual(pushes, [["--atomic", ":refs/tags/release-0.2.0"], ["--atomic", "refs/tags/release-0.2.0"]])
        self.assertEqual(self.waited, ["release-0.2.0"])
        self.assertEqual(self.stale, ["https://example/run"])
        self.assertEqual(cohort.remote_tags("origin")["release-0.2.0"], self.sha)

    def test_a_partial_release_is_resumed_by_pushing_its_tag_again(self):
        # rs-rich went out, then the run failed: `tag` (no --commit) resumes
        # on the release commit even after main has moved on, and pushes the
        # same release tag again; the gate will select only rs-rich-art.
        self.released_here(published=["rs-rich"])
        self.change_rs_rich("main moves on")
        failed = self.run_on(self.sha, conclusion="failure", publish_started=True)
        pushes = self.pushes(commit=None, registry="crates.io", run=failed)
        self.assertEqual(pushes, [["--atomic", ":refs/tags/release-0.2.0"], ["--atomic", "refs/tags/release-0.2.0"]])
        self.assertEqual(cohort.remote_tags("origin")["release-0.2.0"], self.sha)
        self.assertEqual(self.waited, ["release-0.2.0"])
        self.assertEqual(self.stale, ["https://example/run"])
        message = git(self.work, "for-each-ref", "refs/tags/release-0.2.0", "--format=%(contents)")
        self.assertIn("rs-rich-art 0.2.0", message)
        self.assertNotIn("rs-rich 0.1.0", message)

    def test_a_partial_release_holds_the_rest_to_its_commit(self):
        # The run that failed had started publishing: the unpublished
        # crates' records bind too, so a commit that changes them is refused.
        self.released_here(published=["rs-rich"])
        (self.work / "crates/rich-art/src").mkdir(parents=True)
        (self.work / "crates/rich-art/src/lib.rs").write_text("// fix\n")
        git(self.work, "add", ".")
        git(self.work, "commit", "--quiet", "-m", "fix art")
        git(self.work, "push", "--quiet", "origin", "main")
        failed = self.run_on(self.sha, conclusion="failure", publish_started=True)
        with self.assertRaisesRegex(RuntimeError, r"rs-rich-art@0\.2\.0.*src/lib\.rs"):
            self.run_tag(commit="HEAD", registry="crates.io", run=failed)

    def test_a_dead_release_moves_to_a_fix_on_main(self):
        # Nothing went out; main has a fix. `tag` releases HEAD and moves the
        # records and the release tag there in its two pushes.
        self.released_here()
        (self.work / "crates/rich-art/src").mkdir(parents=True)
        (self.work / "crates/rich-art/src/lib.rs").write_text("// fix\n")
        git(self.work, "add", ".")
        git(self.work, "commit", "--quiet", "-m", "fix art")
        git(self.work, "push", "--quiet", "origin", "main")
        head = git(self.work, "rev-parse", "HEAD")
        dead = self.run_on(self.sha, conclusion="failure")
        pushes = self.pushes(commit=None, registry="crates.io", run=dead)
        self.assertEqual(pushes[0][0], "--atomic")
        self.assertEqual(sorted(pushes[0][1:]), [":refs/tags/release-0.2.0", ":refs/tags/rs-rich-art-v0.2.0",
                                                 ":refs/tags/rs-rich-v0.1.0"])
        self.assertEqual(pushes[1:], [["--atomic", "refs/tags/rs-rich-v0.1.0", "refs/tags/rs-rich-art-v0.2.0"],
                                      ["--atomic", "refs/tags/release-0.2.0"]])
        tags = cohort.remote_tags("origin")
        self.assertEqual({tags["release-0.2.0"], tags["rs-rich-art-v0.2.0"], tags["rs-rich-v0.1.0"]}, {head})
        self.assertEqual(self.stale, [None])

    def test_a_live_release_tag_elsewhere_stops(self):
        self.released_here()
        later = self.change_rs_rich("later")
        going = self.run_on(self.sha, status="in_progress", conclusion="")
        with self.assertRaisesRegex(RuntimeError, f"tagged at {self.sha[:12]}"):
            self.run_tag(commit=later, registry="crates.io", run=going)

    def test_the_wait_ignores_the_run_a_pushed_again_tag_replaces(self):
        package = cohort.Package("rs-rich-art", "crates.io", "x", "0.2.0")
        runs = iter([("completed", "failure", "old"), ("completed", "success", "new")])
        cohort.wait_for("release-0.2.0", [package], 60, status=lambda p: 200, conclusion=lambda t: next(runs),
                        sleep=lambda s: None, stale="old")

    def test_the_wait_asks_for_the_release_tags_run_and_every_crate(self):
        rich, art = (cohort.Package(n, "crates.io", "x", "0.2.0") for n in ("rs-rich", "rs-rich-art"))
        out, asked, sleeps = {"rs-rich"}, [], []

        def conclusion(tag_name):
            asked.append(tag_name)
            if len(asked) == 1:
                out.add("rs-rich-art")
            return ("completed", "success", "u")
        cohort.wait_for("release-0.2.0", [rich, art], 60, status=lambda p: 200 if p.name in out else 404,
                        conclusion=conclusion, sleep=sleeps.append)
        self.assertEqual(set(asked), {"release-0.2.0"})
        self.assertEqual(len(sleeps), 1)

    def test_a_failed_release_run_says_how_far_it_got_and_how_to_resume(self):
        rich, art = (cohort.Package(n, "crates.io", "x", "0.2.0") for n in ("rs-rich", "rs-rich-art"))
        with self.assertRaisesRegex(RuntimeError, r"ended failure: u \(1 of 2 published\).*tag again"):
            cohort.wait_for("release-0.2.0", [rich, art], 60, status=lambda p: 200 if p.name == "rs-rich" else 404,
                            conclusion=lambda t: ("completed", "failure", "u"), sleep=lambda s: None)

    def test_the_run_of_a_crate_is_its_release_tags_on_the_same_commit(self):
        art = cohort.Package("rs-rich-art", "crates.io", "x", "0.2.0")
        python = cohort.Package("rs-rich", "pypi", "x", "0.3.0")
        self.assertEqual(cohort.run_tag_of(art, {"rs-rich-art-v0.2.0": "a", "release-0.2.0": "a"}), "release-0.2.0")
        self.assertEqual(cohort.run_tag_of(art, {"rs-rich-art-v0.2.0": "a", "release-0.2.0": "b"}),
                         "rs-rich-art-v0.2.0")       # a crate tag pushed alone, as before release tags
        self.assertEqual(cohort.run_tag_of(python, {"python-v0.3.0": "a", "release-0.2.0": "a"}), "python-v0.3.0")
        self.assertEqual(cohort.workflow_of("release-0.2.0"), "release.yml")
        self.assertEqual(cohort.workflow_of("python-v0.3.0"), "pypi-release.yml")

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
        cohort.wait_for("release-0.2.0", [package], 60, status=lambda p: 200, conclusion=lambda t: next(runs),
                        sleep=sleeps.append)
        self.assertEqual(len(sleeps), 2)

    def test_a_run_that_fails_after_uploading_stops_the_wait(self):
        package = cohort.Package("rs-rich-art", "crates.io", "x", "0.2.0")
        with self.assertRaisesRegex(RuntimeError, "ended failure"):
            cohort.wait_for("release-0.2.0", [package], 60, status=lambda p: 200,
                            conclusion=lambda t: ("completed", "failure", "https://example/run"),
                            sleep=lambda s: None)

    def test_without_gh_the_registry_decides(self):
        package = cohort.Package("rs-rich-art", "crates.io", "x", "0.2.0")
        cohort.wait_for("release-0.2.0", [package], 60, status=lambda p: 200, conclusion=lambda t: None,
                        sleep=lambda s: None)

    def test_a_failed_release_run_stops_the_wait(self):
        package = cohort.Package("rs-rich-art", "crates.io", "x", "0.2.0")
        with self.assertRaisesRegex(RuntimeError, "ended failure"):
            cohort.wait_for("release-0.2.0", [package], 60, status=lambda p: 404,
                            conclusion=lambda t: ("completed", "failure", "https://example/run"),
                            sleep=lambda s: None)


if __name__ == "__main__":
    unittest.main()
