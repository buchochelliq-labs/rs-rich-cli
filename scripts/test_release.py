"""Release regression tests; no registry access or publishing credentials needed."""

from pathlib import Path
import json
import os
import subprocess
import tempfile
import tomllib
import unittest
from unittest.mock import patch

import release


NAMES = ("rs-rich", "rs-rich-plugin-api", "rs-rich-macros", "rs-rich-ext", "rs-rich-cli", "rs-rich-art",
         "rs-rich-mermaid", "rs-rich-lumis", "rs-rich-record", "rs-rich-interact", "rs-rich-micro",
         "rs-rich-diagram", "rs-rich-data", "rs-rich-ratatui", "rs-rich-intuituive", "rs-rich-web", "rs-rich-embed",
         "rs-rich-marketplace", "noctiluca")


def workspace():
    packages = [
        {"name": name, "version": "0.0.2", "id": name, "publish": None}
        for name in NAMES
    ]
    metadata = {"packages": packages, "workspace_members": list(NAMES)}
    root = {"workspace": {"dependencies": {
        name.removeprefix("rs-"): {"package": name, "version": "0.0.2"}
        for name in NAMES if name != "rs-rich-cli"
    }}}
    return metadata, root


def package(metadata, name):
    return next(p for p in metadata["packages"] if p["name"] == name)


