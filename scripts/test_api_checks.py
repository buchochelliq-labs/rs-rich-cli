"""Tests for scripts/public_api.py and scripts/semver_check.py, with no tool,
network or build: cargo-semver-checks' runs and crates.io are stubbed."""

import io
from pathlib import Path
import subprocess
import tomllib
import unittest

import public_api
import release
import semver_check

ROOT = Path(__file__).resolve().parent.parent

REPORT = """\
--- failure enum_missing: pub enum removed or renamed ---

Description:
A publicly-visible enum cannot be imported by its prior path.

Failed in:
  enum rich_ext::rst::Inline, previously in file /registry/rs-rich-ext-0.0.19/src/rst/parse.rs:12
  enum rich_ext::rst::Block, previously in file /registry/rs-rich-ext-0.0.19/src/rst/parse.rs:75

--- failure copy_impl_added: type now implements Copy ---

Failed in:
  rich_ext::surface::Cell in /work/crates/rich-ext/src/surface/screen.rs:221
"""


def runner(stdout="", returncode=0, calls=None):
    def run(command, **kwargs):
        if calls is not None:
            calls.append(command)
        return subprocess.CompletedProcess(command, returncode, stdout, "    Building ...\n")
    return run


class BumpKinds(unittest.TestCase):
    def test_cargo_treats_every_0_0_x_bump_as_breaking(self):
        self.assertEqual(semver_check.bump_kind("0.0.12", "0.0.13"), "major")
        self.assertEqual(semver_check.bump_kind("0.0.12", "0.1.0"), "major")

    def test_0_y_and_x_y(self):
        self.assertEqual(semver_check.bump_kind("0.1.2", "0.1.3"), "minor")
        self.assertEqual(semver_check.bump_kind("0.1.2", "0.2.0"), "major")
        self.assertEqual(semver_check.bump_kind("1.2.3", "1.2.4"), "patch")
        self.assertEqual(semver_check.bump_kind("1.2.3", "1.3.0"), "minor")
        self.assertEqual(semver_check.bump_kind("1.2.3", "2.0.0"), "major")
        self.assertEqual(semver_check.bump_kind("1.2.3", "1.2.3"), "none")


class Reports(unittest.TestCase):
    def test_failures_name_items_without_their_locations(self):
        self.assertEqual(semver_check.parse_failures(REPORT), {
            "enum_missing": ["enum rich_ext::rst::Inline", "enum rich_ext::rst::Block"],
            "copy_impl_added": ["rich_ext::surface::Cell"],
        })

    def test_only_unlisted_items_are_unexplained(self):
        failures = semver_check.parse_failures(REPORT)
        accepted = {"enum_missing": {"enum rich_ext::rst::Inline", "enum rich_ext::rst::Block"}}
        self.assertEqual(semver_check.unexplained(failures, accepted),
                         [("copy_impl_added", "rich_ext::surface::Cell")])

    def test_a_wildcard_takes_every_item_of_its_lint(self):
        failures = semver_check.parse_failures(REPORT)
        accepted = {"enum_missing": {"*"}, "copy_impl_added": {"*"}}
        self.assertEqual(semver_check.unexplained(failures, accepted), [])

    def test_the_checked_in_known_breaks_parse(self):
        text = (ROOT / "api/semver-breaks.toml").read_text(encoding="utf-8")
        breaks = semver_check.known_breaks(text)
        for entry in tomllib.loads(text)["break"]:
            self.assertTrue(entry["why"])
            self.assertIn(entry["lint"], breaks[(entry["crate"], entry["version"])])


