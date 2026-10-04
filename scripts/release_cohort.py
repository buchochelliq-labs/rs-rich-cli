"""Release numbers (RELEASES.toml): check them, show them, and tag what is due.

RELEASES.toml lists every package this repository publishes with its version
and its place in the publication order. This script keeps it honest and acts
on it:

    check       RELEASES.toml matches every manifest, lists every publishable
                crate once, and orders each after its workspace dependencies.
    sync        rewrite RELEASES.toml's versions from the manifests.
    status      each package's version, tag, and whether it is on its registry.
    unreleased  fail when what a release tag selects is already published
                (the first step of release.yml and pypi-release.yml).
    tag         create and push the tag of every package not yet on its
                registry, one at a time in order, waiting for each release to
                appear before the next. A published version is never tagged.

Requires Python 3.11+ and git; `tag` uses the GitHub CLI (`gh`), when present,
to stop as soon as a release run fails.
"""

import argparse
from dataclasses import dataclass
from pathlib import Path
import re
import shutil
import subprocess
import sys
import time
import tomllib
from urllib.error import HTTPError
from urllib.request import Request, urlopen

sys.path.insert(0, str(Path(__file__).resolve().parent))
import release  # noqa: E402  (the crate tag grammar and crates.io lookups)

ROOT = Path(__file__).resolve().parent.parent
CONFIG = "RELEASES.toml"
REGISTRIES = {"crates.io": "release.yml", "pypi": "pypi-release.yml"}
# The PyPI package's Rust manifest, which must carry the same version.
PYPI_CARGO = "crates/rich-py/Cargo.toml"


@dataclass(frozen=True)
class Package:
    name: str
    registry: str
    manifest: str
    version: str

    @property
    def tag(self):
        prefix = "python" if self.registry == "pypi" else self.name
        return f"{prefix}-v{self.version}"

    @property
    def workflow(self):
        return REGISTRIES[self.registry]

    def __str__(self):
        return f"{self.name}@{self.version} ({self.registry})"


def files_on_disk(root=ROOT):
    return lambda path: (root / path).read_text(encoding="utf-8")


def files_at(commit):
    """Read files as they are at `commit`, whatever is checked out."""
    return lambda path: subprocess.check_output(
        ["git", "show", f"{commit}:{path}"], cwd=ROOT, text=True
    )


def load(read):
    entries = tomllib.loads(read(CONFIG)).get("package", [])
    packages = []
    for entry in entries:
        missing = {"name", "registry", "manifest", "version"} - entry.keys()
        if missing:
            raise ValueError(f"{CONFIG}: an entry lacks {', '.join(sorted(missing))}: {entry}")
        packages.append(Package(entry["name"], entry["registry"], entry["manifest"], entry["version"]))
    return packages


def manifest_version(read, package):
    data = tomllib.loads(read(package.manifest))
    if package.registry == "pypi":
        return data["project"]["version"]
    return data["package"]["version"]


def workspace_crates(read):
    """Publishable workspace crates: name -> (manifest path, internal dependencies)."""
    root = tomllib.loads(read("Cargo.toml"))
    aliases = {
        key: spec.get("package", key) if isinstance(spec, dict) else key
        for key, spec in root["workspace"].get("dependencies", {}).items()
    }
    crates = {}
    for member in root["workspace"]["members"]:
        path = f"{member}/Cargo.toml"
        manifest = tomllib.loads(read(path))
        package = manifest["package"]
        if package.get("publish") in (False, []):
            continue
        tables = [manifest.get("dependencies", {}), manifest.get("build-dependencies", {})]
        for target in manifest.get("target", {}).values():
            tables += [target.get("dependencies", {}), target.get("build-dependencies", {})]
        dependencies = set()
        for table in tables:
            for key, spec in table.items():
                spec = spec if isinstance(spec, dict) else {}
                name = aliases.get(key, key) if spec.get("workspace") else spec.get("package", key)
                dependencies.add(name)
        crates[package["name"]] = (path, dependencies)
    for name, (path, dependencies) in crates.items():
        crates[name] = (path, dependencies & set(crates) - {name})
    return crates


