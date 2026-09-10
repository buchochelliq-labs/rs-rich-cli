"""Exercise docs drift, oracle pins, and release ancestry with real temp trees."""

from pathlib import Path
import subprocess
import tempfile
import unittest

import gen_versions
from read_upstream_version import read_version

ROOT = Path(__file__).resolve().parent.parent


class ReadinessTests(unittest.TestCase):
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
        with tempfile.TemporaryDirectory() as tmp:
            def git(*args):
                return subprocess.run(["git", *args], cwd=tmp, check=True,
                                      capture_output=True, text=True).stdout.strip()

            def check(mode, base):
                return subprocess.run(["bash", str(ROOT / "scripts/check_pr_base.sh"), mode, base],
                                      cwd=tmp, capture_output=True).returncode

            git("init", "-q")
            git("config", "user.name", "Test")
            git("config", "user.email", "test@example.invalid")
            git("commit", "--allow-empty", "-qm", "baseline")
            baseline = git("rev-parse", "HEAD")
            git("commit", "--allow-empty", "-qm", "main advances")
            main = git("rev-parse", "HEAD")
            git("update-ref", "refs/remotes/origin/main", main)
            git("checkout", "--detach", baseline)
            for base in ["rc/0.0.3", "release/0.0.3", "releases/v0.0.3-rc"]:
                with self.subTest(base=base):
                    self.assertEqual(check("base", base), 0)
                    self.assertNotEqual(check("current", base), 0)
            self.assertEqual(check("current", "main"), 0)
            for base in ["fix/stacked", "releases/", "rc/", "release/", "releases-other"]:
                self.assertNotEqual(check("base", base), 0)
            git("commit", "--allow-empty", "-qm", "release work")
            git("merge", "--no-ff", "--no-edit", main)
            for base in ["rc/0.0.3", "release/0.0.3", "releases/v0.0.3-rc"]:
                self.assertEqual(check("current", base), 0)


if __name__ == "__main__":
    unittest.main()
