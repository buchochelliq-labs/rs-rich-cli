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

    def test_restaging_an_unpublished_version_verifies_the_current_sibling(self):
        """A second run at the same unpublished version must not reuse the first
        run's unpacked sibling or its compiled artifacts (core 0.0.6 incident)."""
        import contextlib
        import os
        import shutil
        import subprocess
        from unittest.mock import patch

        if not shutil.which("cargo"):
            self.skipTest("Cargo is required for staged artifact regression")
        with tempfile.TemporaryDirectory(prefix="restage-regression-") as directory:
            root = Path(directory)
            (root / "Cargo.toml").write_text(
                '[workspace]\nresolver="2"\nmembers=["core","ext","art","cli"]\n')
            for folder, name, dependency in [("core", "rs-rich", None),
                                             ("ext", "rs-rich-ext", ("rs-rich", "core")),
                                             ("art", "rs-rich-art", None),
                                             ("cli", "rs-rich-cli", ("rs-rich-ext", "ext"))]:
                crate = root / folder
                (crate / "src").mkdir(parents=True)
                manifest = (f'[package]\nname="{name}"\nversion="0.0.6"\nedition="2021"\n'
                            'description="Package regression fixture"\nlicense="MIT"\n')
                if dependency:
                    manifest += (f'[dependencies]\n{dependency[0]}='
                                 f'{{path="../{dependency[1]}",version="0.0.6"}}\n')
                (crate / "Cargo.toml").write_text(manifest)
                (crate / "src/lib.rs").write_text("pub struct Widget;\n")
            core, ext = root / "core/src/lib.rs", root / "ext/src/lib.rs"
            core.write_text("pub trait Api { fn old(&self) {} }\n")
            ext.write_text("pub struct Leaf;\nimpl rs_rich::Api for Leaf {}\n")
            environment = {"CARGO_TARGET_DIR": str(root / "target"),
                           "CARGO_HOME": str(root / "cargo-home"), "CARGO_NET_OFFLINE": "true"}
            with patch.dict(os.environ, environment), \
                    patch("check_packages.registry_status", return_value=404), \
                    contextlib.redirect_stdout(io.StringIO()):
                subprocess.run(["cargo", "generate-lockfile", "--offline"], cwd=root, check=True,
                               capture_output=True)
                check_packages.check(root, allow_dirty=True)
                # Same unpublished version, new core API that ext now uses.
                core.write_text("pub trait Api { fn old(&self) {} fn added(&self) {} }\n")
                ext.write_text("pub struct Leaf;\nimpl rs_rich::Api for Leaf {}\n"
                               "pub fn call() { rs_rich::Api::added(&Leaf) }\n")
                output = io.StringIO()
                with contextlib.redirect_stdout(output):
                    check_packages.check(root, allow_dirty=True)
            self.assertIn("Removed stale staged copy", output.getvalue())
            self.assertIn("Staged package verification passed", output.getvalue())

    def test_purge_skips_public_registries_and_unrelated_versions(self):
        from unittest.mock import patch

        with tempfile.TemporaryDirectory() as directory:
            home = Path(directory)
            staging = home / "registry/src/-e024bdec1b3619fb"
            public = home / "registry/src/index.crates.io-1949cf8c6b5b557f"
            cache = home / "registry/cache/-e024bdec1b3619fb"
            for path in [staging / "rs-rich-0.0.6", staging / "rs-rich-0.0.5",
                         public / "rs-rich-0.0.6"]:
                path.mkdir(parents=True)
            cache.mkdir(parents=True)
            (cache / "rs-rich-0.0.6.crate").write_bytes(b"stale")
            with patch("check_packages.subprocess.run") as run:
                removed = check_packages.purge_staged_copies(home, [("rs-rich", "0.0.6")], home=home)
            self.assertEqual(sorted(removed),
                             sorted([staging / "rs-rich-0.0.6", cache / "rs-rich-0.0.6.crate"]))
            self.assertTrue((staging / "rs-rich-0.0.5").is_dir())
            self.assertTrue((public / "rs-rich-0.0.6").is_dir())
            run.assert_called_once_with(["cargo", "clean", "--quiet", "-p", "rs-rich"],
                                        cwd=home, check=True)


if __name__ == "__main__":
    unittest.main()
