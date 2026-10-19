"""Release numbers (RELEASES.toml): check them, show them, and tag what is due.

RELEASES.toml lists every package this repository publishes with its version
and its place in the publication order. This script keeps it honest and acts
on it:

    check       RELEASES.toml matches every manifest, lists every publishable
                crate once, and orders each after its workspace dependencies.
    sync        rewrite RELEASES.toml's versions from the manifests.
    status      each package's version, tag, and whether it is on its
                registry, and the release tag.
    unreleased  what a tag's run publishes, failing when that is already out
                (release.yml and pypi-release.yml, first). For a release tag,
                every crate not yet on crates.io, each tagged on its commit;
                none due fails. Writes the selection to GITHUB_OUTPUT.
    published   what a verify_only run checks: what the tag released, all on
                crates.io already (a release tag's crates tagged on its commit).
    tag         tag the release on its commit and wait for it: an annotated
                tag for every crate not yet on crates.io (records, which start
                nothing), then `release-<name>`, which starts the one
                release.yml run that publishes them all, and `python-v...` if
                PyPI is due. A published version is never tagged.
                It picks the commit itself: the one a release already in
                progress was tagged from, else the local checkout (HEAD).
                `--release 0.0.20` releases that one (from the commit its
                tags are on, else the last commit on main naming it), and
                `--commit` names the commit outright. Main is never fetched.
                `--registry crates.io` (or `pypi`) tags only that registry's
                packages, to hold the other back. Run again after a failed
                run, it pushes the tag again; the new run publishes only
                what is still due.
    same-commit fail when a release tag's commit is not the one the rest of
                its release was tagged from, and the packages differ between
                the two (release.yml and pypi-release.yml, before building).

A release is named by the version of the last crate in RELEASES.toml
(rs-rich-cli): the 0.0.20 release is the one with rs-rich-cli 0.0.20, and
its release tag is `release-0.0.20`. Each tag `tag` creates records its
release and its commit in the tag's message:

    rs-rich-ext 0.0.17

    Release: 0.0.20
    Commit: <the commit tagged>

Requires Python 3.11+ and git; `tag` uses the GitHub CLI (`gh`), when present,
to follow the runs: to stop as soon as one fails, and to push a failed run's
tag again when re-run.
"""

import sys

if sys.version_info < (3, 11):
    sys.exit(f"release_cohort.py needs Python 3.11 or newer (for tomllib); this is {sys.version.split()[0]}")

import argparse
from dataclasses import dataclass
import json
import os
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
        ["git", "show", f"{commit}:{path}"], cwd=ROOT, text=True, encoding="utf-8",
        stderr=subprocess.DEVNULL,
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


def release_name(packages):
    """The release a RELEASES.toml describes: its last crate's version (rs-rich-cli's)."""
    return [p for p in packages if p.registry == "crates.io"][-1].version


def version_key(version):
    return tuple(int(part) for part in re.findall(r"\d+", version))


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


def release_tag(name):
    """The tag that releases every due crate of release `name` in one run."""
    return f"release-{name}"


def workflow_of(tag_name):
    return REGISTRIES["pypi" if tag_name.startswith("python-v") else "crates.io"]


def selected_by(tag, packages):
    """The RELEASES.toml entries a release tag publishes, checked against the
    tag: a `release-X.Y.Z` tag every crate of release X.Y.Z (of which its run
    publishes those not yet out), a crate tag its crate, `python-v` PyPI's."""
    if match := release.RELEASE.fullmatch(tag):
        name = release_name(packages)
        if match[1] != name:
            raise ValueError(f"{tag}: {CONFIG} here is release {name}, not {match[1]}")
        return [p for p in packages if p.registry == "crates.io"]
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