def check(read):
    """Every way RELEASES.toml can disagree with the repository, as messages."""
    errors = []
    try:
        packages = load(read)
    except (ValueError, tomllib.TOMLDecodeError) as error:
        return [str(error)]
    seen = set()
    for package in packages:
        if package.registry not in REGISTRIES:
            errors.append(f"{package.name}: unknown registry {package.registry!r} "
                          f"(use {' or '.join(REGISTRIES)})")
            continue
        if (package.registry, package.name) in seen:
            errors.append(f"{package}: listed twice")
        seen.add((package.registry, package.name))
        if not re.fullmatch(release.VERSION, package.version):
            errors.append(f"{package.name}: {package.version!r} is not a version")
        try:
            actual = manifest_version(read, package)
        except (subprocess.CalledProcessError, FileNotFoundError, KeyError):
            errors.append(f"{package.name}: cannot read the version in {package.manifest}")
            continue
        if actual != package.version:
            errors.append(f"{package.name}: {CONFIG} says {package.version}, "
                          f"{package.manifest} says {actual} (run release_cohort.py sync)")
        if package.registry == "pypi":
            cargo = tomllib.loads(read(PYPI_CARGO))["package"]["version"]
            if cargo != actual:
                errors.append(f"{package.name}: {package.manifest} says {actual}, {PYPI_CARGO} says {cargo}")
    if sum(p.registry == "pypi" for p in packages) != 1:
        errors.append(f"{CONFIG} must list exactly one PyPI package")

    crates = workspace_crates(read)
    listed = [p for p in packages if p.registry == "crates.io"]
    names = [p.name for p in listed]
    for name in sorted(set(crates) - set(names)):
        errors.append(f"{name}: a publishable workspace crate missing from {CONFIG}")
    for name in sorted(set(names) - set(crates)):
        errors.append(f"{name}: in {CONFIG} but not a publishable workspace crate")
    for package in listed:
        if package.name in crates and crates[package.name][0] != package.manifest:
            errors.append(f"{package.name}: manifest is {crates[package.name][0]}, not {package.manifest}")
    position = {name: index for index, name in enumerate(names)}
    for name in names:
        for dependency in sorted(crates.get(name, (None, set()))[1]):
            if dependency in position and position[dependency] > position[name]:
                errors.append(f"{name} is listed before {dependency}, which it depends on")
    return errors


def sync(path=ROOT / CONFIG, read=None):
    """Rewrite each entry's version from its manifest; return the entries changed."""
    read = read or files_on_disk(path.parent)
    text = path.read_text(encoding="utf-8")
    chunks = re.split(r"(?m)^(?=\[\[package\]\]\s*$)", text)
    changed = []
    for index, chunk in enumerate(chunks):
        match = re.search(r'(?m)^manifest\s*=\s*"([^"]+)"', chunk)
        name = re.search(r'(?m)^name\s*=\s*"([^"]+)"', chunk)
        registry = re.search(r'(?m)^registry\s*=\s*"([^"]+)"', chunk)
        if not (match and name and registry):
            continue
        version = manifest_version(read, Package(name[1], registry[1], match[1], ""))
        updated = re.sub(r'(?m)^version\s*=\s*"[^"]*"', f'version = "{version}"', chunk, count=1)
        if updated != chunk:
            changed.append(f"{name[1]} ({registry[1]}) -> {version}")
            chunks[index] = updated
    path.write_text("".join(chunks), encoding="utf-8")
    return changed


def http_status(url):
    request = Request(url, headers={"User-Agent": "rs-rich-release"})
    try:
        with urlopen(request, timeout=30) as response:
            return response.status
    except HTTPError as error:
        return error.code


def registry_status(package):
    """HTTP status of the exact version on its registry: 200 published, 404 not."""
    if package.registry == "pypi":
        return http_status(f"https://pypi.org/pypi/{package.name}/{package.version}/json")
    return release.registry_status(package.name, package.version)


def is_published(package, status=registry_status):
    code = status(package)
    if code == 200:
        return True
    if code == 404:
        return False
    raise RuntimeError(f"Could not check {package} (HTTP {code})")


def selected_by(tag, packages):
    """The RELEASES.toml entries a release tag publishes, checked against the tag."""
    if tag.startswith("python-v"):
        version = tag.removeprefix("python-v")
        chosen = [p for p in packages if p.registry == "pypi"]
    else:
        match = release.TAG.fullmatch(tag)
        if not match:
            raise ValueError(f"Invalid release tag: {tag!r}")
        crate, version = match.groups()
        chosen = [p for p in packages if p.registry == "crates.io" and (crate is None or p.name == crate)]
    if not chosen:
        raise ValueError(f"{tag}: no {CONFIG} entry for this package")
    for package in chosen:
        if package.version != version:
            raise ValueError(f"{tag}: {CONFIG} has {package.name} at {package.version}, not {version}")
    return chosen


def unreleased(tag, packages, status=registry_status):
    """Raise when anything the tag selects is already on its registry."""
    released = []
    for package in selected_by(tag, packages):
        if is_published(package, status):
            released.append(str(package))
        else:
            print(f"{package} is not yet published", flush=True)
    if released:
        raise RuntimeError(f"{tag}: already released: {', '.join(released)}. "
                           "A published version is permanent; bump the version instead.")


def git(*args, capture=True):
    result = subprocess.run(["git", *args], cwd=ROOT, check=True, text=True,
                            stdout=subprocess.PIPE if capture else None)
    return (result.stdout or "").strip()


def remote_tags(remote):
    """Tag name -> the commit it points at, on `remote`."""
    tags = {}
    for line in git("ls-remote", "--tags", remote).splitlines():
        sha, ref = line.split("\t")
        name = ref.removeprefix("refs/tags/")
        if name.endswith("^{}"):
            tags[name.removesuffix("^{}")] = sha      # an annotated tag's commit
        else:
            tags.setdefault(name, sha)
    return tags


