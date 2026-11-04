"""Tests for the release on release-kit: release-kit.yml, release.yml and the
rehearsal script. No registry access or publishing credentials needed."""

from pathlib import Path
import contextlib
import io
import json
import re
import tempfile
import tomllib
import unittest

import release_rehearsal

ROOT = Path(__file__).resolve().parent.parent
WORKFLOW = (ROOT / ".github/workflows/release.yml").read_text()
KIT = release_rehearsal.kit_module()
CONFIG = KIT.load(KIT.files_on_disk(ROOT))


def job(name):
    """A job's text in release.yml, up to the next job."""
    start = WORKFLOW.index(f"\n  {name}:\n")
    following = re.search(r"\n  [a-z-]+:\n", WORKFLOW[start + 1:])
    return WORKFLOW[start:start + 1 + following.start()] if following else WORKFLOW[start:]


def job_names():
    jobs = WORKFLOW[WORKFLOW.index("\njobs:\n"):]
    return re.findall(r"(?m)^  ([a-z-]+):$", jobs)


class Config(unittest.TestCase):
    """release-kit.yml against RELEASES.toml, which it replaces."""

    def test_the_kit_finds_no_problem(self):
        self.assertEqual(KIT.check(KIT.files_on_disk(ROOT)), [])

    def test_every_package_of_releases_toml_in_its_order(self):
        old = tomllib.loads((ROOT / "RELEASES.toml").read_text())["package"]
        self.assertEqual([(p["name"], p["registry"], p["manifest"], p["version"]) for p in old],
                         [(p.name, p.registry, p.manifest, p.version) for p in CONFIG.packages])

    def test_the_release_is_named_by_the_cli(self):
        self.assertEqual(CONFIG.name_from, "rs-rich-cli")
        self.assertEqual(CONFIG.tag, f"release-{CONFIG.packages[-2].version}")

    def test_the_pypi_package_keeps_its_python_tags(self):
        pypi = [p for p in CONFIG.packages if p.registry == "pypi"]
        self.assertEqual([(p.name, p.tag) for p in pypi], [("rs-rich", f"python-v{pypi[0].version}")])

    def test_a_crate_ships_what_its_include_names(self):
        for package in CONFIG.packages:
            if package.registry != "crates.io":
                continue
            folder = str(Path(package.manifest).parent)
            include = tomllib.loads((ROOT / package.manifest).read_text())["package"]["include"]
            expected = [f"{folder}/{g.removesuffix('/**/*') + '/**' if g.endswith('/**/*') else g}"
                        for g in include]
            self.assertEqual([p for p in package.paths if p != "Cargo.lock"], expected, package.name)

    def test_the_wheels_are_built_where_pypi_release_built_them(self):
        pypi = next(p for p in CONFIG.packages if p.registry == "pypi")
        (artifact,) = pypi.artifacts
        self.assertEqual(artifact["kind"], "maturin")
        with tempfile.TemporaryDirectory() as scratch, contextlib.redirect_stdout(io.StringIO()):
            jobs = KIT.plan(release_rehearsal.full_selection(KIT, KIT.files_on_disk(ROOT)),
                            read=KIT.files_on_disk(ROOT), output=str(Path(scratch) / "out"))
        runners = {j["target"]: j["os"] for j in jobs if j["kind"] == "maturin"}
        self.assertEqual(runners, {
            "x86_64-unknown-linux-gnu": "ubuntu-latest",
            "aarch64-unknown-linux-gnu": "ubuntu-24.04-arm",
            "aarch64-apple-darwin": "macos-latest",
            "x86_64-apple-darwin": "macos-15-intel",
            "x86_64-pc-windows-msvc": "windows-latest",
        })
        self.assertIn("rs-rich-sdist", [j["id"] for j in jobs])

    def test_every_wheel_is_installed_and_imported(self):
        pypi = next(p for p in CONFIG.packages if p.registry == "pypi")
        test = pypi.artifacts[0]["test"]
        self.assertIsInstance(test, str)    # every target, none skipped
        self.assertIn('python crates/rich-py/tests/smoke.py "$DIST"', test)

    def test_the_wheel_test_installs_what_ci_tests_install(self):
        # A dependency only python.yml installs (Pillow, for the micro page)
        # would fail the release after every wheel is built.
        def deps(text):
            line = next(l for l in text.splitlines() if "pip install" in l and "pytest" in l)
            return {d for d in line.split("pip install", 1)[1].split()[1:] if "mypy" not in d}
        python = (ROOT / ".github/workflows/python.yml").read_text()
        test = next(p for p in CONFIG.packages if p.registry == "pypi").artifacts[0]["test"]
        self.assertLessEqual(deps(python), deps(test))