def unreleased(tag, packages, status=registry_status, existing=None, sha=None):
    """What the tag's run publishes; raise when that is already on its
    registry. A crate or `python-v` tag: its package, which must not be out.
    A release tag: every crate of the release not yet on crates.io, which
    must not be none, each with its own tag (a record, `tag` pushes it) on
    the release tag's commit `sha` in `existing` (tag -> commit)."""
    chosen = selected_by(tag, packages)
    if release.RELEASE.fullmatch(tag):
        due = []
        for package in chosen:
            if is_published(package, status):
                print(f"{package} is already published; not selected", flush=True)
            else:
                print(f"{package} is not yet published; selected", flush=True)
                due.append(package)
        if not due:
            raise RuntimeError(f"{tag}: every crate of release {release_name(packages)} is already on "
                               "crates.io; nothing to publish")
        untagged = [f"{p.tag} ({'missing' if p.tag not in existing else 'at ' + existing[p.tag][:12]})"
                    for p in due if existing.get(p.tag) != sha]
        if untagged:
            raise RuntimeError(f"{tag}: each selected crate's tag must be on {sha[:12]}, the release "
                               f"commit: {', '.join(untagged)}. release_cohort.py tag pushes them.")
        return due
    released = []
    for package in chosen:
        if is_published(package, status):
            released.append(str(package))
        else:
            print(f"{package} is not yet published", flush=True)
    if released:
        raise RuntimeError(f"{tag}: already released: {', '.join(released)}. "
                           "A published version is permanent; bump the version instead.")
    return chosen


def published(tag, packages, status=registry_status, existing=None, sha=None):
    """What a `verify_only` run checks: what the tag released, all of which
    must be out. A release tag: the crates tagged on its commit `sha`."""
    chosen = selected_by(tag, packages)
    if release.RELEASE.fullmatch(tag):
        chosen = [p for p in chosen if existing.get(p.tag) == sha]
        if not chosen:
            raise RuntimeError(f"{tag}: no crate of the release is tagged on {sha[:12]}")
    missing = [str(p) for p in chosen if not is_published(p, status)]
    if missing:
        raise RuntimeError(f"{tag}: not on its registry yet: {', '.join(missing)}; verify_only "
                           "cannot complete a partial upload")
    return chosen


def write_selection(chosen):
    """Hand the crates a run publishes to the next step, as {name: version}
    in RELEASES.toml order (GITHUB_OUTPUT `selection`)."""
    crates = {p.name: p.version for p in chosen if p.registry == "crates.io"}
    encoded = json.dumps(crates, separators=(",", ":"))
    print(f"selection={encoded}", flush=True)
    if crates and (output := os.environ.get("GITHUB_OUTPUT")):
        with open(output, "a", encoding="utf-8") as stream:
            stream.write(f"selection={encoded}\n")


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


def run_conclusion(tag_name):
    """The latest run for a tag: (status, conclusion, url), or None."""
    if not shutil.which("gh"):
        return None
    result = subprocess.run(
        ["gh", "run", "list", "--workflow", workflow_of(tag_name), "--branch", tag_name, "--limit", "1",
         "--json", "status,conclusion,url", "--jq", '.[0] | "\\(.status) \\(.conclusion) \\(.url)"'],
        cwd=ROOT, text=True, encoding="utf-8", capture_output=True,
    )
    fields = result.stdout.split()
    return tuple(fields) if result.returncode == 0 and len(fields) == 3 else None


