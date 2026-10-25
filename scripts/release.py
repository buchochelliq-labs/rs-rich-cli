"""Select, publish, and verify a release of the crates.

A release tag (`release-X.Y.Z`, X.Y.Z being the release's name, rs-rich-cli's
version) publishes every crate of its RELEASES.toml not yet on crates.io, in
one run: release_cohort.py computes that selection, `plan` checks it against
the manifests, and `preflight`, `publish` and `verify` act on all of it at
once. A crate tag (`<crate>-vX.Y.Z`, by manual dispatch) still publishes that
crate alone. The workflow carries one validated selection between jobs; no
phase infers a different crate or version from the tag. Requires Python 3.11+
and Cargo.
"""

import argparse
import json
import os
from pathlib import Path
import re
import subprocess
import tempfile
import time
import tomllib
from urllib.error import HTTPError
from urllib.request import Request, urlopen


CRATES = ("rs-rich", "rs-rich-plugin-api", "rs-rich-macros", "rs-rich-ext", "rs-rich-cli", "rs-rich-art",
          "rs-rich-mermaid", "rs-rich-lumis", "rs-rich-record", "rs-rich-interact", "rs-rich-micro",
          "rs-rich-diagram", "rs-rich-data", "rs-rich-ratatui", "rs-rich-intuituive", "rs-rich-web", "rs-rich-embed",
          "rs-rich-marketplace")
NUMBER = r"(?:0|[1-9][0-9]*)"
PRERELEASE = rf"(?:{NUMBER}|[0-9]*[A-Za-z-][0-9A-Za-z-]*)"
VERSION = rf"{NUMBER}\.{NUMBER}\.{NUMBER}(?:-{PRERELEASE}(?:\.{PRERELEASE})*)?(?:\+[0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*)?"
TAG = re.compile(rf"(?:({'|'.join(CRATES)})-)?v({VERSION})")
# A release tag: one run publishes every crate of the release not yet out.
RELEASE = re.compile(rf"release-({VERSION})")


def validate_tag(tag):
    if not (TAG.fullmatch(tag) or RELEASE.fullmatch(tag)):
        raise ValueError(f"Invalid release tag: {tag!r}")
    ref = f"refs/tags/{tag}"
    kind = subprocess.check_output(["git", "cat-file", "-t", ref], text=True, encoding="utf-8").strip()
    if kind != "tag":
        raise ValueError(f"Release tag {tag!r} must be annotated, not lightweight")
    sha = subprocess.check_output(["git", "rev-parse", "--verify", f"{ref}^{{commit}}"],
                                  text=True, encoding="utf-8").strip()
    head = subprocess.check_output(["git", "rev-parse", "HEAD"], text=True, encoding="utf-8").strip()
    if sha != head:
        raise ValueError("Release tag must point at the checked-out commit")
    subprocess.run(["git", "merge-base", "--is-ancestor", sha, "origin/main"], check=True)
    return sha


def release_entries(text):
    """A RELEASES.toml's crates in order, {name: version}, and its release name."""
    crates = {entry["name"]: entry["version"] for entry in tomllib.loads(text).get("package", [])
              if entry.get("registry") == "crates.io"}
    if not crates:
        raise ValueError("RELEASES.toml lists no crates.io package")
    return crates, list(crates.values())[-1]


