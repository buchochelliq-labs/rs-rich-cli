#!/usr/bin/env python3
"""cargo-semver-checks for each published crate, against its last release.

Each crate's library is compared with the newest version of it on crates.io.

**Cargo's rule** (what `scripts/release.py plan --semver-checks` enforces):
the version bump must be at least as big as the change. From 0.1.0 a
breaking change needs a new minor (0.y) or major (x.0) version, and an
addition a new minor version from 1.0.0. In 0.0.x every bump is a breaking
one: Cargo treats 0.0.1 and 0.0.2 as incompatible, and so does
cargo-semver-checks. A 0.0.x crate whose version moved past its release can
change anything, so the rule is met without building it.

**This repository's rule for 0.0.x** (what CI's `public API and SemVer` job
adds): a 0.0.x crate is checked as if its bump were a minor one, so every
breaking change shows. Each must be listed in `api/semver-breaks.toml`
under the version that ships it, with the reason; an unlisted one fails.
Entries for a version already on crates.io are spent, and reported.

    python scripts/semver_check.py                  # every published crate
    python scripts/semver_check.py rs-rich-ext      # one crate
    python scripts/semver_check.py --gate rs-rich   # Cargo's rule only

Needs `cargo-semver-checks` and the toolchain pinned in `api/tools.toml`, and
network access to crates.io.
"""

import argparse
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import tomllib
from urllib.error import HTTPError
from urllib.request import Request, urlopen

ROOT = Path(__file__).resolve().parent.parent
TOOLS = tomllib.loads((ROOT / "api/tools.toml").read_text(encoding="utf-8"))
BREAKS = ROOT / "api/semver-breaks.toml"
# The features each crate is checked with: the public API snapshots' own
# (scripts/public_api.py).
DEFAULT_ONLY = ("rs-rich-cli", "rs-rich-lumis")
FAILURE = re.compile(r"^--- failure ([a-z0-9_]+): ")
LOCATION = re.compile(r"(?:, previously in file | in )\S+:\d+$")


def version_tuple(version):
    core = version.split("+")[0].split("-")[0]
    return tuple(int(part) for part in core.split("."))


def bump_kind(baseline, current):
    """What Cargo makes of the move from baseline to current: "major" when
    they are incompatible (0.0.x to anything else, 0.y to 0.z, x to y),
    "minor" or "patch" otherwise, "none" when they are equal."""
    old, new = version_tuple(baseline), version_tuple(current)
    if old == new:
        return "none"
    if old[0] != new[0] or (old[0] == 0 and (old[1] != new[1] or old[1] == 0)):
        return "major"
    return "minor" if old[1] != new[1] or old[0] == 0 else "patch"


def published_version(name):
    """The newest version of `name` on crates.io that is not yanked, or None."""
    request = Request(f"https://crates.io/api/v1/crates/{name}", headers={"User-Agent": "rs-rich-release-check"})
    try:
        with urlopen(request, timeout=30) as response:
            data = json.load(response)
    except HTTPError as error:
        if error.code == 404:
            return None
        raise
    versions = [v["num"] for v in data.get("versions", []) if not v.get("yanked")]
    return max(versions, key=version_tuple) if versions else None


def library_crates():
    """{crate: version} for every publishable workspace crate with a library
    (cargo-semver-checks reads no proc-macro crate)."""
    metadata = json.loads(subprocess.check_output(
        ["cargo", "metadata", "--no-deps", "--format-version", "1", "--locked"], cwd=ROOT, text=True,
        encoding="utf-8"))
    members = set(metadata["workspace_members"])
    return {p["name"]: p["version"] for p in metadata["packages"]
            if p["id"] in members and p.get("publish") != []
            and any("lib" in t["kind"] for t in p["targets"])}


def semver_checks(name, baseline, strict, run=subprocess.run):
    """Run cargo-semver-checks; return (failures, output). failures maps each
    failing lint to the items it names, without their file locations. With
    `strict`, the release is taken as a minor one, so breaking changes fail
    whatever the versions say."""
    command = ["cargo", f"+{TOOLS['semver-checks-toolchain']}", "semver-checks", "check-release", "-p", name,
               "--baseline-version", baseline, "--color", "never",
               "--default-features" if name in DEFAULT_ONLY else "--all-features"]
    if strict:
        command += ["--release-type", "minor"]
    result = run(command, cwd=ROOT, capture_output=True, text=True, encoding="utf-8")
    # The report is on standard output; progress goes to standard error.
    output = (result.stdout or "") + (result.stderr or "")
    failures = parse_failures(result.stdout or "")
    if result.returncode != 0 and not failures:
        raise RuntimeError(f"cargo-semver-checks failed on {name}:\n{output}")
    return failures, output