class SelectionTests(unittest.TestCase):
    def test_legacy_tag_selects_all_four(self):
        self.assertEqual(release.select("v0.0.2", *workspace()), dict.fromkeys(NAMES, "0.0.2"))

    def test_each_crate_can_advance_independently(self):
        for name in NAMES:
            with self.subTest(name=name):
                metadata, root = workspace()
                next(p for p in metadata["packages"] if p["name"] == name)["version"] = "0.0.3"
                if name != "rs-rich-cli":
                    root["workspace"]["dependencies"][name.removeprefix("rs-")]["version"] = "0.0.3"
                self.assertEqual(release.select(f"{name}-v0.0.3", metadata, root), {name: "0.0.3"})
                with self.assertRaises(ValueError):
                    release.select("v0.0.3", metadata, root)

    def test_cli_and_art_at_same_new_version_remain_separate(self):
        metadata, root = workspace()
        for p in metadata["packages"]:
            if p["name"] in ("rs-rich-cli", "rs-rich-art"):
                p["version"] = "0.0.3"
        root["workspace"]["dependencies"]["rich-art"]["version"] = "0.0.3"
        for name in ("rs-rich-cli", "rs-rich-art"):
            self.assertEqual(release.select(f"{name}-v0.0.3", metadata, root), {name: "0.0.3"})

    def test_mismatch_and_invalid_tags_fail(self):
        for tag in ("v0.0.3", "rs-rich-cli-v0.0.3", "rs-unknown-v0.0.2", "main",
                    "v", "v01.0.2", "v0.0.2;echo bad", "rs-rich-art-v0.0.2\n"):
            with self.subTest(tag=tag), self.assertRaises(ValueError):
                release.select(tag, *workspace())

    def test_prerelease_requires_exact_manifest_version(self):
        metadata, root = workspace()
        package(metadata, "rs-rich-cli")["version"] = "0.0.3-rc.1"
        self.assertEqual(release.select("rs-rich-cli-v0.0.3-rc.1", metadata, root),
                         {"rs-rich-cli": "0.0.3-rc.1"})
        with self.assertRaises(ValueError):
            release.select("rs-rich-cli-v0.0.3", metadata, root)

    def test_stale_internal_requirement_fails(self):
        metadata, root = workspace()
        package(metadata, "rs-rich-art")["version"] = "0.0.3"
        with self.assertRaisesRegex(ValueError, "rich-art"):
            release.select("rs-rich-art-v0.0.3", metadata, root)

    def test_a_release_tag_selects_what_the_gate_handed_it_in_order(self):
        metadata, root = workspace()
        wanted = {"rs-rich-ext": "0.0.2", "rs-rich": "0.0.2", "rs-rich-cli": "0.0.2"}
        selection = release.select("release-0.0.2", metadata, root, wanted)
        self.assertEqual(list(selection), ["rs-rich-ext", "rs-rich", "rs-rich-cli"])

    def test_a_release_tag_checks_every_selected_manifest_and_requirement(self):
        metadata, root = workspace()
        package(metadata, "rs-rich-art")["version"] = "0.0.3"
        with self.assertRaisesRegex(ValueError, "rs-rich-art: manifest says 0.0.3, the release says 0.0.2"):
            release.select("release-0.0.2", metadata, root, {"rs-rich-art": "0.0.2"})
        with self.assertRaisesRegex(ValueError, "workspace dependency rich-art"):
            release.select("release-0.0.2", metadata, root, {"rs-rich-art": "0.0.3"})
        for wanted in ({}, {"rs-unknown": "0.0.2"}, {"rs-rich": "0.0.2;bad"}):
            with self.subTest(wanted=wanted), self.assertRaises(ValueError):
                release.select("release-0.0.2", *workspace(), wanted)

    def test_a_release_tag_without_a_selection_plans_the_whole_release(self):
        releases = "".join(f'[[package]]\nname = "{n}"\nregistry = "crates.io"\nversion = "0.0.2"\n'
                           for n in NAMES)
        releases += '[[package]]\nname = "rs-rich"\nregistry = "pypi"\nversion = "9.9.9"\n'
        self.assertEqual(release.select("release-0.0.2", *workspace(), releases=releases),
                         dict.fromkeys(NAMES, "0.0.2"))
        with self.assertRaisesRegex(ValueError, "names release 0.0.2"):
            release.select("release-0.0.3", *workspace(), releases=releases)
        with self.assertRaisesRegex(ValueError, "needs its selection"):
            release.select("release-0.0.2", *workspace())

    def test_a_crate_tag_refuses_a_selection_it_does_not_name(self):
        with self.assertRaisesRegex(ValueError, "not what the tag selects"):
            release.select("rs-rich-cli-v0.0.2", *workspace(), {"rs-rich": "0.0.2"})
        self.assertEqual(release.select("rs-rich-cli-v0.0.2", *workspace(), {"rs-rich-cli": "0.0.2"}),
                         {"rs-rich-cli": "0.0.2"})

    def test_release_tags_follow_the_version_grammar(self):
        for tag in ("release-0.0.20", "release-1.2.3-rc.1"):
            self.assertTrue(release.RELEASE.fullmatch(tag), tag)
        for tag in ("release-", "release-v0.0.20", "release-0.0", "releases-0.0.20", "release-0.0.20;x"):
            self.assertFalse(release.RELEASE.fullmatch(tag), tag)

    def test_changed_publishable_workspace_is_rejected(self):
        for change in ("missing", "extra", "disabled"):
            metadata, root = workspace()
            if change == "missing":
                metadata["workspace_members"].pop()
            elif change == "extra":
                metadata["packages"].append({"name": "other", "id": "other", "publish": None})
                metadata["workspace_members"].append("other")
            else:
                metadata["packages"][0]["publish"] = []
            with self.subTest(change=change), self.assertRaises(ValueError):
                release.select("v0.0.2", metadata, root)