def select(tag, metadata, root, wanted=None, releases=None):
    """The {crate: version} a tag publishes, checked against the manifests and
    the internal requirements. A release tag publishes `wanted` (the gate's
    selection, in RELEASES.toml order), or, without it, every crate of the
    release in `releases` (RELEASES.toml's text); a crate tag its one crate,
    which `wanted` must then agree with."""
    if release_match := RELEASE.fullmatch(tag):
        if wanted is None:
            if releases is None:
                raise ValueError(f"{tag}: a release tag needs its selection (or RELEASES.toml)")
            wanted, name = release_entries(releases)
            if name != release_match[1]:
                raise ValueError(f"{tag}: RELEASES.toml names release {name}")
        if not isinstance(wanted, dict) or not wanted:
            raise ValueError(f"{tag}: the selection is empty")
        planned = dict(wanted)
    else:
        match = TAG.fullmatch(tag)
        if not match:
            raise ValueError(f"Invalid release tag: {tag!r}; use release-X.Y.Z or <crate>-vX.Y.Z")
        crate, version = match.groups()
        planned = {crate: version} if crate else dict.fromkeys(CRATES, version)
        if wanted is not None and wanted != planned:
            raise ValueError(f"{tag}: the selection {wanted} is not what the tag selects")
    members = set(metadata["workspace_members"])
    packages = {
        p["name"]: p for p in metadata["packages"]
        if p["id"] in members and p.get("publish") != []
    }
    if packages.keys() != set(CRATES):
        raise ValueError(f"Publishable workspace must contain exactly {CRATES}; got {sorted(packages)}")
    errors = []
    for name, version in planned.items():
        if name not in packages:
            errors.append(f"{name}: not a publishable workspace crate")
            continue
        if not isinstance(version, str) or not re.fullmatch(VERSION, version):
            errors.append(f"{name}: {version!r} is not a version")
            continue
        package = packages[name]
        if package["version"] != version:
            errors.append(f'{name}: manifest says {package["version"]}, the release says {version}')
        if package.get("publish") is not None and "crates-io" not in package["publish"]:
            errors.append(f"{name}: publishing to crates.io is disabled")
    # Keep the repository's exact internal-requirement policy, but compare each
    # requirement with its OWN crate, not the version of the release tag.
    requirements = root["workspace"]["dependencies"]
    for name in ("rs-rich", "rs-rich-plugin-api", "rs-rich-macros", "rs-rich-ext", "rs-rich-art",
                 "rs-rich-mermaid", "rs-rich-lumis", "rs-rich-record", "rs-rich-interact", "rs-rich-micro",
                 "rs-rich-diagram", "rs-rich-data", "rs-rich-ratatui", "rs-rich-intuituive", "rs-rich-web", "rs-rich-embed",
                 "rs-rich-marketplace"):
        key = name.removeprefix("rs-")
        dependency = requirements.get(key, {})
        expected = packages[name]["version"]
        if dependency.get("package") != name or dependency.get("version") != expected:
            errors.append(f"workspace dependency {key}: expected package {name}, version {expected}")
    if errors:
        raise ValueError("\n".join(errors))
    return planned


def registry_status(name, version):
    request = Request(f"https://crates.io/api/v1/crates/{name}/{version}",
                      headers={"User-Agent": "rs-rich-release"})
    try:
        with urlopen(request, timeout=30) as response:
            return response.status
    except HTTPError as error:
        return error.code


def crate_status(name):
    request = Request(f"https://crates.io/api/v1/crates/{name}",
                      headers={"User-Agent": "rs-rich-release"})
    try:
        with urlopen(request, timeout=30) as response:
            return response.status
    except HTTPError as error:
        return error.code


def preflight(selection):
    """Refuse versions that exist; return the selected crates not yet on crates.io.

    Trusted Publishing works only for a crate that already exists, so the
    workflow uploads a brand-new crate's first version with the `crates-io`
    environment's token instead (docs/BRANCHING.md, Registry authentication).
    New and existing crates may share a release: `publish` uploads each kind
    with its own credential (`publish_runs`).
    """
    new = []
    for name, version in selection.items():
        code = registry_status(name, version)
        if code == 200:
            raise RuntimeError(f"{name}@{version} already exists; refusing release")
        if code != 404:
            raise RuntimeError(f"Could not verify availability of {name}@{version} (HTTP {code})")
        crate = crate_status(name)
        if crate == 404:
            new.append(name)
        elif crate != 200:
            raise RuntimeError(f"Could not check whether {name} exists (HTTP {crate})")
        print(f"{name}@{version} is available{' (new crate)' if crate == 404 else ''}", flush=True)
    return new


