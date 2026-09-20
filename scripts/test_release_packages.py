"""Package-content regression fixtures run without a registry connection."""
import io
from pathlib import Path
import tarfile
import tempfile
import unittest

import check_packages


def archive(path, files):
    with tarfile.open(path, "w:gz") as stream:
        for name, value in files.items():
            data = value.encode()
            entry = tarfile.TarInfo(f"rs-rich-ext-0.0.5/{name}")
            entry.size = len(data)
            stream.addfile(entry, io.BytesIO(data))


class PackageTests(unittest.TestCase):
    def compare(self, local, published):
        with tempfile.TemporaryDirectory() as directory:
            left, right = Path(directory) / "local.crate", Path(directory) / "registry.crate"
            archive(left, local)
            archive(right, published)
            check_packages.compare_packages("rs-rich-ext", "0.0.5", left, right)

    def test_unversioned_clone_api_change_is_rejected(self):
        with self.assertRaisesRegex(RuntimeError, "rs-rich-ext@0.0.5.*src/lib.rs"):
            self.compare({"src/lib.rs": "#[derive(Clone)]\npub struct Widget;"},
                         {"src/lib.rs": "pub struct Widget;"})

    def test_added_and_deleted_packaged_sources_are_rejected(self):
        for left, right in [({"build.rs": "fn main() {}"}, {}),
                            ({}, {"src/old.rs": "pub struct Old;"})]:
            with self.subTest(left=left), self.assertRaises(RuntimeError):
                self.compare(left, right)

    def test_generated_provenance_and_lockfile_do_not_require_version_bump(self):
        self.compare({"src/lib.rs": "pub struct Widget;", "Cargo.lock": "new lock",
                      "Cargo.toml.orig": "workspace = true", ".cargo_vcs_info.json": "new sha",
                      "Cargo.toml": '[package]\nname="example"\nversion="0.0.5"\n'},
                     {"src/lib.rs": "pub struct Widget;", "Cargo.lock": "old lock",
                      "Cargo.toml": '# Generated\n[package]\nversion="0.0.5"\nname="example"\n'})

    def test_manifest_dependency_change_is_rejected(self):
        with self.assertRaisesRegex(RuntimeError, "Cargo.toml"):
            self.compare({"Cargo.toml": '[dependencies]\nfoo="2"'},
                         {"Cargo.toml": '[dependencies]\nfoo="1"'})

    def test_registry_errors_are_not_treated_as_unpublished(self):
        for code in (401, 403, 429, 500):
            with self.subTest(code=code), self.assertRaises(RuntimeError):
                check_packages.is_published("rs-rich-ext", "0.0.5", code)
        self.assertTrue(check_packages.is_published("rs-rich-ext", "0.0.5", 200))
        self.assertFalse(check_packages.is_published("rs-rich-ext", "0.0.6", 404))


class StagedCargoTests(unittest.TestCase):
    def test_staged_new_dependency_builds_but_unchanged_clone_mismatch_fails(self):
        """Real Cargo packages/compiles fixtures; only registry HTTP is replaced."""
        import contextlib
        import os
        import shutil
        import subprocess
        from unittest.mock import patch

        if not shutil.which("cargo"):
            self.skipTest("Cargo is required for staged artifact regression")
        with tempfile.TemporaryDirectory(prefix="package-regression-") as directory:
            root = Path(directory)
            target = root / "target"
            (root / "Cargo.toml").write_text(
                '[workspace]\nresolver="2"\nmembers=["core","ext","art","cli"]\n')
            for folder, name in [("core", "rs-rich"), ("ext", "rs-rich-ext"),
                                 ("art", "rs-rich-art"), ("cli", "rs-rich-cli")]:
                crate = root / folder
                (crate / "src").mkdir(parents=True)
                manifest = (f'[package]\nname="{name}"\nversion="0.0.5"\nedition="2021"\n'
                            'description="Package regression fixture"\nlicense="MIT"\n')
                if folder == "cli":
                    manifest += '[dependencies]\nrs-rich-ext={path="../ext",version="0.0.5"}\n'
                (crate / "Cargo.toml").write_text(manifest)
                (crate / "src/lib.rs").write_text("pub struct Widget;\n")
            environment = {"CARGO_TARGET_DIR": str(target), "CARGO_HOME": str(root / "cargo-home"),
                           "CARGO_NET_OFFLINE": "true"}
            with patch.dict(os.environ, environment):
                subprocess.run(["cargo", "generate-lockfile", "--offline"], cwd=root, check=True,
                               capture_output=True)
                subprocess.run(["cargo", "package", "-p", "rs-rich-ext", "--no-verify",
                                "--allow-dirty", "--offline"], cwd=root, check=True, capture_output=True)
                registry_tarball = (target / "package/rs-rich-ext-0.0.5.crate").read_bytes()
                (root / "ext/src/lib.rs").write_text("#[derive(Clone)]\npub struct Widget;\n")
                (root / "cli/src/lib.rs").write_text(
                    "pub fn use_new_api() { let _ = rs_rich_ext::Widget.clone(); }\n")
                with patch("check_packages.registry_status", side_effect=lambda name, version:
                           200 if (name, version) == ("rs-rich-ext", "0.0.5") else 404), \
                        patch("check_packages.urlopen", side_effect=lambda *args, **kwargs:
                              io.BytesIO(registry_tarball)):
                    with self.assertRaisesRegex(RuntimeError, "rs-rich-ext@0.0.5.*src/lib.rs"):
                        check_packages.check(root, allow_dirty=True)
                    for filename in ["ext/Cargo.toml", "cli/Cargo.toml"]:
                        path = root / filename
                        path.write_text(path.read_text().replace('version="0.0.5"', 'version="0.0.6"'))
                    subprocess.run(["cargo", "generate-lockfile", "--offline"], cwd=root, check=True,
                                   capture_output=True)
                    output = io.StringIO()
                    with contextlib.redirect_stdout(output):
                        check_packages.check(root, allow_dirty=True)
                    self.assertIn("Staged package verification passed", output.getvalue())
                    self.assertIn("does not establish registry publication readiness", output.getvalue())


if __name__ == "__main__":
    unittest.main()
