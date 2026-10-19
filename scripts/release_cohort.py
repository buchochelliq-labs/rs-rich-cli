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
                It tags the local checkout (HEAD, or `--commit`) and fetches
                nothing, so a resumed run releases the same commit.
                `--registry crates.io` (or `pypi`) tags only that registry's
                packages, to hold the other back.

Requires Python 3.11+ and git; `tag` uses the GitHub CLI (`gh`), when present,
to stop as soon as a release run fails.
"""

import sys

if sys.version_info < (3, 11):
    sys.exit(f"release_cohort.py needs Python 3.11 or newer (for tomllib); this is {sys.version.split()[0]}")

import argparse
from dataclasses import dataclass
from pathlib import Path
import re
import shutil
import subprocess
import time
import tomllib
from urllib.error import HTTPError, URLError
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
        ["git", "show", f"{commit}:{path}"], cwd=ROOT, text=True, encoding="utf-8"
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
            # The name is what `unreleased` and `tag` look up on PyPI: it must
            # be the project pypi-release.yml uploads, or they ask about another.
            project = tomllib.loads(read(package.manifest))["project"]["name"]
            if project != package.name:
                errors.append(f"{package.name}: {package.manifest} names the PyPI project {project!r}")
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
    result = subprocess.run(["git", *args], cwd=ROOT, check=True, text=True, encoding="utf-8",
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
        cwd=ROOT, text=True, encoding="utf-8", capture_output=True,
    )
    fields = result.stdout.split()
    return tuple(fields) if result.returncode == 0 and len(fields) == 3 else None


def tag_run(package):
    """The latest release run for the package's tag, as a dict: status,
    conclusion, url, head_sha, and whether its publish job started; None
    when there is no GitHub CLI or no run."""
    if not shutil.which("gh"):
        return None
    result = subprocess.run(
        ["gh", "run", "list", "--workflow", package.workflow, "--branch", package.tag, "--limit", "1",
         "--json", "status,conclusion,url,headSha,databaseId",
         "--jq", '.[0] | "\\(.status) \\(.conclusion) \\(.url) \\(.headSha) \\(.databaseId)"'],
        cwd=ROOT, text=True, encoding="utf-8", capture_output=True,
    )
    fields = result.stdout.split()
    if result.returncode or len(fields) != 5:
        return None
    status, conclusion, url, head_sha, run_id = fields
    # A publish job that started may have uploaded before the run ended:
    # the registry can take a while to show it. Unknown counts as started.
    jobs = subprocess.run(
        ["gh", "api", f"repos/{{owner}}/{{repo}}/actions/runs/{run_id}/jobs",
         "--jq", '[.jobs[] | select((.name | startswith("publish")) and .status != "queued"'
                 ' and .conclusion != "skipped")] | length'],
        cwd=ROOT, text=True, encoding="utf-8", capture_output=True,
    )
    started = jobs.returncode != 0 or jobs.stdout.strip() != "0"
    return {"status": status, "conclusion": conclusion, "url": url, "head_sha": head_sha,
            "publish_started": started}


def dead_tag(run, tagged):
    """Whether a tag's release run proves nothing was published from it: it
    ran on the tagged commit, finished without success, and never started
    publishing."""
    return bool(run and run["head_sha"] == tagged and run["status"] == "completed"
                and run["conclusion"] != "success" and not run["publish_started"])


def wait_for(package, timeout, status=registry_status, conclusion=run_conclusion, sleep=time.sleep):
    """Return once the release is done: its run succeeded and the version is
    on the registry. The run uploads before it verifies the upload as a
    consumer would, so the version can appear while the run still fails;
    when the run cannot be seen (no `gh`), the registry alone decides."""
    deadline = time.monotonic() + timeout
    while True:
        published = is_published(package, status)
        run = conclusion(package)
        if run and run[0] == "completed" and run[1] != "success":
            raise RuntimeError(f"The {package.tag} release run ended {run[1]}: {run[2]}")
        if published and (run is None or run[0] == "completed"):
            print(f"{package} is published", flush=True)
            return
        if time.monotonic() > deadline:
            raise RuntimeError(f"{package} did not appear within {timeout // 60} minutes; "
                               f"check the {package.workflow} run for {package.tag}")
        sleep(30)


def glob_match(path, pattern):
    """Whether `path` matches a Cargo `include`/`exclude` glob: `**/` is any
    number of directories, `*` and `?` stay inside one, and a pattern with
    no `/` matches a file of that name at any depth."""
    if "/" not in pattern.rstrip("/"):
        pattern = "**/" + pattern
    regex, i = "", 0
    while i < len(pattern):
        if pattern.startswith("**/", i):
            regex, i = regex + "(?:.*/)?", i + 3
        elif pattern.startswith("**", i):
            regex, i = regex + ".*", i + 2
        elif pattern[i] == "*":
            regex, i = regex + "[^/]*", i + 1
        elif pattern[i] == "?":
            regex, i = regex + "[^/]", i + 1
        else:
            regex, i = regex + re.escape(pattern[i]), i + 1
    return re.fullmatch(regex, path) is not None


def packaged_changes(package, old, new, read):
    """The files a package ships that differ between commits `old` and `new`,
    by its manifest's `include` and `exclude` (all of its directory without
    an `include`)."""
    folder = str(Path(package.manifest).parent)
    names = git("diff", "--name-only", old, new, "--", folder).splitlines()
    manifest = tomllib.loads(read(package.manifest))
    section = manifest.get("package", manifest.get("project", {}))
    include, exclude = section.get("include"), section.get("exclude", [])
    shipped = []
    for name in names:
        inner = name[len(folder) + 1:]
        if include and not any(glob_match(inner, glob) for glob in include):
            continue
        if any(glob_match(inner, glob) for glob in exclude):
            continue
        shipped.append(inner)
    return shipped


def published_elsewhere(packages, existing, sha, remote, read, status):
    """Published packages whose tag is on another commit and whose shipped
    files differ at `sha`: releasing `sha` would put the rest of the cohort
    beside packages built from other code under the same numbers (the split
    cohort of rs-rich-intuituive 0.0.4). Returns {tag commit: [package
    (files)]}."""
    found = {}
    for package in packages:
        tagged = existing.get(package.tag)
        if not tagged or tagged == sha or not is_published(package, status):
            continue
        if subprocess.run(["git", "cat-file", "-e", f"{tagged}^{{commit}}"], cwd=ROOT,
                          capture_output=True).returncode:
            git("fetch", "--quiet", "--no-tags", remote, f"refs/tags/{package.tag}")
        if changed := packaged_changes(package, tagged, sha, read):
            shown = ", ".join(changed[:3]) + (", ..." if len(changed) > 3 else "")
            found.setdefault(tagged, []).append(f"{package} ({shown})")
    return found


def start_at(packages, start):
    """The index in `packages` of `start`: a package name or its tag."""
    for index, package in enumerate(packages):
        if start in (package.name, package.tag, f"{package.name}@{package.version}"):
            return index
    raise ValueError(f"--from {start}: no {CONFIG} entry has that name or tag")


def tag(commit, remote, timeout, dry_run, status=registry_status, wait=wait_for, registry=None,
        run=tag_run, start=None):
    # The local checkout decides what is released: nothing is fetched, so a
    # run that is resumed later tags the same commit, not whatever main has
    # moved on to since. Only the remote's tags and the registries are read.
    sha = git("rev-parse", "--verify", f"{commit}^{{commit}}")
    if subprocess.run(["git", "merge-base", "--is-ancestor", sha, f"{remote}/main"], cwd=ROOT).returncode:
        raise RuntimeError(f"{sha} is not on {remote}/main as last fetched; release tags go on main "
                           "(docs/BRANCHING.md)")
    read = files_at(sha)
    if subprocess.run(["git", "cat-file", "-e", f"{sha}:{CONFIG}"], cwd=ROOT, capture_output=True).returncode:
        raise RuntimeError(f"{CONFIG} is not in {sha[:12]}; tag a commit that has it")
    if errors := check(read):
        raise RuntimeError(f"{CONFIG} at {sha[:12]} disagrees with the repository:\n  " + "\n  ".join(errors))
    existing = remote_tags(remote)
    packages = load(read)
    if split := published_elsewhere(packages, existing, sha, remote, read, status):
        lines = [f"  released from {commit[:12]}: " + "; ".join(names) for commit, names in split.items()]
        hint = " or ".join(f"--commit {commit[:12]}" for commit in split)
        raise RuntimeError(
            f"{sha[:12]} changes packages already published under the same version from other commits:\n"
            + "\n".join(lines)
            + f"\nRelease from the commit they came from ({hint}), or bump them first.")
    first = start_at(packages, start) if start else 0
    if first:
        # Packages before --from are left as they are. One a later crate
        # needs at a version not yet on crates.io would make that crate's
        # publish fail, so it is refused before anything is tagged.
        held = {p.name: p for p in packages[:first]
                if p.registry == "crates.io" and not is_published(p, status)}
        needs = {name: deps for name, (_, deps) in workspace_crates(read).items()}
        blocked = [f"{p.name} needs {held[d]}" for p in packages[first:] if p.registry == "crates.io"
                   for d in sorted(needs.get(p.name, ())) if d in held]
        if blocked:
            raise RuntimeError(f"--from {start} leaves unpublished versions that later crates need: "
                               + "; ".join(blocked) + ". Start from the first of those instead.")
    print(f"Releasing from {sha}", flush=True)
    for index, package in enumerate(packages):
        if index < first:
            print(f"leave  {package.tag}: before --from {start}", flush=True)
            continue
        if registry and package.registry != registry:
            print(f"leave  {package.tag}: only {registry} packages were asked for", flush=True)
            continue
        if is_published(package, status):
            print(f"skip   {package.tag}: {package} is already on {package.registry}", flush=True)
            continue
        if package.tag in existing and existing[package.tag] != sha:
            # A tag on another commit whose version is not on the registry.
            # Moved here only when its release run, on that commit, ended
            # without success and never started publishing; anything less
            # certain stops.
            old = existing[package.tag][:12]
            latest = run(package)
            if not dead_tag(latest, existing[package.tag]):
                if not latest:
                    state = "no release run can be seen"
                elif latest["head_sha"] != existing[package.tag]:
                    state = f"its latest release run is on {latest['head_sha'][:12]}, not the tag"
                elif latest["status"] != "completed":
                    state = f"its release run is {latest['status']}: {latest['url']}"
                elif latest["publish_started"]:
                    state = f"its release run started publishing: {latest['url']}"
                else:
                    state = f"its release run succeeded: {latest['url']}"
                raise RuntimeError(
                    f"{package.tag} exists on {remote} at {old}, not {sha[:12]}, and {package} is not "
                    f"published; {state}. If nothing will come of it, delete it and run again:\n"
                    f"  git push {remote} :refs/tags/{package.tag} && git tag -d {package.tag}")
            print(f"move   {package.tag}: its run at {old} ended {latest['conclusion']} before publishing; "
                  f"tagging {sha[:12]}", flush=True)
            if dry_run:
                continue
            git("push", remote, f":refs/tags/{package.tag}", capture=False)
            if subprocess.run(["git", "rev-parse", "--verify", "--quiet", f"refs/tags/{package.tag}"],
                              cwd=ROOT, capture_output=True).returncode == 0:
                git("tag", "-d", package.tag)
            del existing[package.tag]
        if package.tag in existing:
            print(f"wait   {package.tag}: already tagged, waiting for its release", flush=True)
            if dry_run:
                continue
        elif dry_run:
            print(f"would  tag {package.tag} at {sha[:12]} and push it", flush=True)
            continue
        else:
            local = subprocess.run(["git", "rev-parse", "--verify", "--quiet", f"refs/tags/{package.tag}^{{commit}}"],
                                   cwd=ROOT, text=True, encoding="utf-8", capture_output=True).stdout.strip()
            if local and local != sha:
                # Not on the remote, so never released: a leftover of an
                # earlier run (or of a remote tag deleted by hand). Replace it.
                print(f"drop   local {package.tag} at {local[:12]}: it is not on {remote}", flush=True)
                git("tag", "-d", package.tag)
                local = ""
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
    tagger.add_argument("--commit", default="HEAD",
                        help="commit to tag (default: HEAD, the local checkout): any commit on main, "
                             "such as an earlier one known to be good; its RELEASES.toml decides")
    tagger.add_argument("--remote", default="origin")
    tagger.add_argument("--timeout", type=int, default=120, help="minutes to wait for each release")
    tagger.add_argument("--dry-run", action="store_true", help="show what would be tagged, then stop")
    tagger.add_argument("--from", dest="start", metavar="PACKAGE",
                        help="start at this package (its name or tag) in RELEASES.toml order, leaving "
                             "the ones before it; refused when a later crate needs one of them unpublished")
    tagger.add_argument("--registry", choices=sorted(REGISTRIES),
                        help="tag only the packages released to this registry (default: all)")
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
            tag(args.commit, args.remote, args.timeout * 60, args.dry_run, registry=args.registry,
                start=args.start)
    except (RuntimeError, ValueError, subprocess.CalledProcessError) as error:
        print(f"error: {error}", file=sys.stderr)
        return 1
    except URLError as error:
        # Raised when a registry cannot be reached at all; an HTTP status is
        # handled where it is read. python.org's macOS installer leaves Python
        # without CA certificates until "Install Certificates.command" runs.
        print(f"error: could not reach crates.io or PyPI: {error.reason}", file=sys.stderr)
        if "CERTIFICATE_VERIFY_FAILED" in str(error.reason):
            print("hint: on macOS, run \"Install Certificates.command\" from your Python's "
                  "Applications folder, then try again", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
