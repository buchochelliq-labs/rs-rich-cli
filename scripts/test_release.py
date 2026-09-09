"""Release regression tests; no registry access or publishing credentials needed."""

from pathlib import Path
import json
import subprocess
import tempfile
import tomllib
import unittest
from unittest.mock import patch

import release


NAMES = ("rs-rich", "rs-rich-ext", "rs-rich-cli", "rs-rich-art")


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
        metadata["packages"][2]["version"] = "0.0.3-rc.1"
        self.assertEqual(release.select("rs-rich-cli-v0.0.3-rc.1", metadata, root),
                         {"rs-rich-cli": "0.0.3-rc.1"})
        with self.assertRaises(ValueError):
            release.select("rs-rich-cli-v0.0.3", metadata, root)

    def test_stale_internal_requirement_fails(self):
        metadata, root = workspace()
        metadata["packages"][3]["version"] = "0.0.3"
        with self.assertRaisesRegex(ValueError, "rich-art"):
            release.select("rs-rich-art-v0.0.3", metadata, root)

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
        metadata["packages"][2]["version"] = "0.0.3"
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory) / "output"
            with patch("sys.argv", ["release.py", "plan", "rs-rich-cli-v0.0.3"]), \
                    patch("release.subprocess.check_output", return_value=json.dumps(metadata)) as cargo, \
                    patch("release.Path.read_text", return_value=""), \
                    patch("release.tomllib.loads", return_value=root), \
                    patch.dict("release.os.environ", {"GITHUB_OUTPUT": str(output)}):
                release.main()
            cargo.assert_called_once_with(
                ["cargo", "metadata", "--no-deps", "--format-version", "1", "--locked"], text=True,
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
        for selection in ({}, {"unknown": "0.0.3"}, {"rs-rich-cli": "0.0.3;bad"},
                          {"rs-rich-cli": "0.0.3", "rs-rich-art": "0.0.3"}):
            with self.subTest(selection=selection), \
                    patch.dict("release.os.environ", {"RELEASE_SELECTION": json.dumps(selection)}), \
                    patch("sys.argv", ["release.py", "publish"]), self.assertRaises(ValueError):
                release.main()
        run.assert_not_called()

    @patch("release.registry_status", return_value=404)
    def test_preflight_queries_only_selected_versions(self, status):
        release.preflight({"rs-rich-art": "0.0.3"})
        status.assert_called_once_with("rs-rich-art", "0.0.3")

    def test_preflight_fails_closed(self):
        for code in (200, 403, 429, 500):
            with self.subTest(code=code), patch("release.registry_status", return_value=code):
                with self.assertRaises(RuntimeError):
                    release.preflight({"rs-rich-cli": "0.0.3"})

    @patch("release.subprocess.run")
    def test_publish_and_dry_run_use_same_selection(self, run):
        for selection, args in (({"rs-rich-cli": "0.0.3"}, ["-p", "rs-rich-cli"]),
                                ({"rs-rich-art": "0.0.3"}, ["-p", "rs-rich-art"]),
                                (dict.fromkeys(NAMES, "0.0.2"), ["--workspace"])):
            for dry_run in (True, False):
                release.publish(selection, dry_run=dry_run)
                run.assert_called_with(["cargo", "publish", *args, "--locked",
                                        *(["--dry-run"] if dry_run else [])], check=True)

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
        for name in ("rs-rich", "rs-rich-ext", "rs-rich-art"):
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
        self.assertEqual(run.call_count, 4)

    @patch("release.subprocess.run", side_effect=subprocess.CalledProcessError(1, "cargo"))
    @patch("release.registry_status", return_value=200)
    def test_consumer_build_failure_is_fatal(self, status, run):
        with self.assertRaises(subprocess.CalledProcessError):
            release.verify({"rs-rich-art": "0.0.3"})


if __name__ == "__main__":
    unittest.main()