def internal_dependencies():
    """Workspace crate -> the workspace crates it needs on crates.io first
    (normal and build dependencies; dev-dependencies are not published)."""
    metadata = json.loads(subprocess.check_output(
        ["cargo", "metadata", "--no-deps", "--format-version", "1", "--locked"], text=True, encoding="utf-8",
    ))
    names = {p["name"] for p in metadata["packages"]}
    return {p["name"]: {d["name"] for d in p["dependencies"]
                        if d["name"] in names and d.get("kind") in (None, "build")}
            for p in metadata["packages"]}


def publish_runs(selection, new, dependencies):
    """The uploads, as [(new?, [crate, ...])]: one `cargo publish` per run of
    crates that share a credential (the token for new crates, the Trusted
    Publishing exchange for existing ones). Each run takes, in the selection's
    (RELEASES.toml's) order, every crate of its kind whose selected
    dependencies are already in an earlier run or in this one, so a new crate
    that needs an existing one in the release goes after it, and the reverse."""
    new, wanted, done, runs = set(new), set(selection), set(), []
    remaining = list(selection)
    while remaining:
        kind, batch = remaining[0] in new, []
        for name in remaining:
            if (name in new) == kind and dependencies.get(name, set()) & wanted <= done | set(batch):
                batch.append(name)
        if remaining[0] not in batch:
            raise ValueError(f"{remaining[0]} is selected before a crate it depends on")
        runs.append((kind, batch))
        done |= set(batch)
        remaining = [name for name in remaining if name not in done]
    return runs


def publish(selection, dry_run=False, new=(), dependencies=None, environ=None):
    """`cargo publish -p a -p b ...` for the selection. A dry run packages and
    verifies all of it in one command, each crate against the earlier ones'
    tarballs (multi-package publish, Cargo 1.90+). The upload runs once per
    `publish_runs` run, with that run's credential: RELEASE_TRUSTED_TOKEN
    (the exchange) for existing crates, RELEASE_NEW_CRATE_TOKEN for new ones.
    Cargo orders each run's crates by dependency and waits for each on the
    index before uploading what needs it."""
    if dry_run:
        subprocess.run(["cargo", "publish", *package_args(selection), "--locked", "--dry-run"], check=True)
        return
    environ = dict(os.environ if environ is None else environ)
    tokens = {False: environ.pop("RELEASE_TRUSTED_TOKEN", ""), True: environ.pop("RELEASE_NEW_CRATE_TOKEN", "")}
    environ.pop("CARGO_REGISTRY_TOKEN", None)
    runs = publish_runs(selection, new, internal_dependencies() if dependencies is None else dependencies)
    for kind, names in runs:
        if not tokens[kind]:
            raise RuntimeError(
                "No credential for " + ", ".join(names) + ": " +
                ("a new crate's first version needs CARGO_REGISTRY_TOKEN in the crates-io environment"
                 if kind else "the Trusted Publishing exchange gave no token"))
    for kind, names in runs:
        print(f"Publishing {', '.join(names)} ({'new crates, token' if kind else 'Trusted Publishing'})",
              flush=True)
        subprocess.run(["cargo", "publish", *package_args(names), "--locked"], check=True,
                       env={**environ, "CARGO_REGISTRY_TOKEN": tokens[kind]})


def package_args(names):
    return [arg for name in names for arg in ("-p", name)]