class Workflow(unittest.TestCase):
    """release.yml: the kit's jobs, and this repository's own."""

    def test_the_kit_is_installed_unmodified(self):
        self.assertEqual(KIT.vendored(ROOT), [])

    def test_only_a_release_tag_push_or_a_dispatch_starts_it(self):
        on = WORKFLOW[WORKFLOW.index("\non:"):WORKFLOW.index("\npermissions:")]
        self.assertIn('  push:\n    tags: ["release-*"]\n  workflow_dispatch:', on)
        self.assertEqual(on.count("tags:"), 1)
        self.assertIn("      verify_only:", on)

    def test_ci_runs_once_on_the_gated_commit_with_its_own_input_name(self):
        self.assertEqual(WORKFLOW.count("uses: ./.github/workflows/ci.yml"), 1)
        self.assertIn("checkout_ref: ${{ needs.gate.outputs.sha }}", job("ci"))
        self.assertIn("needs: gate", job("ci"))
        ci = (ROOT / ".github/workflows/ci.yml").read_text()
        self.assertIn("      checkout_ref:", ci[:ci.index("\njobs:")])

    def test_the_gate_and_the_build_are_the_kits(self):
        self.assertIn("uses: ./.github/release-kit/gate", job("gate"))
        self.assertIn("fetch-depth: 0", job("gate"))
        self.assertIn("uses: ./.github/workflows/release-kit-build.yml", job("build"))

    def test_only_jobs_that_upload_are_named_publish(self):
        # The kit reads a started `publish…` job as a possible upload and then
        # never moves the run's tags (docs: recovery, `dead()`).
        names = job_names()
        self.assertEqual([n for n in names if n.startswith("publish")],
                         ["publish-crates", "publish-pypi", "publish-playground"])
        for name in names:
            text = job(name)
            uploads = any(word in text for word in ("release-kit/publish-", "gh release upload", "id-token: write"))
            self.assertEqual(uploads, name.startswith("publish"), name)
        self.assertIn("name: publish-playground", job("publish-playground"))

    def test_nothing_publishes_before_ci_the_build_and_the_semver_check(self):
        for name in ("publish-crates", "publish-pypi"):
            self.assertIn("needs: [gate, ci, build, semver]", job(name))
            self.assertIn("needs.ci.result == 'success'", job(name))
        self.assertIn("needs.semver.result == 'success'", job("publish-crates"))
        self.assertIn("needs: [gate, publish-crates]", job("publish-playground"))
        self.assertIn("needs.publish-crates.result == 'success'", job("publish-playground"))

    def test_the_semver_job_checks_the_selected_crates(self):
        semver = job("semver")
        self.assertIn("needs.gate.outputs.crates-io == 'true'", semver)
        self.assertIn('python3 scripts/release.py plan "release-$RELEASE" --semver-checks', semver)
        self.assertIn('if p["registry"] == "crates.io"', semver)
        self.assertNotIn("id-token", semver)

    def test_the_semver_selection_is_what_scripts_release_py_reads(self):
        import release
        selection = json.loads(release_rehearsal.full_selection(KIT, KIT.files_on_disk(ROOT)))
        crates = {p["name"]: p["version"] for p in selection if p["registry"] == "crates.io"}
        self.assertEqual(list(crates), [p.name for p in CONFIG.packages if p.registry == "crates.io"])
        self.assertTrue(release.RELEASE.fullmatch(f"release-{CONFIG.name}"))

    def test_trusted_publishing_in_protected_environments(self):
        self.assertIn("environment: crates-io", job("publish-crates"))
        self.assertIn("environment: pypi", job("publish-pypi"))
        self.assertEqual(WORKFLOW.count("id-token: write"), 2)
        # A crate's first version alone uses the environment's token.
        self.assertEqual(WORKFLOW.count("secrets."), 1)
        self.assertIn("steps.preflight.outputs.new_crate == 'true' && secrets.CARGO_REGISTRY_TOKEN || ''",
                      job("publish-crates"))
        crates = job("publish-crates")
        self.assertLess(crates.index("release.py preflight"), crates.index("crates-io-auth-action"))

    def test_actions_are_pinned_to_commits(self):
        for line in WORKFLOW.splitlines():
            if "uses:" in line and "uses: ./" not in line:
                self.assertRegex(line, r"uses: [\w.-]+/[\w./-]+@[0-9a-f]{40} # \S+")

    def test_releases_never_cancel_each_other(self):
        self.assertIn("group: release\n  cancel-in-progress: false", WORKFLOW)


class Rehearsal(unittest.TestCase):
    def test_the_full_selection_has_the_gates_shape(self):
        selection = json.loads(release_rehearsal.full_selection(KIT, KIT.files_on_disk(ROOT)))
        self.assertEqual(len(selection), len(CONFIG.packages))
        self.assertEqual(set(selection[0]), {"name", "registry", "version", "manifest", "tag", "slug"})
        self.assertEqual(selection[-1]["tag"], f"python-v{CONFIG.packages[-1].version}")

    def test_ci_rehearses_the_release(self):
        ci = (ROOT / ".github/workflows/ci.yml").read_text()
        tests = ci[ci.index("  release-tests:"):ci.index("\n  packages:")]
        self.assertIn("pip install --quiet pyyaml", tests)
        self.assertIn("python3 scripts/release_rehearsal.py", tests)


if __name__ == "__main__":
    unittest.main()
