"""Exercise docs drift, oracle pins, and release ancestry with real temp trees."""

import os
import json
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest

import gen_versions
import validate_release
from read_upstream_version import read_version

ROOT = Path(__file__).resolve().parent.parent


def git_env():
    return {
        key: value
        for key, value in os.environ.items()
        if not key.startswith("GIT_CONFIG_")
    }


def git(cwd, *args):
    proc = subprocess.run(["git", *args], cwd=cwd,
                          capture_output=True, text=True, env=git_env())
    if proc.returncode != 0:
        raise AssertionError(
            f"git {' '.join(args)} failed\nstdout:\n{proc.stdout}\nstderr:\n{proc.stderr}"
        )
    return proc.stdout.strip()


class ReadinessTests(unittest.TestCase):
    def test_release_tag_must_be_annotated_checked_out_and_on_main(self):
        with tempfile.TemporaryDirectory() as tmp:
            def validate(tag):
                return subprocess.run(
                    [sys.executable, str(ROOT / "scripts/release.py"), "validate-tag", tag],
                    cwd=tmp, capture_output=True, text=True, env=git_env(),
                )

            git(tmp, "init", "-q")
            git(tmp, "config", "user.name", "Test")
            git(tmp, "config", "user.email", "test@example.invalid")
            git(tmp, "commit", "--allow-empty", "-qm", "release")
            main = git(tmp, "rev-parse", "HEAD")
            git(tmp, "update-ref", "refs/remotes/origin/main", main)
            for tag in ("v0.0.3", "rs-rich-cli-v0.0.3", "rs-rich-art-v0.0.3-rc.1"):
                git(tmp, "tag", "-a", tag, "-m", "release")
                result = validate(tag)
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertEqual(result.stdout.strip(), main)
            git(tmp, "tag", "rs-rich-v0.0.3")
            result = validate("rs-rich-v0.0.3")
            self.assertNotEqual(result.returncode, 0)
            self.assertIn("must be annotated", result.stderr)
            git(tmp, "commit", "--allow-empty", "-qm", "unmerged work")
            self.assertNotEqual(validate("v0.0.3").returncode, 0)
            git(tmp, "tag", "-a", "v0.0.4", "-m", "not on main")
            self.assertNotEqual(validate("v0.0.4").returncode, 0)
            self.assertNotEqual(validate("main").returncode, 0)

    def test_independent_versions_and_drift(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            (root / "Cargo.toml").write_text('[workspace]\nmembers = ["core", "cli"]\n')
            for name, version in [("core", "0.0.2"), ("cli", "0.0.3-rc.1")]:
                (root / name).mkdir()
                (root / name / "Cargo.toml").write_text(
                    f'[package]\nname = "{name}"\nversion = "{version}"\n')
            (root / "docs").mkdir()
            for doc in ["README.md", "docs/index.md"]:
                (root / doc).write_text(gen_versions.START + "\nstale\n" + gen_versions.END)
            self.assertEqual(gen_versions.update(root, check=True), 1)
            self.assertIn("stale", (root / "README.md").read_text())
            self.assertEqual(gen_versions.update(root), 0)
            self.assertEqual(gen_versions.update(root, check=True), 0)
            self.assertIn("`0.0.2`", (root / "README.md").read_text())
            self.assertIn("`0.0.3-rc.1`", (root / "README.md").read_text())
            (root / "docs/index.md").write_text("missing markers")
            with self.assertRaises(ValueError):
                gen_versions.update(root, check=True)

    def test_oracle_pin_parses_toml_not_line_format(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "UPSTREAM.toml"
            path.write_text("[rich-cli]\nversion='1.8.1'\n [rich] # oracle\n version = '15.0.0' # exact\n")
            self.assertEqual(read_version(path), "15.0.0")
            for value in ['""', '"15.*"', '15', '"15.0.0 extra"']:
                with self.subTest(value=value):
                    path.write_text(f"[rich]\nversion={value}\n")
                    with self.assertRaises(ValueError):
                        read_version(path)
            path.write_text('[rich-cli]\nversion="1.8.1"\n')
            with self.assertRaises(KeyError):
                read_version(path)

    def test_release_aliases_require_main_ancestry(self):
        bash = shutil.which("bash")
        if bash is None:
            self.skipTest("bash is required to exercise scripts/check_pr_base.sh")
        with tempfile.TemporaryDirectory() as tmp:
            def check(mode, base):
                return subprocess.run(
                    [bash, str(ROOT / "scripts/check_pr_base.sh"), mode, base],
                    cwd=tmp,
                    capture_output=True,
                    env=git_env(),
                ).returncode

            git(tmp, "init", "-q")
            git(tmp, "config", "user.name", "Test")
            git(tmp, "config", "user.email", "test@example.invalid")
            git(tmp, "commit", "--allow-empty", "-qm", "baseline")
            baseline = git(tmp, "rev-parse", "HEAD")
            git(tmp, "commit", "--allow-empty", "-qm", "main advances")
            main = git(tmp, "rev-parse", "HEAD")
            git(tmp, "update-ref", "refs/remotes/origin/main", main)
            git(tmp, "checkout", "--detach", baseline)
            for base in ["rc/0.0.3", "release/0.0.3", "releases/v0.0.3-rc"]:
                with self.subTest(base=base):
                    self.assertEqual(check("base", base), 0)
                    self.assertNotEqual(check("current", base), 0)
            self.assertEqual(check("current", "main"), 0)
            for base in ["fix/stacked", "releases/", "rc/", "release/", "releases-other"]:
                self.assertNotEqual(check("base", base), 0)
            git(tmp, "commit", "--allow-empty", "-qm", "release work")
            git(tmp, "merge", "--no-ff", "--no-edit", main)
            for base in ["rc/0.0.3", "release/0.0.3", "releases/v0.0.3-rc"]:
                self.assertEqual(check("current", base), 0)

    def test_release_validation_wrapper_serializes_shared_target_cargo(self):
        steps = validate_release.commands("rs-rich-cli-v0.0.6")
        self.assertIn(["cargo", "test", "--all"], steps)
        self.assertIn(["cargo", "build", "-p", "rs-rich-cli", "--locked"], steps)
        self.assertIn(
            [sys.executable, "-m", "unittest", "discover", "-s", "scripts", "-p", "test_release.py", "-v"],
            steps,
        )
        self.assertIn(
            [
                sys.executable,
                "-m",
                "unittest",
                "discover",
                "-s",
                "scripts",
                "-p",
                "test_release_readiness.py",
                "-v",
            ],
            steps,
        )
        self.assertEqual(
            steps[-1],
            [sys.executable, "scripts/release.py", "plan", "rs-rich-cli-v0.0.6"],
        )
        self.assertLess(
            steps.index(["cargo", "test", "--all"]),
            steps.index(["cargo", "build", "-p", "rs-rich-cli", "--locked"]),
        )
        self.assertTrue(validate_release.cli_binary().endswith(".exe") if os.name == "nt" else True)

    def test_release_handoff_gate_catches_release_file_only_prs(self):
        workflow = (ROOT / ".github/workflows/pr-hygiene.yml").read_text()
        policy = json.loads((ROOT / ".github/release-readiness.json").read_text())
        self.assertIn("actions/checkout@v4", workflow)
        self.assertIn("policy.handoffTriggers.some", workflow)
        self.assertIn("Unknown release handoff trigger in policy", workflow)
        self.assertIn("new RegExp(policy.versionPattern)", workflow)
        self.assertIn("policy.releaseFiles.includes(filename)", workflow)
        self.assertIn("policy.releaseFileSuffixes.some", workflow)
        self.assertEqual(["versionSignal", "releaseFiles"], policy["handoffTriggers"])
        self.assertIn(".claude/skills/release/SKILL.md", policy["releaseFiles"])
        self.assertIn("/Cargo.toml", policy["releaseFileSuffixes"])

    def test_workflows_use_published_checkout_major(self):
        workflow_dir = ROOT / ".github/workflows"
        checkout_refs = [
            (path, line)
            for path in workflow_dir.glob("*.yml")
            for line in path.read_text().splitlines()
            if "uses: actions/checkout@" in line
        ]
        self.assertGreaterEqual(len(checkout_refs), 1)
        self.assertEqual(
            [],
            [
                f"{path.relative_to(ROOT)}: {line.strip()}"
                for path, line in checkout_refs
                if "actions/checkout@v4" not in line
            ],
        )

    def test_release_handoff_gate_enforces_final_snapshot_fields(self):
        workflow = (ROOT / ".github/workflows/pr-hygiene.yml").read_text()
        policy = json.loads((ROOT / ".github/release-readiness.json").read_text())
        self.assertIn("while (hasNextPage)", workflow)
        self.assertIn("reviewThreads(first:100, after:$cursor)", workflow)
        self.assertIn("headRefOid", workflow)
        self.assertIn("mergeStateStatus", workflow)
        self.assertIn("Release readiness snapshot", workflow)
        self.assertIn("Remaining visible blocker:", workflow)
        self.assertIn(".filter((thread) => !thread.isResolved)", workflow)
        self.assertIn("unresolved review thread(s)", workflow)
        self.assertIn("body.includes(pr.head.sha)", workflow)
        self.assertIn("policy.handoffRequiredText", workflow)
        self.assertGreaterEqual(len(policy["handoffRequiredText"]), 1)


if __name__ == "__main__":
    unittest.main()
