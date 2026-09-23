"""Pre-tag package gate (Python 3.11+, Cargo 1.90+).

Run online: python scripts/check_packages.py [--allow-dirty]
Every published workspace version must match its registry artifact. Cargo then
verifies workspace tarballs using its staging registry for unpublished siblings.
This is NOT registry readiness: publish changed dependencies first and retain the
release workflow's unpatched publish dry run and exact-version consumer checks.

Registry HTTP/network errors fail closed. There is deliberately no offline bypass
or stale cache fallback; offline regression fixtures live in test_release_packages.py.
Only generated provenance, the original pre-normalized manifest, and Cargo.lock
are excluded from comparison (library consumers resolve their own lockfiles).
"""

import argparse
import json
import os
from pathlib import Path
import shutil
import subprocess
import tarfile
import tempfile
import tomllib
from urllib.request import Request, urlopen

from release import CRATES, registry_status


GENERATED = {"Cargo.lock", "Cargo.toml.orig", ".cargo_vcs_info.json"}


def package_contents(path):
    """Read regular files without extracting untrusted registry archive paths."""
    contents = {}
    with tarfile.open(path, "r:gz") as archive:
        for entry in archive:
            if entry.isdir():
                continue
            parts = entry.name.split("/", 1)
            if not entry.isfile() or len(parts) != 2 or ".." in parts[1].split("/"):
                raise RuntimeError(f"Unsupported package entry: {entry.name}")
            name = parts[1]
            if name in GENERATED:
                continue
            if name in contents:
                raise RuntimeError(f"Duplicate package entry: {name}")
            with archive.extractfile(entry) as stream:
                data = stream.read()
            contents[name] = tomllib.loads(data.decode("utf-8")) if name == "Cargo.toml" else data
    return contents


def compare_packages(name, version, local, published):
    left, right = package_contents(local), package_contents(published)
    different = sorted(key for key in left.keys() | right.keys() if left.get(key) != right.get(key))
    if different:
        raise RuntimeError(
            f"{name}@{version} differs from its immutable registry package: {', '.join(different)}. "
            "Bump this crate and its dependent version requirements before tagging."
        )


# Registry caches that only ever hold published, immutable artifacts.
PUBLIC_REGISTRY_PREFIXES = ("index.crates.io-", "github.com-")


def purge_staged_copies(root, staged, home=None):
    """Drop Cargo's cached copies of staged-only versions before verification.

    Cargo's staging registry unpacks each sibling tarball into
    $CARGO_HOME/registry/src/<staging>/NAME-VERSION and, as for any registry
    package, treats it as immutable: an unpack left by an earlier run at the same
    unpublished version is reused, and so are artifacts compiled from it. Packaging
    again after a code change then verifies dependents against the old sibling,
    failing a good tree or passing one that uses a removed API (the core 0.0.6
    `fit_to_measurement` incident). Returns the removed cache paths.
    """
    registry = Path(home or os.environ.get("CARGO_HOME") or Path.home() / ".cargo") / "registry"
    removed = []
    for kind, suffix in (("src", ""), ("cache", ".crate")):
        folder = registry / kind
        if not folder.is_dir():
            continue
        for index in sorted(folder.iterdir()):
            if index.name.startswith(PUBLIC_REGISTRY_PREFIXES):
                continue
            for name, version in staged:
                path = index / f"{name}-{version}{suffix}"
                if path.is_dir():
                    shutil.rmtree(path)
                elif path.exists():
                    path.unlink()
                else:
                    continue
                removed.append(path)
    for name, _ in staged:
        subprocess.run(["cargo", "clean", "--quiet", "-p", name], cwd=root, check=True)
    return removed


def is_published(name, version, status):
    if status not in (200, 404):
        raise RuntimeError(f"Cannot establish registry state for {name}@{version}: HTTP {status}")
    return status == 200


def check(root, allow_dirty=False):
    metadata = json.loads(subprocess.check_output(
        ["cargo", "metadata", "--no-deps", "--format-version", "1", "--locked"], cwd=root, text=True,
    ))
    members = set(metadata["workspace_members"])
    packages = {p["name"]: p for p in metadata["packages"]
                if p["id"] in members and p.get("publish") != []}
    if set(packages) != set(CRATES):
        raise RuntimeError(f"Unexpected publishable packages: {sorted(packages)}")
    published = {name: is_published(name, p["version"], registry_status(name, p["version"]))
                 for name, p in packages.items()}
    command = ["cargo", "package", "--workspace", "--locked", "--all-features"]
    if allow_dirty:
        command.append("--allow-dirty")
    # Cargo itself handles topological packaging and staged sibling resolution.
    # Compare before compiling so unchanged local API additions cannot be hidden
    # by Cargo's staging registry (the ext 0.0.5 Clone incident).
    subprocess.run([*command, "--no-verify"], cwd=root, check=True)
    artifacts = Path(metadata["target_directory"]) / "package"
    with tempfile.TemporaryDirectory(prefix="rs-rich-registry-packages-") as temporary:
        for name, package in packages.items():
            version = package["version"]
            if not published[name]:
                print(f"STAGED ONLY: {name}@{version} is unpublished", flush=True)
                continue
            filename = f"{name}-{version}.crate"
            request = Request(f"https://static.crates.io/crates/{name}/{filename}",
                              headers={"User-Agent": "rs-rich-package-check"})
            destination = Path(temporary) / filename
            with urlopen(request, timeout=60) as response:
                destination.write_bytes(response.read())
            compare_packages(name, version, artifacts / filename, destination)
            print(f"Registry contents match: {name}@{version}", flush=True)
    staged = sorted((name, p["version"]) for name, p in packages.items() if not published[name])
    for path in purge_staged_copies(root, staged):
        print(f"Removed stale staged copy: {path}", flush=True)
    subprocess.run(command, cwd=root, check=True)
    print("Staged package verification passed. This does not establish registry publication readiness.",
          flush=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--allow-dirty", action="store_true", help="include uncommitted preparation edits")
    args = parser.parse_args()
    check(Path(__file__).resolve().parent.parent, args.allow_dirty)


if __name__ == "__main__":
    main()