def tag_run(tag_name):
    """The latest run for a tag, as a dict: status, conclusion, url,
    head_sha, and whether its publish job started; None when there is no
    GitHub CLI or no run. A run dispatched by hand is on its branch (main),
    not the tag, and is not seen."""
    if not shutil.which("gh"):
        return None
    result = subprocess.run(
        ["gh", "run", "list", "--workflow", workflow_of(tag_name), "--branch", tag_name, "--limit", "1",
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


def run_tag_of(package, existing):
    """The tag whose run publishes `package` as it is tagged in `existing`:
    for a crate, the release tag on the same commit (its own tag is only a
    record), else its own tag (a crate tag pushed alone, as before release
    tags, or `python-v`)."""
    tagged = existing.get(package.tag)
    if package.registry == "crates.io" and tagged:
        for name, commit in sorted(existing.items()):
            if commit == tagged and release.RELEASE.fullmatch(name):
                return name
    return package.tag


def dead_tag(run, tagged):
    """Whether a tag's release run proves nothing was published from it: it
    ran on the tagged commit, finished without success, and never started
    publishing."""
    return bool(run and run["head_sha"] == tagged and run["status"] == "completed"
                and run["conclusion"] != "success" and not run["publish_started"])


def wait_for(tag_name, packages, timeout, status=registry_status, conclusion=run_conclusion, sleep=time.sleep,
             stale=None):
    """Return once the run of `tag_name` is done: it succeeded and every one
    of `packages` is on its registry. The run uploads before it verifies the
    upload as a consumer would, so the versions can appear while the run
    still fails; when the run cannot be seen (no `gh`), the registry alone
    decides. A run at the URL `stale` (the one a re-pushed tag replaces) is
    not it."""
    deadline = time.monotonic() + timeout
    workflow = workflow_of(tag_name)
    while True:
        missing = [p for p in packages if not is_published(p, status)]
        run = conclusion(tag_name)
        if run and run[2] == stale:
            run = ("queued", "", stale)
        if run and run[0] == "completed" and run[1] != "success":
            out = len(packages) - len(missing)
            raise RuntimeError(f"The {tag_name} run ended {run[1]}: {run[2]} ({out} of {len(packages)} "
                               "published). Fix the cause, then run `release_cohort.py tag` again: it "
                               "pushes the tag again, and the run publishes only what is still due "
                               "(docs/BRANCHING.md, Re-running a release).")
        if not missing and (run is None or run[0] == "completed"):
            print(f"{tag_name}: {', '.join(str(p) for p in packages)} published", flush=True)
            return
        if time.monotonic() > deadline:
            raise RuntimeError(f"{', '.join(str(p) for p in missing) or tag_name} did not finish within "
                               f"{timeout // 60} minutes; check the {workflow} run for {tag_name}")
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


def have_commits(existing, remote):
    """Fetch the objects of any tagged commit this clone lacks (only those
    tags; main is never fetched)."""
    missing = sorted(name for name, sha in existing.items()
                     if subprocess.run(["git", "cat-file", "-e", f"{sha}^{{commit}}"], cwd=ROOT,
                                       capture_output=True).returncode)
    if missing:
        git("fetch", "--quiet", "--no-tags", remote, *(f"refs/tags/{name}" for name in missing))


def releases_tagged(existing):
    """Commit -> its RELEASES.toml entries, for every commit some release tag
    was cut from: a tag on the commit names a version that commit lists, or
    is its release tag."""
    found = {}
    for commit in set(existing.values()):
        try:
            packages = load(files_at(commit))
            release_name(packages)
        except (subprocess.CalledProcessError, ValueError, IndexError, tomllib.TOMLDecodeError):
            continue
        if any(existing.get(p.tag) == commit for p in packages) or \
                existing.get(release_tag(release_name(packages))) == commit:
            found[commit] = packages
    return found


def newest_first(commits):
    return sorted(commits, key=lambda c: int(git("rev-list", "--count", c)), reverse=True)


def split_from(packages, existing, sha, read, binds):
    """Packages whose tag is on another commit, and whose shipped files differ
    at `sha`: releasing `sha` would put the rest of the release beside
    packages built from other code under the same numbers (the split
    release of rs-rich-intuituive 0.0.4). `binds(package, tagged)` says
    whether such a tag holds this release to its commit. Returns {tag
    commit: [package (files)]}."""
    found = {}
    for package in packages:
        tagged = existing.get(package.tag)
        if not tagged or tagged == sha or not binds(package, tagged):
            continue
        if changed := packaged_changes(package, tagged, sha, read):
            shown = ", ".join(changed[:3]) + (", ..." if len(changed) > 3 else "")
            found.setdefault(tagged, []).append(f"{package} ({shown})")
    return found


def split_error(sha, split, what="changes packages already released under the same version"):
    lines = [f"  tagged at {commit[:12]}: " + "; ".join(names) for commit, names in split.items()]
    hint = " or ".join(f"--commit {commit[:12]}" for commit in split)
    return (f"{sha[:12]} {what} from other commits:\n" + "\n".join(lines)
            + f"\nRelease from the commit they came from ({hint}), or bump them first. A tag "
              "whose run will never publish can be deleted: git push origin :refs/tags/<tag> "
              "&& git tag -d <tag>")


def resolve(commit, release_wanted, remote, existing, binds, published):
    """The commit to release and why: `--commit`; else the commit the release
    (`--release`, or the newest one tagged) was started from, while any of
    it is still due; else, for `--release`, the last commit on main naming
    it; else HEAD."""
    if commit:
        return git("rev-parse", "--verify", f"{commit}^{{commit}}"), "--commit"
    have_commits(existing, remote)
    tagged = releases_tagged(existing)
    names = {c: release_name(p) for c, p in tagged.items()}
    wanted = release_wanted.removeprefix("v") if release_wanted else None
    target = wanted or max(names.values(), key=version_key, default=None)
    started = newest_first([c for c in tagged if names[c] == target])
    for candidate in started:
        packages = tagged[candidate]
        # Held only by tags whose runs provably published nothing, the
        # release never really started there: those tags are moved.
        if not any(binds(p, candidate) for p in packages if existing.get(p.tag) == candidate):
            continue
        if split_from(packages, existing, candidate, files_at(candidate), binds):
            continue
        due = [p for p in packages if not published(p)]
        if due or wanted:
            shown = ", ".join(sorted(t for t, c in existing.items() if c == candidate))
            return candidate, f"release {target}, already tagged there: {shown}"
    if started and wanted:
        raise RuntimeError(f"release {wanted}'s tags are on {', '.join(c[:12] for c in started)}, "
                           "and none of them can take the rest without splitting it; use --commit")
    if wanted:
        for candidate in git("rev-list", "--first-parent", f"{remote}/main").splitlines():
            try:
                name = release_name(load(files_at(candidate)))
            except (subprocess.CalledProcessError, ValueError, IndexError, tomllib.TOMLDecodeError):
                break
            if name == wanted:
                return candidate, f"release {wanted}: the last commit on {remote}/main naming it"
            if version_key(name) < version_key(wanted):
                break
        raise RuntimeError(f"no commit on {remote}/main (as last fetched) names release {wanted} "
                           f"(the version of the last crate in {CONFIG})")
    return git("rev-parse", "--verify", "HEAD^{commit}"), "the local checkout (HEAD)"


def tag_message(package, release_wanted, sha):
    """A package tag's message: the package, then its release and commit."""
    return f"{package.name} {package.version}\n\nRelease: {release_wanted}\nCommit: {sha}\n"


def release_message(name, due, sha):
    """A release tag's message: the release, the crates due when it was
    pushed, then its release and commit."""
    crates = "".join(f"{p.name} {p.version}\n" for p in due)
    return f"Release {name}\n\n{crates}\nRelease: {name}\nCommit: {sha}\n"


def tag_trailers(tag_name):
    """The `Key: value` lines of a local tag's message."""
    text = git("for-each-ref", f"refs/tags/{tag_name}", "--format=%(contents)")
    return dict(re.findall(r"(?m)^(Release|Commit): (\S+)$", text))


def same_commit(tag_name, remote):
    """Fail when a release tag (checked out) would split its release: another
    package of the same RELEASES.toml is tagged on another commit and ships
    other files under its number, or the tag's recorded commit is not the
    one it points at. Returns a summary of the tag, its release and commit."""
    sha = git("rev-parse", "--verify", f"refs/tags/{tag_name}^{{commit}}")
    read = files_at(sha)
    packages = load(read)
    name = release_name(packages)
    recorded = tag_trailers(tag_name)
    if release.RELEASE.fullmatch(tag_name) and recorded.keys() != {"Release", "Commit"}:
        raise RuntimeError(f"{tag_name} must record its release and commit (Release: and Commit: "
                           "lines in its message, as release_cohort.py tag writes them)")
    if recorded.get("Commit") and recorded["Commit"] != sha:
        raise RuntimeError(f"{tag_name} records commit {recorded['Commit']} but points at {sha}")
    if recorded.get("Release") and recorded["Release"] != name:
        raise RuntimeError(f"{tag_name} records release {recorded['Release']}, but {CONFIG} at "
                           f"{sha[:12]} is release {name}")
    existing = remote_tags(remote)
    have_commits(existing, remote)
    selected_by(tag_name, packages)  # the tag names versions this RELEASES.toml lists
    # Every package's own tag is compared, the tag's own included: a release
    # tag (or an all-crates `vX.Y.Z`) is checked against each crate's tag.
    if split := split_from(packages, existing, sha, read, lambda package, tagged: True):
        raise RuntimeError(split_error(sha, split, f"would split release {name}: it changes packages tagged"))
    siblings = sorted(p.tag for p in packages if existing.get(p.tag) == sha)
    return (f"{tag_name} -> {sha} (release {name}). Tagged on this commit: "
            f"{', '.join(siblings) or tag_name}.")


RELEASE_WORKFLOW = ".github/workflows/release.yml"


def starts_on_release_tags(read):
    """Whether a commit's release.yml runs on a pushed `release-*` tag."""
    try:
        text = read(RELEASE_WORKFLOW)
    except (subprocess.CalledProcessError, FileNotFoundError):
        return False
    return re.search(r'(?m)^\s+tags:\s*\[[^\]]*"release-\*"', text) is not None


def local_tag(name):
    """The commit a local tag points at, or ''."""
    return subprocess.run(["git", "rev-parse", "--verify", "--quiet", f"refs/tags/{name}^{{commit}}"],
                          cwd=ROOT, text=True, encoding="utf-8", capture_output=True).stdout.strip()


def tag(commit, remote, timeout, dry_run, status=registry_status, wait=wait_for, registry=None,
        run=tag_run, release_wanted=None):
    """Tag a release on its commit and wait for it to be published.

    Every crate due (not yet on crates.io) gets its own annotated tag, a
    record that triggers nothing; the release tag `release-<name>` then
    starts the one release.yml run that publishes all of them, and
    `python-v<version>` the PyPI run, if PyPI is due. Record tags go up in
    one atomic push, the tags that start runs in a second one: GitHub starts
    no workflow for a push of more than three tags."""
    # The local checkout decides what is released: main is never fetched, so
    # a run that is resumed later tags the same commit, not whatever main has
    # moved on to since. Only the remote's tags and the registries are read.
    existing = remote_tags(remote)

    def binds(package, tagged):
        # A tag elsewhere holds the release to its commit unless the run that
        # publishes it provably published nothing: then it is moved here.
        return is_published(package, status) or not dead_tag(run(run_tag_of(package, existing)), tagged)

    sha, why = resolve(commit, release_wanted, remote, existing, binds,
                       lambda package: is_published(package, status))
    if subprocess.run(["git", "merge-base", "--is-ancestor", sha, f"{remote}/main"], cwd=ROOT).returncode:
        raise RuntimeError(f"{sha} is not on {remote}/main as last fetched; release tags go on main "
                           "(docs/BRANCHING.md)")
    read = files_at(sha)
    if subprocess.run(["git", "cat-file", "-e", f"{sha}:{CONFIG}"], cwd=ROOT, capture_output=True).returncode:
        raise RuntimeError(f"{CONFIG} is not in {sha[:12]}; tag a commit that has it")
    if errors := check(read):
        raise RuntimeError(f"{CONFIG} at {sha[:12]} disagrees with the repository:\n  " + "\n  ".join(errors))
    packages = load(read)
    name = release_name(packages)
    have_commits(existing, remote)
    if split := split_from(packages, existing, sha, read, binds):
        raise RuntimeError(split_error(sha, split))
    print(f"Release {name} from {sha} ({why})", flush=True)

    deletions = []        # remote tags to delete first: moved, or pushed again
    records, starts = [], []  # (tag, message) to create here and push
    waits = []            # (tag whose run publishes, packages, stale run URL)
    due = []              # crates the release tag's run will publish
    for package in packages:
        if registry and package.registry != registry:
            print(f"leave  {package.tag}: only {registry} packages were asked for", flush=True)
            continue
        if is_published(package, status):
            print(f"skip   {package.tag}: {package} is already on {package.registry}", flush=True)
            continue
        tagged = existing.get(package.tag)
        runner = run_tag_of(package, existing)
        latest = run(runner) if tagged else None
        if tagged and tagged != sha:
            # A tag on another commit whose version is not on the registry.
            # Moved here when the run that publishes it, on that commit,
            # ended without success and never started publishing. Otherwise
            # it stands, and the split check has shown it ships the same
            # files as `sha`: its own run is waited on.
            if not dead_tag(latest, tagged):
                print(f"wait   {package.tag}: tagged at {tagged[:12]}, which ships the same files; "
                      f"waiting for the {runner} run", flush=True)
                if not dry_run:
                    wait(runner, [package], timeout, status)
                continue
            print(f"move   {package.tag}: the {runner} run at {tagged[:12]} ended {latest['conclusion']} "
                  f"before publishing; tagging {sha[:12]}", flush=True)
            deletions.append(package.tag)
            tagged = None
        message = tag_message(package, name, sha)
        if package.registry == "pypi":
            stale = None
            if tagged and dead_tag(latest, sha):
                # Tagged here, but its run ended before publishing: push the
                # tag again to start a new run.
                print(f"retag  {package.tag}: its run ended before publishing; pushing it again", flush=True)
                deletions.append(package.tag)
                stale, tagged = latest["url"], None
            if tagged:
                print(f"wait   {package.tag}: already tagged here, waiting for its run", flush=True)
            else:
                print(f"tag    {package.tag} at {sha[:12]} (starts {package.workflow})", flush=True)
                starts.append((package.tag, message))
            waits.append((package.tag, [package], stale))
            continue
        if tagged:
            print(f"keep   {package.tag}: already tagged here", flush=True)
        else:
            print(f"tag    {package.tag} at {sha[:12]} (a record)", flush=True)
            records.append((package.tag, message))
        due.append(package)

    if due and not starts_on_release_tags(read):
        # The workflow a tag push runs is the tagged commit's: one from
        # before release tags (release 0.0.19's) starts nothing on one.
        raise RuntimeError(
            f"{sha[:12]}'s {RELEASE_WORKFLOW} does not start on release-* tags: it predates one-run "
            "releases, and a release tag pushed there would start nothing. Finish this release one "
            "tag at a time with the release_cohort.py of main before one-run releases (git worktree "
            "add ../release-old 5989aac && python3 ../release-old/scripts/release_cohort.py tag; "
            "docs/BRANCHING.md), or release a later commit (bump first if its crates changed).")
    if due:
        name_tag = release_tag(name)
        tagged, stale = existing.get(name_tag), None
        latest = run(name_tag) if tagged else None
        if tagged and tagged != sha:
            # Its crates' tags were here or moved here, so the run there is
            # dead or the split check stopped above; a live one stops here.
            if not dead_tag(latest, tagged):
                raise RuntimeError(f"{name_tag} is on {tagged[:12]}, not {sha[:12]}, and its run there has "
                                   "not provably published nothing; release from that commit "
                                   f"(--commit {tagged[:12]}), or wait for the run to end")
            print(f"move   {name_tag}: its run at {tagged[:12]} ended {latest['conclusion']} before "
                  f"publishing; tagging {sha[:12]}", flush=True)
            deletions.append(name_tag)
            tagged = None
        elif tagged and latest and latest["status"] == "completed" and latest["conclusion"] != "success":
            # Ended without success with crates still due: before publishing
            # (dead), or partway (some crates out). Either way a new run of
            # the same tag selects only what is still unpublished.
            what = "before publishing" if dead_tag(latest, sha) else "after publishing started"
            print(f"retag  {name_tag}: its run ended {latest['conclusion']} {what}; pushing it again, "
                  "and the new run publishes only what is still due", flush=True)
            deletions.append(name_tag)
            stale, tagged = latest["url"], None
        if tagged:
            print(f"wait   {name_tag}: already tagged here, waiting for its run", flush=True)
        else:
            print(f"tag    {name_tag} at {sha[:12]} (starts {REGISTRIES['crates.io']}: "
                  f"{', '.join(p.name for p in due)})", flush=True)
            starts.insert(0, (name_tag, release_message(name, due, sha)))
        waits.insert(0, (name_tag, due, stale))

    if dry_run:
        print("Dry run: nothing was tagged.")
        return
    if deletions:
        git("push", "--atomic", remote, *(f":refs/tags/{t}" for t in deletions), capture=False)
        for name_ in deletions:
            if local_tag(name_):
                git("tag", "-d", name_)
            existing.pop(name_, None)
    for tag_name, message in records + starts:
        local = local_tag(tag_name)
        if local and local != sha:
            # Not on the remote, so never released: a leftover of an earlier
            # run (or of a remote tag deleted by hand). Replace it.
            print(f"drop   local {tag_name} at {local[:12]}: it is not on {remote}", flush=True)
            git("tag", "-d", tag_name)
        elif local:
            git("tag", "-d", tag_name)     # made again with this run's message
        git("tag", "-a", tag_name, "-m", message, sha)
    if records:
        git("push", "--atomic", remote, *(f"refs/tags/{t}" for t, _ in records), capture=False)
    if starts:
        git("push", "--atomic", remote, *(f"refs/tags/{t}" for t, _ in starts), capture=False)
        print(f"pushed {', '.join(t for t, _ in starts)}; waiting for the runs", flush=True)
    for tag_name, chosen, stale in waits:
        wait(tag_name, chosen, timeout, status, stale=stale)
    print("Every package in RELEASES.toml is published." if not registry
          else f"Every {registry} package in RELEASES.toml is published.")


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    commands = parser.add_subparsers(dest="command", required=True)
    commands.add_parser("check")
    commands.add_parser("sync")
    commands.add_parser("status")
    guard = commands.add_parser("unreleased")
    guard.add_argument("tag")
    guard.add_argument("--remote", default="origin")
    done = commands.add_parser("published")
    done.add_argument("tag")
    done.add_argument("--remote", default="origin")
    split = commands.add_parser("same-commit")
    split.add_argument("tag")
    split.add_argument("--remote", default="origin")
    tagger = commands.add_parser("tag")
    which = tagger.add_mutually_exclusive_group()
    which.add_argument("--commit",
                       help="commit to tag: any commit on main, such as an earlier one known to be "
                            "good; its RELEASES.toml decides (default: the commit a release in "
                            "progress was tagged from, else HEAD)")
    which.add_argument("--release", dest="release_wanted", metavar="VERSION",
                       help="release this one (rs-rich-cli's version, such as 0.0.20 or v0.0.20): from "
                            "the commit its tags are on, else the last commit on main naming it")
    tagger.add_argument("--remote", default="origin")
    tagger.add_argument("--timeout", type=int, default=120, help="minutes to wait for each run")
    tagger.add_argument("--dry-run", action="store_true", help="show what would be tagged, then stop")
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
            packages = load(files_on_disk())
            due = 0
            for package in packages:
                out = is_published(package)
                due += not out and package.registry == "crates.io"
                tagged = f"tagged at {tags[package.tag][:12]}" if package.tag in tags else "-"
                print(f"{package.tag:<28} {package.registry:<10} {'published' if out else 'unreleased':<11} {tagged}")
            name_tag = release_tag(release_name(packages))
            tagged = f"tagged at {tags[name_tag][:12]}" if name_tag in tags else "-"
            print(f"{name_tag:<28} {'crates.io':<10} {f'{due} due' if due else 'done':<11} {tagged}")
        elif args.command in ("unreleased", "published"):
            existing = sha = None
            if release.RELEASE.fullmatch(args.tag):
                existing = remote_tags(args.remote)
                sha = git("rev-parse", "--verify", f"refs/tags/{args.tag}^{{commit}}")
            guard = unreleased if args.command == "unreleased" else published
            write_selection(guard(args.tag, load(files_on_disk()), existing=existing, sha=sha))
        elif args.command == "same-commit":
            summary = same_commit(args.tag, args.remote)
            print(summary)
            if path := os.environ.get("GITHUB_STEP_SUMMARY"):
                with open(path, "a", encoding="utf-8") as out:
                    out.write(summary + "\n")
        else:
            tag(args.commit, args.remote, args.timeout * 60, args.dry_run, registry=args.registry,
                release_wanted=args.release_wanted)
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