class PublicationTests(unittest.TestCase):
    def test_plan_exports_selection_for_downstream_commands(self):
        metadata, root = workspace()
        package(metadata, "rs-rich-cli")["version"] = "0.0.3"
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory) / "output"
            with patch("sys.argv", ["release.py", "plan", "rs-rich-cli-v0.0.3"]), \
                    patch("release.subprocess.check_output", return_value=json.dumps(metadata)) as cargo, \
                    patch("release.Path.read_text", return_value=""), \
                    patch("release.tomllib.loads", return_value=root), \
                    patch.dict("release.os.environ", {"GITHUB_OUTPUT": str(output)}):
                release.main()
            cargo.assert_called_once_with(
                ["cargo", "metadata", "--no-deps", "--format-version", "1", "--locked"], text=True, encoding="utf-8",
            )
            exported = output.read_text().removeprefix("selection=").strip()
            self.assertEqual(json.loads(exported), {"rs-rich-cli": "0.0.3"})
            with patch.dict("release.os.environ", {"RELEASE_SELECTION": exported}), \
                    patch("sys.argv", ["release.py", "publish", "--dry-run"]), \
                    patch("release.subprocess.run") as run:
                release.main()
            run.assert_called_once_with(
                ["cargo", "publish", "-p", "rs-rich-cli", "--locked", "--dry-run"], check=True,
            )

    @patch("release.subprocess.run")
    def test_invalid_downstream_selection_never_publishes(self, run):
        for selection in ({}, {"unknown": "0.0.3"}, {"rs-rich-cli": "0.0.3;bad"}, ["rs-rich-cli"]):
            with self.subTest(selection=selection), \
                    patch.dict("release.os.environ", {"RELEASE_SELECTION": json.dumps(selection)}), \
                    patch("sys.argv", ["release.py", "publish"]), self.assertRaises(ValueError):
                release.main()
        run.assert_not_called()

    @patch("release.crate_status", return_value=200)
    @patch("release.registry_status", return_value=404)
    def test_preflight_queries_only_selected_versions(self, status, crate):
        self.assertEqual([], release.preflight({"rs-rich-art": "0.0.3"}))
        status.assert_called_once_with("rs-rich-art", "0.0.3")
        crate.assert_called_once_with("rs-rich-art")

    def test_preflight_fails_closed(self):
        for code in (200, 403, 429, 500):
            with self.subTest(code=code), patch("release.registry_status", return_value=code), \
                    patch("release.crate_status", return_value=200):
                with self.assertRaises(RuntimeError):
                    release.preflight({"rs-rich-cli": "0.0.3"})

    @patch("release.registry_status", return_value=404)
    def test_preflight_names_a_brand_new_crate(self, status):
        # Its first version cannot use Trusted Publishing (docs/BRANCHING.md).
        with patch("release.crate_status", return_value=404):
            self.assertEqual(["rs-rich-record"], release.preflight({"rs-rich-record": "0.0.1"}))

    @patch("release.registry_status", return_value=404)
    def test_preflight_lets_new_and_existing_crates_share_a_release(self, status):
        # `publish` uploads each kind with its own credential.
        exists = {"rs-rich": 200, "rs-rich-record": 404}
        with patch("release.crate_status", side_effect=lambda name: exists[name]):
            self.assertEqual(["rs-rich-record"],
                             release.preflight({"rs-rich": "0.0.9", "rs-rich-record": "0.0.1"}))

    @patch("release.registry_status", return_value=404)
    def test_preflight_fails_closed_when_crate_existence_is_unknown(self, status):
        for code in (403, 429, 500):
            with self.subTest(code=code), patch("release.crate_status", return_value=code):
                with self.assertRaises(RuntimeError):
                    release.preflight({"rs-rich-record": "0.0.1"})

    @patch("release.crate_status", return_value=404)
    @patch("release.registry_status", return_value=404)
    def test_preflight_exports_whether_a_crate_is_new(self, status, crate):
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory) / "output"
            env = {"RELEASE_SELECTION": '{"rs-rich-interact":"0.0.1"}', "GITHUB_OUTPUT": str(output)}
            with patch.dict(os.environ, env), patch("sys.argv", ["release.py", "preflight"]):
                release.main()
            self.assertEqual('new_crate=true\nexisting_crate=false\nnew_crates=["rs-rich-interact"]\n',
                             output.read_text(encoding="utf-8"))

    TOKENS = {"RELEASE_TRUSTED_TOKEN": "oidc", "RELEASE_NEW_CRATE_TOKEN": "secret"}

    @patch("release.subprocess.run")
    def test_publish_and_dry_run_use_same_selection(self, run):
        for selection, args in (({"rs-rich-cli": "0.0.3"}, ["-p", "rs-rich-cli"]),
                                ({"rs-rich-art": "0.0.3"}, ["-p", "rs-rich-art"]),
                                ({"rs-rich": "0.0.3", "rs-rich-ext": "0.0.4", "rs-rich-cli": "0.0.5"},
                                 ["-p", "rs-rich", "-p", "rs-rich-ext", "-p", "rs-rich-cli"]),
                                (dict.fromkeys(NAMES, "0.0.2"), [a for n in NAMES for a in ("-p", n)])):
            release.publish(selection, dry_run=True)
            run.assert_called_with(["cargo", "publish", *args, "--locked", "--dry-run"], check=True)
            release.publish(selection, dependencies={}, environ={"PATH": "/bin", **self.TOKENS})
            run.assert_called_with(["cargo", "publish", *args, "--locked"], check=True,
                                   env={"PATH": "/bin", "CARGO_REGISTRY_TOKEN": "oidc"})

    def test_publish_runs_group_crates_by_credential_in_dependency_order(self):
        order = {"rs-rich": "1", "rs-rich-ext": "1", "rs-rich-data": "1", "rs-rich-ratatui": "1", "rs-rich-cli": "1"}
        needs = {"rs-rich-ext": {"rs-rich"}, "rs-rich-data": {"rs-rich-ext"}, "rs-rich-ratatui": {"rs-rich"},
                 "rs-rich-cli": {"rs-rich-ext", "rs-rich-data"}}
        # No new crate: one upload.
        self.assertEqual(release.publish_runs(order, [], needs), [(False, list(order))])
        # A new crate nothing selected needs joins its own run after the existing ones.
        self.assertEqual(release.publish_runs(order, ["rs-rich-ratatui"], needs),
                         [(False, ["rs-rich", "rs-rich-ext", "rs-rich-data", "rs-rich-cli"]),
                          (True, ["rs-rich-ratatui"])])
        # A new crate between existing ones in the dependency order: three runs.
        self.assertEqual(release.publish_runs(order, ["rs-rich-data"], needs),
                         [(False, ["rs-rich", "rs-rich-ext", "rs-rich-ratatui"]), (True, ["rs-rich-data"]),
                          (False, ["rs-rich-cli"])])
        # Every crate new: one run with the token.
        self.assertEqual(release.publish_runs(order, list(order), needs), [(True, list(order))])
        with self.assertRaisesRegex(ValueError, "before a crate it depends on"):
            release.publish_runs({"rs-rich-ext": "1", "rs-rich": "1"}, ["rs-rich"], needs)

    @patch("release.subprocess.run")
    def test_each_run_uploads_with_its_own_credential_only(self, run):
        selection = {"rs-rich": "0.0.3", "rs-rich-data": "0.0.1", "rs-rich-cli": "0.0.4"}
        needs = {"rs-rich-data": {"rs-rich"}, "rs-rich-cli": {"rs-rich-data"}}
        release.publish(selection, new=["rs-rich-data"], dependencies=needs,
                        environ={**self.TOKENS, "CARGO_REGISTRY_TOKEN": "ambient"})
        self.assertEqual([(c.args[0], c.kwargs["env"]) for c in run.call_args_list], [
            (["cargo", "publish", "-p", "rs-rich", "--locked"], {"CARGO_REGISTRY_TOKEN": "oidc"}),
            (["cargo", "publish", "-p", "rs-rich-data", "--locked"], {"CARGO_REGISTRY_TOKEN": "secret"}),
            (["cargo", "publish", "-p", "rs-rich-cli", "--locked"], {"CARGO_REGISTRY_TOKEN": "oidc"}),
        ])

    @patch("release.subprocess.run")
    def test_a_missing_credential_stops_before_any_upload(self, run):
        selection = {"rs-rich": "0.0.3", "rs-rich-data": "0.0.1"}
        for environ, missing in (({"RELEASE_TRUSTED_TOKEN": "oidc"}, "CARGO_REGISTRY_TOKEN"),
                                 ({"RELEASE_NEW_CRATE_TOKEN": "secret"}, "exchange")):
            with self.subTest(missing=missing), self.assertRaisesRegex(RuntimeError, missing):
                release.publish(selection, new=["rs-rich-data"], dependencies={}, environ=environ)
        run.assert_not_called()

    @patch("release.subprocess.run")
    def test_publish_reads_the_new_crates_from_the_preflight(self, run):
        env = {"RELEASE_SELECTION": '{"rs-rich":"0.0.3","rs-rich-data":"0.0.1"}',
               "RELEASE_NEW_CRATES": '["rs-rich-data"]', **self.TOKENS}
        with patch.dict(os.environ, env), patch("sys.argv", ["release.py", "publish"]), \
                patch("release.internal_dependencies", return_value={"rs-rich-data": {"rs-rich"}}):
            release.main()
        self.assertEqual([c.args[0][-3:] for c in run.call_args_list],
                         [["-p", "rs-rich", "--locked"], ["-p", "rs-rich-data", "--locked"]])
        with patch.dict(os.environ, {**env, "RELEASE_NEW_CRATES": '["rs-rich-cli"]'}), \
                patch("sys.argv", ["release.py", "publish"]), self.assertRaises(ValueError):
            release.main()

    def test_internal_dependencies_skip_dev_dependencies(self):
        metadata = {"packages": [
            {"name": "rs-rich", "dependencies": [{"name": "rs-rich-ext", "kind": "dev"}]},
            {"name": "rs-rich-ext", "dependencies": [{"name": "rs-rich", "kind": None},
                                                     {"name": "serde", "kind": None}]},
            {"name": "rs-rich-cli", "dependencies": [{"name": "rs-rich-ext", "kind": "build"}]},
        ]}
        with patch("release.subprocess.check_output", return_value=json.dumps(metadata)):
            self.assertEqual(release.internal_dependencies(),
                             {"rs-rich": set(), "rs-rich-ext": {"rs-rich"}, "rs-rich-cli": {"rs-rich-ext"}})

    @patch("release.time.sleep")
    @patch("release.subprocess.run")
    @patch("release.registry_status", return_value=404)
    def test_verification_exhaustion_never_builds(self, status, run, sleep):
        with self.assertRaises(RuntimeError):
            release.verify({"rs-rich-art": "0.0.3"})
        self.assertEqual(status.call_count, 6)
        run.assert_not_called()

    @patch("release.subprocess.run")
    @patch("release.time.sleep")
    def test_verification_registry_errors_fail_immediately(self, sleep, run):
        for code in (403, 429, 500):
            with self.subTest(code=code), patch("release.registry_status", return_value=code):
                with self.assertRaises(RuntimeError):
                    release.verify({"rs-rich-art": "0.0.3"})
        sleep.assert_not_called()
        run.assert_not_called()

    @patch("release.time.sleep")
    @patch("release.subprocess.run")
    @patch("release.registry_status", side_effect=[404, 200])
    def test_cli_verification_retries_and_installs_exact_version(self, status, run, sleep):
        release.verify({"rs-rich-cli": "0.0.3"})
        self.assertEqual(status.call_args_list, [unittest.mock.call("rs-rich-cli", "0.0.3")] * 2)
        command = run.call_args.args[0]
        self.assertEqual(command[:6], ["cargo", "install", "rs-rich-cli", "--version", "=0.0.3", "--locked"])

    @patch("release.registry_status", return_value=200)
    def test_library_verification_builds_exact_registry_dependency(self, status):
        for name in ("rs-rich", "rs-rich-plugin-api", "rs-rich-macros", "rs-rich-ext", "rs-rich-art",
                     "rs-rich-mermaid", "rs-rich-lumis", "rs-rich-record", "rs-rich-interact", "rs-rich-micro",
                     "rs-rich-diagram", "rs-rich-data", "rs-rich-ratatui", "rs-rich-intuituive", "rs-rich-web", "rs-rich-embed",
                     "rs-rich-marketplace"):
            def check_consumer(command, **kwargs):
                self.assertEqual(command, ["cargo", "check"])
                consumer = Path(kwargs["cwd"])
                self.assertNotEqual(consumer, Path.cwd())
                manifest = tomllib.loads((consumer / "Cargo.toml").read_text())
                self.assertEqual(manifest["dependencies"], {
                    "released": {"package": name, "version": "=0.0.3"},
                })
                self.assertNotIn("patch", manifest)
            with self.subTest(name=name), patch("release.subprocess.run", side_effect=check_consumer):
                release.verify({name: "0.0.3"})

    @patch("release.subprocess.run")
    @patch("release.registry_status", return_value=200)
    def test_coordinated_verification_checks_every_package(self, status, run):
        release.verify(dict.fromkeys(NAMES, "0.0.2"))
        self.assertEqual({call.args for call in status.call_args_list}, {(name, "0.0.2") for name in NAMES})
        self.assertEqual(run.call_count, len(NAMES))

    @patch("release.subprocess.run", side_effect=subprocess.CalledProcessError(1, "cargo"))
    @patch("release.registry_status", return_value=200)
    def test_consumer_build_failure_is_fatal(self, status, run):
        with self.assertRaises(subprocess.CalledProcessError):
            release.verify({"rs-rich-art": "0.0.3"})


if __name__ == "__main__":
    unittest.main()
