"""Select, publish, and verify coordinated or independently versioned releases.

The workflow carries one validated selection between jobs. No phase infers a
different crate or version from the tag. Requires Python 3.11+ and Cargo.
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


CRATES = ("rs-rich", "rs-rich-ext", "rs-rich-cli", "rs-rich-art")
NUMBER = r"(?:0|[1-9][0-9]*)"
PRERELEASE = rf"(?:{NUMBER}|[0-9]*[A-Za-z-][0-9A-Za-z-]*)"
VERSION = rf"{NUMBER}\.{NUMBER}\.{NUMBER}(?:-{PRERELEASE}(?:\.{PRERELEASE})*)?(?:\+[0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*)?"
TAG = re.compile(rf"(?:({'|'.join(CRATES)})-)?v({VERSION})")


def validate_tag(tag):
    if not TAG.fullmatch(tag):
        raise ValueError(f"Invalid release tag: {tag!r}")
    ref = f"refs/tags/{tag}"
    kind = subprocess.check_output(["git", "cat-file", "-t", ref], text=True).strip()
    if kind != "tag":
        raise ValueError(f"Release tag {tag!r} must be annotated, not lightweight")
    sha = subprocess.check_output(["git", "rev-parse", "--verify", f"{ref}^{{commit}}"],
                                  text=True).strip()
    head = subprocess.check_output(["git", "rev-parse", "HEAD"], text=True).strip()
    if sha != head:
        raise ValueError("Release tag must point at the checked-out commit")
    subprocess.run(["git", "merge-base", "--is-ancestor", sha, "origin/main"], check=True)
    return sha


def select(tag, metadata, root):
    match = TAG.fullmatch(tag)
    if not match:
        raise ValueError(f"Invalid release tag: {tag!r}; use vX.Y.Z or <crate>-vX.Y.Z")
    crate, version = match.groups()
    members = set(metadata["workspace_members"])
    packages = {
        p["name"]: p for p in metadata["packages"]
        if p["id"] in members and p.get("publish") != []
    }
    if packages.keys() != set(CRATES):
        raise ValueError(f"Publishable workspace must contain exactly {CRATES}; got {sorted(packages)}")
    selected = (crate,) if crate else CRATES
    errors = []
    for name in selected:
        package = packages[name]
        if package["version"] != version:
            errors.append(f'{name}: manifest says {package["version"]}, tag says {version}')
        if package.get("publish") is not None and "crates-io" not in package["publish"]:
            errors.append(f"{name}: publishing to crates.io is disabled")
    # Keep the repository's exact internal-requirement policy, but compare each
    # requirement with its OWN crate, not the version of the release tag.
    requirements = root["workspace"]["dependencies"]
    for name in ("rs-rich", "rs-rich-ext", "rs-rich-art"):
        key = name.removeprefix("rs-")
        dependency = requirements.get(key, {})
        expected = packages[name]["version"]
        if dependency.get("package") != name or dependency.get("version") != expected:
            errors.append(f"workspace dependency {key}: expected package {name}, version {expected}")
    if errors:
        raise ValueError("\n".join(errors))
    return {name: packages[name]["version"] for name in selected}


def registry_status(name, version):
    request = Request(f"https://crates.io/api/v1/crates/{name}/{version}",
                      headers={"User-Agent": "rs-rich-release"})
    try:
        with urlopen(request, timeout=30) as response:
            return response.status
    except HTTPError as error:
        return error.code


def preflight(selection):
    for name, version in selection.items():
        code = registry_status(name, version)
        if code == 200:
            raise RuntimeError(f"{name}@{version} already exists; refusing release")
        if code != 404:
            raise RuntimeError(f"Could not verify availability of {name}@{version} (HTTP {code})")
        print(f"{name}@{version} is available", flush=True)


def publish(selection, dry_run=False):
    # Preserve Cargo's topological ordering and sibling-tarball verification for
    # the legacy workspace path; a crate tag never uploads unchanged siblings.
    args = ["--workspace"] if set(selection) == set(CRATES) else ["-p", next(iter(selection))]
    subprocess.run(["cargo", "publish", *args, "--locked",
                    *(["--dry-run"] if dry_run else [])], check=True)


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
    plan = commands.add_parser("plan")
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
            ["cargo", "metadata", "--no-deps", "--format-version", "1", "--locked"], text=True,
        ))
        root = tomllib.loads(Path("Cargo.toml").read_text(encoding="utf-8"))
        selection = select(args.tag, metadata, root)
        encoded = json.dumps(selection, separators=(",", ":"))
        print(encoded)
        if output := os.environ.get("GITHUB_OUTPUT"):
            with open(output, "a", encoding="utf-8") as stream:
                stream.write(f"selection={encoded}\n")
    else:
        selection = json.loads(os.environ["RELEASE_SELECTION"])
        if not isinstance(selection, dict) or not selection or not set(selection) <= set(CRATES):
            raise ValueError("Invalid release selection")
        if len(selection) != 1 and set(selection) != set(CRATES):
            raise ValueError("Select one crate or the entire workspace")
        for version in selection.values():
            if not isinstance(version, str) or not re.fullmatch(VERSION, version):
                raise ValueError("Invalid release version")
        if args.command == "preflight":
            preflight(selection)
        elif args.command == "publish":
            publish(selection, args.dry_run)
        else:
            verify(selection)


if __name__ == "__main__":
    main()
