#!/usr/bin/env python3
"""Snapshots of each published crate's public API, made with `cargo public-api`.

One file per crate, `api/<crate>.txt`, lists every public item as
`cargo public-api -s` prints it (blanket impls left out; auto traits such as
`Send` and `Sync` kept, since they are API). CI's `public API` job runs
`--check`: a change to a crate's API that the pull request does not carry in
its snapshot fails, with the diff. A contributor who changed an API on
purpose runs `--bless` and commits the snapshot, so the review sees it.

    python scripts/public_api.py --check            # every crate
    python scripts/public_api.py --bless rs-rich    # rewrite one snapshot

Both need the tools pinned in `api/tools.toml` (`--install` prints the
commands): a nightly toolchain, for rustdoc's JSON output (no stable rustdoc
writes it; only the docs are built with it), and the `cargo-public-api` that
reads that nightly's JSON format. Moving either pin can change every
snapshot's text without any API change, so a pin moves in its own commit,
with the snapshots re-blessed.

The snapshots are made on Linux. An item behind `cfg(windows)` or another
platform is not in them; one behind a Cargo feature is, for the features
FEATURES enables.
"""

import argparse
import difflib
import json
import os
from pathlib import Path
import subprocess
import sys
import tomllib

ROOT = Path(__file__).resolve().parent.parent
SNAPSHOTS = ROOT / "api"
TOOLS = tomllib.loads((SNAPSHOTS / "tools.toml").read_text(encoding="utf-8"))
NIGHTLY = TOOLS["nightly"]
TOOL_VERSION = TOOLS["cargo-public-api"]
# The features each crate's snapshot covers: all of them, except where that
# only adds code, not API (the CLI's commands and tree-sitter languages), or
# needs a C toolchain the API does not depend on.
DEFAULT_ONLY = ()
FEATURES = {
    "rs-rich-cli": DEFAULT_ONLY,
    "rs-rich-lumis": DEFAULT_ONLY,
}


def run(command, **kwargs):
    return subprocess.run(command, cwd=ROOT, check=True, **kwargs)


def crates(metadata):
    """{crate: library target name} for every publishable workspace crate."""
    members = set(metadata["workspace_members"])
    found = {}
    for package in metadata["packages"]:
        if package["id"] not in members or package.get("publish") == []:
            continue
        for target in package["targets"]:
            if {"lib", "proc-macro"} & set(target["kind"]):
                found[package["name"]] = target["name"].replace("-", "_")
    return dict(sorted(found.items()))


def features(name):
    return list(FEATURES.get(name, ("--all-features",)))


def snapshot_path(name):
    return SNAPSHOTS / f"{name}.txt"


def check_tools():
    """Refuse to run with tools other than the pinned ones: a different
    nightly or cargo-public-api prints a different text for the same API."""
    problems = []
    toolchains = subprocess.run(["rustup", "toolchain", "list"], capture_output=True, text=True)
    if NIGHTLY not in toolchains.stdout:
        problems.append(f"the toolchain {NIGHTLY} is not installed")
    version = subprocess.run(["cargo", "public-api", "--version"], capture_output=True, text=True)
    if version.returncode != 0:
        problems.append("cargo-public-api is not installed")
    elif version.stdout.split()[-1:] != [TOOL_VERSION]:
        problems.append(f"cargo-public-api is {version.stdout.strip()}, not {TOOL_VERSION}")
    if problems:
        sys.exit("public_api.py: " + "; ".join(problems) + "\nInstall the pinned tools with:\n"
                 + install_commands())


def install_commands():
    return (f"  rustup toolchain install {NIGHTLY} --profile minimal\n"
            f"  cargo install --locked cargo-public-api --version {TOOL_VERSION}\n")


def public_api(name, lib, target_dir):
    """The crate's public API as `cargo public-api -s` lists it."""
    run(["cargo", f"+{NIGHTLY}", "rustdoc", "--quiet", "--locked", "-p", name, "--lib",
         *features(name), "--target-dir", str(target_dir),
         "-Z", "unstable-options", "--output-format", "json"],
        env={**os.environ, "RUSTDOCFLAGS": "--cap-lints allow"})
    json_path = Path(target_dir) / "doc" / f"{lib}.json"
    listed = run(["cargo", "public-api", "--rustdoc-json", str(json_path), "-s", "--color", "never"],
                 capture_output=True, text=True, encoding="utf-8")
    return listed.stdout


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    mode = parser.add_mutually_exclusive_group(required=True)
    mode.add_argument("--check", action="store_true", help="fail when a snapshot differs from the API")
    mode.add_argument("--bless", action="store_true", help="rewrite the snapshots from the API")
    mode.add_argument("--install", action="store_true", help="print the commands that install the tools")
    mode.add_argument("--nightly", action="store_true", help="print the pinned nightly toolchain")
    parser.add_argument("crates", nargs="*", help="limit to these crates (default: every published crate)")
    parser.add_argument("--target-dir", help="Cargo's target directory (default: CARGO_TARGET_DIR or target)")
    args = parser.parse_args(argv)
    if args.install:
        print(install_commands(), end="")
        return 0
    if args.nightly:
        print(NIGHTLY)
        return 0
    metadata = json.loads(run(["cargo", "metadata", "--no-deps", "--format-version", "1", "--locked"],
                              capture_output=True, text=True, encoding="utf-8").stdout)
    known = crates(metadata)
    unknown = sorted(set(args.crates) - set(known))
    if unknown:
        parser.error(f"not a published library crate: {', '.join(unknown)}")
    selected = {name: known[name] for name in args.crates} if args.crates else known
    check_tools()
    target_dir = Path(args.target_dir or os.environ.get("CARGO_TARGET_DIR") or ROOT / "target") / "public-api"
    SNAPSHOTS.mkdir(exist_ok=True)
    changed = []
    for name, lib in selected.items():
        print(f"public API of {name}", file=sys.stderr, flush=True)
        current = public_api(name, lib, target_dir)
        path = snapshot_path(name)
        recorded = path.read_text(encoding="utf-8") if path.exists() else None
        if current == recorded:
            continue
        if args.bless:
            path.write_text(current, encoding="utf-8", newline="\n")
            print(f"wrote {path.relative_to(ROOT)}", file=sys.stderr)
            continue
        changed.append(name)
        sys.stdout.writelines(difflib.unified_diff(
            (recorded or "").splitlines(keepends=True), current.splitlines(keepends=True),
            f"api/{name}.txt (checked in)", f"api/{name}.txt (this tree)"))
    if not args.crates and args.bless:
        # A crate no longer published leaves no stale snapshot behind.
        for path in SNAPSHOTS.glob("*.txt"):
            if path.stem not in known:
                path.unlink()
                print(f"removed {path.relative_to(ROOT)}", file=sys.stderr)
    if not args.crates and args.check:
        stale = sorted(path.stem for path in SNAPSHOTS.glob("*.txt") if path.stem not in known)
        if stale:
            print(f"snapshots of crates that are not published: {', '.join(stale)}", file=sys.stderr)
            changed += stale
    if changed:
        print(f"\nThe public API of {', '.join(changed)} differs from its snapshot in api/.\n"
              "If the change is meant, run `python scripts/public_api.py --bless "
              + " ".join(changed) + "` and commit the snapshot, so the review sees it.", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
