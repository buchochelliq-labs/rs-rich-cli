"""Tests for scripts/validate_marketplace.py."""

import copy
import contextlib
import hashlib
import io
import json
import os
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, os.path.dirname(__file__))

import validate_marketplace as vm  # noqa: E402

EXAMPLES = vm.SCHEMA_DIR / "examples"
VALID = sorted((EXAMPLES / "valid").glob("*.json"))
INVALID = sorted((EXAMPLES / "invalid").glob("*.json"))


def example(name):
    return json.loads((EXAMPLES / "valid" / f"{name}.json").read_text())


class ExamplesTest(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.validator = vm.Validator("off")

    def test_there_is_a_valid_example_of_every_kind(self):
        kinds = {json.loads(p.read_text())["kind"] for p in VALID}
        self.assertEqual(kinds, {"crate", "wasm-plugin", "micro-pack", "theme"})

    def test_every_valid_example_passes(self):
        for path in VALID:
            with self.subTest(path.name):
                self.assertEqual(self.validator.check_listing_file(path), [])

    def test_every_invalid_example_is_refused(self):
        self.assertGreaterEqual(len(INVALID), 3)
        for path in INVALID:
            with self.subTest(path.name):
                self.assertNotEqual(self.validator.check_listing_file(path), [])

    def test_the_named_invalid_examples_fail_for_their_reason(self):
        reasons = {
            "bad-hash": "artifact.sha256",
            "short-hash": "artifact.sha256",
            "unknown-kind": "kind:",
            "missing-abi": "requires: missing 'abi'",
            "wasm-network-permission": "permissions[1]",
            "yanked-without-reason": "missing 'yank_reason'",
            "crate-without-native-code": "native-code",
            "http-artifact": "artifact.url",
            "unsorted-versions": "newest first",
            "ansi-without-permission": "needs 'ansi-output'",
            "crate-url-mismatch": "static.crates.io",
            "official-outside-repository": "tier:",
            "duplicate-version": "repeats",
        }
        self.assertEqual(set(reasons), {p.stem for p in INVALID})
        for path in INVALID:
            with self.subTest(path.name):
                errors = self.validator.check_listing_file(path)
                self.assertTrue(
                    any(reasons[path.stem] in e for e in errors), errors
                )


class RulesTest(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.validator = vm.Validator("off")

    def errors(self, listing):
        return self.validator.check_listing(listing)

    def test_a_newer_schema_version_is_refused(self):
        listing = example("shout")
        listing["schema_version"] = 2
        self.assertIn("schema_version: must be 1", self.errors(listing))

    def test_unknown_keys_are_refused_at_every_level(self):
        for mutate in (
            lambda o: o.__setitem__("extra", 1),
            lambda o: o["versions"][0].__setitem__("extra", 1),
            lambda o: o["versions"][0]["artifact"].__setitem__("md5", "x"),
        ):
            listing = example("shout")
            mutate(listing)
            self.assertTrue(any("unknown key" in e for e in self.errors(listing)))

    def test_requires_names_only_what_the_kind_needs(self):
        listing = example("shout")
        listing["versions"][0]["requires"]["plugin_api"] = 1
        self.assertNotEqual(self.errors(listing), [])
        crate = example("mermaid")
        crate["versions"][0]["requires"] = {"abi": "1.0"}
        self.assertNotEqual(self.errors(crate), [])

    def test_a_numeric_pre_release_identifier_has_no_leading_zero(self):
        for version, ok in [("1.0.0-0", True), ("1.0.0-10", True), ("1.0.0-01", False),
                            ("1.0.0-alpha.01", False), ("1.0.0-0a", True), ("1.0.0-01a", True),
                            ("1.0.0+01", True)]:
            listing = example("shout")
            listing["versions"] = listing["versions"][:1]
            listing["versions"][0]["version"] = version
            with self.subTest(version):
                self.assertEqual(self.errors(listing) == [], ok, self.errors(listing))
                self.assertEqual(vm.SEMVER.match(version) is not None, ok)

    def test_a_published_day_must_exist(self):
        listing = example("shout")
        listing["versions"][0]["published"] = "2026-02-30"
        self.assertTrue(any("published" in e for e in self.errors(listing)))

    def test_the_abi_is_major_dot_minor(self):
        for abi, ok in [("1.0", True), ("1.12", True), ("1", False), ("1.0.0", False),
                        ("01.0", False), ("v1.0", False)]:
            listing = example("shout")
            for version in listing["versions"]:
                version["requires"]["abi"] = abi
            with self.subTest(abi):
                self.assertEqual(self.errors(listing) == [], ok)

    def test_asset_packs_declare_no_permissions(self):
        for name in ("status-icons", "solarized"):
            listing = example(name)
            listing["versions"][0]["permissions"] = ["filesystem-read"]
            with self.subTest(name):
                self.assertNotEqual(self.errors(listing), [])

    def test_a_micro_pack_id_is_a_micro_name_segment(self):
        listing = example("status-icons")
        listing["id"] = "status.icons"
        self.assertNotEqual(self.errors(listing), [])
        theme = example("solarized")
        theme["id"] = "solarized.dark"
        self.assertEqual(self.errors(theme), [])

    def test_a_crate_listing_needs_its_crate_and_others_must_not_have_one(self):
        crate = example("mermaid")
        del crate["crate"]
        self.assertTrue(any("missing 'crate'" in e for e in self.errors(crate)))
        wasm = example("shout")
        wasm["crate"] = {"name": "shout"}
        self.assertNotEqual(self.errors(wasm), [])

    def test_a_wasm_artifact_over_the_hosts_module_limit_is_refused(self):
        listing = example("shout")
        listing["versions"][0]["artifact"]["size"] = 16 * 1024 * 1024 + 1
        self.assertTrue(any("more than" in e for e in self.errors(listing)))

    def test_a_wasm_version_names_its_source_revision(self):
        listing = example("shout")
        del listing["versions"][0]["source_revision"]
        self.assertNotEqual(self.errors(listing), [])

    def test_control_characters_are_refused_in_text(self):
        listing = example("shout")
        listing["description"] = "evil\x1b]0;title\x07"
        self.assertNotEqual(self.errors(listing), [])

    def test_a_reason_without_a_yank_is_refused(self):
        listing = example("shout")
        listing["versions"][0]["yank_reason"] = "why"
        self.assertNotEqual(self.errors(listing), [])

    def test_a_capability_declared_twice_is_refused(self):
        listing = example("shout")
        caps = listing["versions"][0]["capabilities"]
        caps.append(copy.deepcopy(caps[0]))
        self.assertTrue(any("declared twice" in e for e in self.errors(listing)))


class EvaluatorTest(unittest.TestCase):
    def test_a_schema_with_an_unsupported_keyword_is_refused(self):
        with self.assertRaisesRegex(vm.SetupError, "oneOf"):
            vm.Evaluator({"properties": {"a": {"oneOf": [{"type": "string"}]}}})

    def test_json_equality_keeps_booleans_and_numbers_apart(self):
        self.assertFalse(vm.json_equal(True, 1))
        self.assertTrue(vm.json_equal(1, 1.0))
        self.assertFalse(vm.json_equal({"a": [1]}, {"a": [True]}))
        evaluator = vm.Evaluator({"const": 1})
        self.assertNotEqual(evaluator.errors(True), [])
        self.assertEqual(evaluator.errors(1.0), [])

    def test_integers_and_booleans(self):
        evaluator = vm.Evaluator({"type": "integer"})
        self.assertEqual(evaluator.errors(3), [])
        self.assertNotEqual(evaluator.errors(True), [])
        self.assertNotEqual(evaluator.errors(1.5), [])

    def test_semver_precedence(self):
        ordered = ["0.9.0", "1.0.0-alpha", "1.0.0-alpha.1", "1.0.0-alpha.beta",
                   "1.0.0-beta.2", "1.0.0-beta.11", "1.0.0-rc.1", "1.0.0", "1.0.1", "1.10.0"]
        keys = [vm.semver_key(v) for v in ordered]
        self.assertEqual(keys, sorted(keys))
        self.assertEqual(vm.semver_key("1.0.0+build.5"), vm.semver_key("1.0.0"))

    def test_json_that_is_ambiguous_is_refused(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "x.json"
            path.write_text('{"id": "a", "id": "b"}')
            with self.assertRaisesRegex(ValueError, "duplicate key"):
                vm.load_json(path)
            path.write_text('{"size": NaN}')
            with self.assertRaisesRegex(ValueError, "NaN"):
                vm.load_json(path)


class IndexTest(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.validator = vm.Validator("off")

    def marketplace(self, tmp, names=("shout", "mermaid")):
        root = Path(tmp)
        (root / "packages").mkdir()
        listings = {}
        for name in names:
            data = (EXAMPLES / "valid" / f"{name}.json").read_bytes()
            (root / "packages" / f"{name}.json").write_bytes(data)
            listings[name] = {
                "path": f"packages/{name}.json",
                "sha256": hashlib.sha256(data).hexdigest(),
            }
        index = {
            "schema_version": 1,
            "serial": 7,
            "generated": "2026-10-20T12:00:00Z",
            "expires": "2026-11-19T12:00:00Z",
            "listings": listings,
        }
        path = root / "index.json"
        path.write_text(json.dumps(index, indent=2))
        return path, index

    def test_a_consistent_marketplace_passes(self):
        with tempfile.TemporaryDirectory() as tmp:
            path, _ = self.marketplace(tmp)
            self.assertEqual(self.validator.check_index_file(path), [])

    def test_a_listing_changed_after_the_index_is_refused(self):
        with tempfile.TemporaryDirectory() as tmp:
            path, _ = self.marketplace(tmp)
            listing = path.parent / "packages" / "shout.json"
            listing.write_text(listing.read_text().replace("Shout", "Shout!"))
            errors = self.validator.check_index_file(path)
            self.assertTrue(any("SHA-256 is" in e for e in errors), errors)

    def test_a_listing_the_index_leaves_out_is_refused(self):
        with tempfile.TemporaryDirectory() as tmp:
            path, _ = self.marketplace(tmp)
            stray = path.parent / "packages" / "solarized.json"
            stray.write_bytes((EXAMPLES / "valid" / "solarized.json").read_bytes())
            errors = self.validator.check_index_file(path)
            self.assertTrue(any("not in the index" in e for e in errors), errors)

    def test_a_listing_filed_under_another_id_is_refused(self):
        with tempfile.TemporaryDirectory() as tmp:
            path, index = self.marketplace(tmp, names=("shout",))
            root = path.parent
            (root / "packages" / "shout.json").rename(root / "packages" / "loud.json")
            entry = index["listings"].pop("shout")
            entry["path"] = "packages/loud.json"
            index["listings"]["loud"] = entry
            path.write_text(json.dumps(index))
            errors = self.validator.check_index_file(path)
            self.assertTrue(any("filed as 'loud'" in e for e in errors), errors)

    def test_an_invalid_index_is_refused(self):
        with tempfile.TemporaryDirectory() as tmp:
            path, index = self.marketplace(tmp)
            index["serial"] = 0
            index["expires"] = index["generated"]
            path.write_text(json.dumps(index))
            errors = self.validator.check_index_file(path)
            self.assertTrue(any("serial" in e for e in errors), errors)
            index["serial"] = 1
            path.write_text(json.dumps(index))
            errors = self.validator.check_index_file(path)
            self.assertTrue(any("expires" in e for e in errors), errors)
            # Times that cannot exist are refused, by the schema's ranges
            # and by the calendar.
            for bad in ("2026-99-99T99:99:99Z", "2026-02-30T00:00:00Z"):
                index["generated"] = bad
                path.write_text(json.dumps(index))
                errors = self.validator.check_index_file(path)
                self.assertTrue(any("generated" in e for e in errors), (bad, errors))


class MainTest(unittest.TestCase):
    def run_main(self, *argv):
        out = io.StringIO()
        with contextlib.redirect_stdout(out), contextlib.redirect_stderr(out):
            try:
                code = vm.main(["--jsonschema", "off", *map(str, argv)])
            except SystemExit as exit:
                code = exit.code
        return code, out.getvalue()

    def test_exit_status(self):
        self.assertEqual(self.run_main(*VALID)[0], 0)
        self.assertEqual(self.run_main("--expect-invalid", *INVALID)[0], 0)
        code, out = self.run_main("--expect-invalid", VALID[0])
        self.assertEqual(code, 1)
        self.assertIn("expected to be refused", out)
        code, out = self.run_main(INVALID[0])
        self.assertEqual(code, 1)
        self.assertIn("FAIL", out)
        self.assertEqual(self.run_main()[0], 2)

    def test_require_without_jsonschema_is_a_setup_error(self):
        try:
            import jsonschema  # noqa: F401
        except ImportError:
            out = io.StringIO()
            with contextlib.redirect_stderr(out):
                self.assertEqual(vm.main(["--jsonschema", "require", str(VALID[0])]), 2)
        else:
            self.skipTest("jsonschema is installed")


class JsonschemaAgreementTest(unittest.TestCase):
    """When jsonschema is installed, it and the built-in evaluator agree on
    every example and every schema-level rule."""

    def setUp(self):
        try:
            import jsonschema  # noqa: F401
        except ImportError:
            self.skipTest("jsonschema is not installed")
        self.validator = vm.Validator("require")

    def test_both_agree_on_every_example(self):
        for path in VALID + INVALID:
            document = json.loads(path.read_text())
            with self.subTest(path.name):
                ours = self.validator.listing.errors(document)
                theirs = list(self.validator.third_party["listing"].iter_errors(document))
                self.assertEqual(ours == [], theirs == [], (ours, theirs))


if __name__ == "__main__":
    unittest.main()