def verify(selection):
    for name, version in selection.items():
        for attempt in range(6):
            code = registry_status(name, version)
            if code == 200:
                break
            if code != 404:
                raise RuntimeError(f"Could not verify {name}@{version} (HTTP {code})")
            if attempt == 5:
                raise RuntimeError(f"{name}@{version} did not appear on crates.io after six attempts")
            print(f"Waiting for {name}@{version}, attempt {attempt + 1}", flush=True)
            time.sleep(20)
        # A fresh consumer outside the checkout cannot accidentally use a local
        # path dependency or repository Cargo configuration. '=' is essential:
        # a caret range could verify a different, already-published version.
        with tempfile.TemporaryDirectory(prefix="rs-rich-verify-") as directory:
            consumer = Path(directory)
            if name == "rs-rich-cli":
                subprocess.run(["cargo", "install", name, "--version", f"={version}",
                                "--locked", "--root", str(consumer)], cwd=consumer, check=True)
            else:
                (consumer / "Cargo.toml").write_text(
                    '[package]\nname = "release-consumer"\nversion = "0.0.0"\nedition = "2021"\n'
                    f'[dependencies]\nreleased = {{ package = "{name}", version = "={version}" }}\n',
                    encoding="utf-8",
                )
                (consumer / "src").mkdir()
                (consumer / "src/main.rs").write_text("use released as _;\nfn main() {}\n", encoding="utf-8")
                subprocess.run(["cargo", "check"], cwd=consumer, check=True)
        print(f"Verified {name}@{version} from crates.io", flush=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    tag_check = commands.add_parser("validate-tag")
    tag_check.add_argument("tag")
    plan = commands.add_parser("plan",
                               help="check a tag's selection (RELEASE_SELECTION, for a release tag) "
                                    "against the manifests and export it")
    plan.add_argument("tag")
    commands.add_parser("preflight")
    upload = commands.add_parser("publish")
    upload.add_argument("--dry-run", action="store_true")
    commands.add_parser("verify")
    args = parser.parse_args()
    if args.command == "validate-tag":
        sha = validate_tag(args.tag)
        print(sha)
        if output := os.environ.get("GITHUB_OUTPUT"):
            with open(output, "a", encoding="utf-8") as stream:
                stream.write(f"sha={sha}\n")
    elif args.command == "plan":
        metadata = json.loads(subprocess.check_output(
            ["cargo", "metadata", "--no-deps", "--format-version", "1", "--locked"], text=True, encoding="utf-8",
        ))
        root = tomllib.loads(Path("Cargo.toml").read_text(encoding="utf-8"))
        wanted = os.environ.get("RELEASE_SELECTION") or None
        releases = Path("RELEASES.toml")
        selection = select(args.tag, metadata, root, json.loads(wanted) if wanted else None,
                           releases.read_text(encoding="utf-8") if releases.exists() else None)
        encoded = json.dumps(selection, separators=(",", ":"))
        print(encoded)
        if output := os.environ.get("GITHUB_OUTPUT"):
            with open(output, "a", encoding="utf-8") as stream:
                stream.write(f"selection={encoded}\n")
    else:
        selection = json.loads(os.environ["RELEASE_SELECTION"])
        if not isinstance(selection, dict) or not selection or not set(selection) <= set(CRATES):
            raise ValueError("Invalid release selection")
        for version in selection.values():
            if not isinstance(version, str) or not re.fullmatch(VERSION, version):
                raise ValueError("Invalid release version")
        if args.command == "preflight":
            new = preflight(selection)
            if output := os.environ.get("GITHUB_OUTPUT"):
                with open(output, "a", encoding="utf-8") as stream:
                    stream.write(f"new_crate={'true' if new else 'false'}\n"
                                 f"existing_crate={'true' if len(new) < len(selection) else 'false'}\n"
                                 f"new_crates={json.dumps(new, separators=(',', ':'))}\n")
        elif args.command == "publish":
            new = [] if args.dry_run else json.loads(os.environ.get("RELEASE_NEW_CRATES") or "[]")
            if not isinstance(new, list) or not set(new) <= set(selection):
                raise ValueError("Invalid new-crate list")
            publish(selection, args.dry_run, new)
        else:
            verify(selection)


if __name__ == "__main__":
    main()