def parse_failures(output):
    failures, lint, in_items = {}, None, False
    for line in output.splitlines():
        if match := FAILURE.match(line):
            lint, in_items = match[1], False
            failures.setdefault(lint, [])
        elif lint and line.strip() == "Failed in:":
            in_items = True
        elif in_items and line.startswith("  ") and line.strip():
            item = LOCATION.sub("", line.strip())
            if item not in failures[lint]:
                failures[lint].append(item)
        else:
            in_items = False
    return failures


def known_breaks(text):
    """api/semver-breaks.toml as {(crate, version): {lint: set(items)}}."""
    accepted = {}
    for entry in tomllib.loads(text).get("break", []):
        key = (entry["crate"], entry["version"])
        accepted.setdefault(key, {}).setdefault(entry["lint"], set()).update(entry["items"])
    return accepted


def unexplained(failures, accepted):
    """The failing (lint, item) pairs api/semver-breaks.toml does not list."""
    return [(lint, item) for lint, items in sorted(failures.items()) for item in items
            if item not in accepted.get(lint, set())]


def gate(selection, published=published_version, run=subprocess.run, out=sys.stdout):
    """Cargo's rule for {crate: version}: refuse a bump smaller than the
    change. Returns the refusals as messages."""
    refusals = []
    for name, version in selection.items():
        baseline = published(name)
        if baseline is None:
            print(f"{name} {version}: not on crates.io yet, nothing to compare", file=out)
            continue
        kind = bump_kind(baseline, version)
        if version_tuple(version) < version_tuple(baseline):
            refusals.append(f"{name}: {version} is older than {baseline} on crates.io")
        elif kind == "major":
            print(f"{name} {baseline} -> {version}: a breaking bump under Cargo's rules; any change fits", file=out)
        else:
            failures, _ = semver_checks(name, baseline, strict=False, run=run)
            if failures:
                refusals.append(f"{name} {baseline} -> {version} ({kind}) is too small for: "
                                + ", ".join(sorted(failures)))
            else:
                print(f"{name} {baseline} -> {version} ({kind}): fits the change", file=out)
    return refusals


def check(crates, accepted, published=published_version, run=subprocess.run, out=sys.stdout):
    """CI's check: Cargo's rule for every crate, and for a 0.0.x crate every
    breaking change listed in api/semver-breaks.toml. Returns the problems."""
    problems, seen = [], {}

    def newest(name):
        if name not in seen:
            seen[name] = published(name)
        return seen[name]

    for name, version in crates.items():
        baseline = newest(name)
        if baseline is None:
            print(f"{name}: not on crates.io yet", file=out)
            continue
        if version_tuple(version)[:2] != (0, 0):
            problems += gate({name: version}, published=lambda _: baseline, run=run, out=out)
            continue
        failures, output = semver_checks(name, baseline, strict=True, run=run)
        listed = accepted.get((name, version), {})
        missing = unexplained(failures, listed)
        if missing:
            print(output, file=out)
            problems.append(f"{name} {baseline} -> {version}: breaking changes api/semver-breaks.toml does not "
                            "list:\n" + "\n".join(f"  {lint}: {item}" for lint, item in missing))
        elif failures:
            print(f"{name} {baseline} -> {version}: {sum(map(len, failures.values()))} known breaking "
                  f"changes, all listed", file=out)
        else:
            print(f"{name} {baseline} -> {version}: no breaking change", file=out)
    for (name, version) in sorted(accepted):
        baseline = newest(name) if name in crates else None
        if baseline and version_tuple(version) <= version_tuple(baseline):
            print(f"note: api/semver-breaks.toml's entries for {name} {version} are spent "
                  f"({baseline} is on crates.io); remove them", file=out)
    return problems


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("crates", nargs="*", help="limit to these crates (default: every published library)")
    parser.add_argument("--gate", action="store_true", help="Cargo's rule only (what release.py plan runs)")
    args = parser.parse_args(argv)
    os.environ.setdefault("CARGO_TERM_COLOR", "never")
    crates = library_crates()
    unknown = sorted(set(args.crates) - set(crates))
    if unknown:
        parser.error(f"not a published library crate: {', '.join(unknown)}")
    selected = {name: crates[name] for name in args.crates} if args.crates else crates
    if args.gate:
        problems = gate(selected)
    else:
        problems = check(selected, known_breaks(BREAKS.read_text(encoding="utf-8")))
    for problem in problems:
        print(f"error: {problem}", file=sys.stderr)
    return 1 if problems else 0


if __name__ == "__main__":
    sys.exit(main())