def run_conclusion(package):
    """The latest release run for the package's tag: (status, conclusion, url), or None."""
    if not shutil.which("gh"):
        return None
    result = subprocess.run(
        ["gh", "run", "list", "--workflow", package.workflow, "--branch", package.tag, "--limit", "1",
         "--json", "status,conclusion,url", "--jq", '.[0] | "\\(.status) \\(.conclusion) \\(.url)"'],
        cwd=ROOT, text=True, capture_output=True,
    )
    fields = result.stdout.split()
    return tuple(fields) if result.returncode == 0 and len(fields) == 3 else None


def wait_for(package, timeout, status=registry_status, conclusion=run_conclusion, sleep=time.sleep):
    deadline = time.monotonic() + timeout
    while True:
        if is_published(package, status):
            print(f"{package} is published", flush=True)
            return
        run = conclusion(package)
        if run and run[0] == "completed" and run[1] != "success":
            raise RuntimeError(f"The {package.tag} release run ended {run[1]}: {run[2]}")
        if time.monotonic() > deadline:
            raise RuntimeError(f"{package} did not appear within {timeout // 60} minutes; "
                               f"check the {package.workflow} run for {package.tag}")
        sleep(30)


def tag(commit, remote, timeout, dry_run, status=registry_status, wait=wait_for):
    git("fetch", "--quiet", remote, "main", "--tags")
    sha = git("rev-parse", "--verify", f"{commit}^{{commit}}")
    if subprocess.run(["git", "merge-base", "--is-ancestor", sha, f"{remote}/main"], cwd=ROOT).returncode:
        raise RuntimeError(f"{sha} is not on {remote}/main; release tags go on main (docs/BRANCHING.md)")
    read = files_at(sha)
    if subprocess.run(["git", "cat-file", "-e", f"{sha}:{CONFIG}"], cwd=ROOT, capture_output=True).returncode:
        raise RuntimeError(f"{CONFIG} is not in {sha[:12]}; tag a commit that has it")
    if errors := check(read):
        raise RuntimeError(f"{CONFIG} at {sha[:12]} disagrees with the repository:\n  " + "\n  ".join(errors))
    existing = remote_tags(remote)
    print(f"Releasing from {sha}", flush=True)
    for package in load(read):
        if is_published(package, status):
            print(f"skip   {package.tag}: {package} is already on {package.registry}", flush=True)
            continue
        if package.tag in existing:
            if existing[package.tag] != sha:
                raise RuntimeError(f"{package.tag} exists on {remote} at {existing[package.tag][:12]}, "
                                   f"not {sha[:12]}, but {package} is not published; resolve it by hand")
            print(f"wait   {package.tag}: already tagged, waiting for its release", flush=True)
            if dry_run:
                continue
        elif dry_run:
            print(f"would  tag {package.tag} at {sha[:12]} and push it", flush=True)
            continue
        else:
            local = subprocess.run(["git", "rev-parse", "--verify", "--quiet", f"refs/tags/{package.tag}^{{commit}}"],
                                   cwd=ROOT, text=True, capture_output=True).stdout.strip()
            if local and local != sha:
                raise RuntimeError(f"A local tag {package.tag} points at {local[:12]}; delete it first")
            if not local:
                git("tag", "-a", package.tag, "-m", f"{package.name} {package.version}", sha)
            git("push", remote, f"refs/tags/{package.tag}", capture=False)
            print(f"tagged {package.tag}; waiting for {package.workflow}", flush=True)
        wait(package, timeout, status)
    print("Every package in RELEASES.toml is published." if not dry_run else "Dry run: nothing was tagged.")


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    commands = parser.add_subparsers(dest="command", required=True)
    commands.add_parser("check")
    commands.add_parser("sync")
    commands.add_parser("status")
    guard = commands.add_parser("unreleased")
    guard.add_argument("tag")
    tagger = commands.add_parser("tag")
    tagger.add_argument("--commit", default="origin/main", help="commit to tag (default: origin/main)")
    tagger.add_argument("--remote", default="origin")
    tagger.add_argument("--timeout", type=int, default=120, help="minutes to wait for each release")
    tagger.add_argument("--dry-run", action="store_true", help="show what would be tagged, then stop")
    args = parser.parse_args()

    try:
        if args.command == "check":
            if errors := check(files_on_disk()):
                raise RuntimeError(f"{CONFIG} disagrees with the repository:\n  " + "\n  ".join(errors))
            print(f"{CONFIG} matches every manifest")
        elif args.command == "sync":
            print("\n".join(sync()) or f"{CONFIG} already matches the manifests")
        elif args.command == "status":
            tags = remote_tags("origin")
            for package in load(files_on_disk()):
                published = "published" if is_published(package) else "unreleased"
                tagged = "tagged" if package.tag in tags else "-"
                print(f"{package.tag:<28} {package.registry:<10} {published:<11} {tagged}")
        elif args.command == "unreleased":
            unreleased(args.tag, load(files_on_disk()))
        else:
            tag(args.commit, args.remote, args.timeout * 60, args.dry_run)
    except (RuntimeError, ValueError, subprocess.CalledProcessError) as error:
        print(f"error: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