class Gate(unittest.TestCase):
    """Cargo's rule, as release.py plan --semver-checks applies it."""

    def test_a_0_0_x_bump_fits_any_change_without_building(self):
        calls = []
        refusals = semver_check.gate({"rs-rich-ext": "0.0.20"}, published=lambda _: "0.0.19",
                                     run=runner(REPORT, 1, calls), out=io.StringIO())
        self.assertEqual(refusals, [])
        self.assertEqual(calls, [])

    def test_a_patch_bump_of_a_0_y_crate_with_a_break_is_refused(self):
        refusals = semver_check.gate({"rs-rich": "0.1.1"}, published=lambda _: "0.1.0",
                                     run=runner(REPORT, 1), out=io.StringIO(), libraries={"rs-rich": "0.1.1"})
        self.assertEqual(len(refusals), 1)
        self.assertIn("too small", refusals[0])

    def test_a_patch_bump_with_no_break_passes(self):
        calls = []
        refusals = semver_check.gate({"rs-rich": "1.0.1"}, published=lambda _: "1.0.0",
                                     run=runner("", 0, calls), out=io.StringIO(), libraries={"rs-rich": "1.0.1"})
        self.assertEqual(refusals, [])
        self.assertNotIn("--release-type", calls[0])
        self.assertEqual(calls[0][calls[0].index("--baseline-version") + 1], "1.0.0")

    def test_a_proc_macro_crate_is_not_built(self):
        calls = []
        refusals = semver_check.gate({"rs-rich-macros": "0.1.1"}, published=lambda _: "0.1.0",
                                     run=runner(REPORT, 1, calls), out=io.StringIO(), libraries={})
        self.assertEqual((refusals, calls), ([], []))

    def test_a_crate_not_yet_published_has_nothing_to_compare(self):
        refusals = semver_check.gate({"rs-rich-new": "0.0.1"}, published=lambda _: None,
                                     run=runner(REPORT, 1), out=io.StringIO())
        self.assertEqual(refusals, [])

    def test_a_version_older_than_the_release_is_refused(self):
        refusals = semver_check.gate({"rs-rich": "0.0.11"}, published=lambda _: "0.0.12",
                                     run=runner(), out=io.StringIO())
        self.assertEqual(len(refusals), 1)

    def test_a_tool_failure_is_not_a_pass(self):
        with self.assertRaises(RuntimeError):
            semver_check.gate({"rs-rich": "1.0.1"}, published=lambda _: "1.0.0",
                              run=runner("", 101), out=io.StringIO(), libraries={"rs-rich": "1.0.1"})

    def test_release_plan_raises_on_a_refusal(self):
        with self.assertRaisesRegex(ValueError, "smaller than the change"):
            release.semver_gate({"rs-rich": "0.1.1"}, gate=lambda selection: ["rs-rich: too small"])
        release.semver_gate({"rs-rich": "0.0.13"}, gate=lambda selection: [])


class Check(unittest.TestCase):
    """CI's check: 0.0.x crates strictly, against the known breaks."""

    def test_listed_breaks_pass_and_unlisted_ones_fail(self):
        calls = []
        accepted = {("rs-rich-ext", "0.0.20"): {
            "enum_missing": {"enum rich_ext::rst::Inline", "enum rich_ext::rst::Block"},
            "copy_impl_added": {"rich_ext::surface::Cell"},
        }}
        problems = semver_check.check({"rs-rich-ext": "0.0.20"}, accepted, published=lambda _: "0.0.19",
                                      run=runner(REPORT, 1, calls), out=io.StringIO())
        self.assertEqual(problems, [])
        self.assertIn("--release-type", calls[0])
        del accepted[("rs-rich-ext", "0.0.20")]["copy_impl_added"]
        problems = semver_check.check({"rs-rich-ext": "0.0.20"}, accepted, published=lambda _: "0.0.19",
                                      run=runner(REPORT, 1), out=io.StringIO())
        self.assertEqual(len(problems), 1)
        self.assertIn("rich_ext::surface::Cell", problems[0])

    def test_breaks_listed_for_another_version_do_not_count(self):
        accepted = {("rs-rich-ext", "0.0.19"): {"enum_missing": {"enum rich_ext::rst::Inline"}}}
        problems = semver_check.check({"rs-rich-ext": "0.0.20"}, accepted, published=lambda _: "0.0.19",
                                      run=runner(REPORT, 1), out=io.StringIO())
        self.assertEqual(len(problems), 1)

    def test_spent_entries_are_reported(self):
        out = io.StringIO()
        accepted = {("rs-rich-ext", "0.0.19"): {"enum_missing": {"x"}}}
        semver_check.check({"rs-rich-ext": "0.0.20"}, accepted, published=lambda _: "0.0.19",
                           run=runner(""), out=out)
        self.assertIn("spent", out.getvalue())


class PublicApi(unittest.TestCase):
    def test_every_publishable_library_and_proc_macro_is_snapshotted(self):
        metadata = {
            "workspace_members": ["a", "b", "c", "d"],
            "packages": [
                {"id": "a", "name": "rs-rich", "targets": [{"kind": ["lib"], "name": "rich"}]},
                {"id": "b", "name": "rs-rich-macros", "targets": [{"kind": ["proc-macro"], "name": "rich_macros"}]},
                {"id": "c", "name": "tool", "publish": [], "targets": [{"kind": ["lib"], "name": "tool"}]},
                {"id": "d", "name": "bin-only", "targets": [{"kind": ["bin"], "name": "bin-only"}]},
            ],
        }
        self.assertEqual(public_api.crates(metadata), {"rs-rich": "rich", "rs-rich-macros": "rich_macros"})

    def test_the_snapshots_cover_the_release_crates(self):
        snapshots = {path.stem for path in (ROOT / "api").glob("*.txt")}
        self.assertEqual(snapshots, set(release.CRATES))


if __name__ == "__main__":
    unittest.main()
