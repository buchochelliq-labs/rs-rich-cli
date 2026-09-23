"""Regenerate the guide's screenshots from its example programs.

Every `crates/*/examples/guide_*.rs` starts with a line like

    //! Guide: Tables — run: cargo run -p rs-rich --example guide_tables [-- --svg docs/media/guide]

This script reads the package and features from that line, runs each example
with `--svg`, and so rewrites `docs/media/guide/guide_*.svg` from real output.
With `--check` it renders into a temporary directory and fails when any
checked-in screenshot differs or is missing, which is how a stale image is
caught. The CLI screenshots come from `scripts/smoke_cli.py --screenshots`.
"""

import argparse
import filecmp
import pathlib
import re
import subprocess
import sys
import tempfile

ROOT = pathlib.Path(__file__).resolve().parent.parent
MEDIA = ROOT / "docs" / "media" / "guide"
RUN = re.compile(r"cargo run (?P<args>[^\[]*)")


def examples():
    """(package, example, features) for every guide example, in path order."""
    found = []
    for path in sorted(ROOT.glob("crates/*/examples/guide_*.rs")):
        first = path.read_text(encoding="utf-8").splitlines()[0]
        run = RUN.search(first)
        package = run and re.search(r"-p (\S+)", run["args"])
        example = run and re.search(r"--example (\S+)", run["args"])
        if not (package and example) or example[1] != path.stem:
            raise SystemExit(f"{path.relative_to(ROOT)}: first line must name its run command")
        features = re.search(r"--features[ =]([\w,-]+)", run["args"])
        found.append((package[1], path.stem, features[1] if features else None))
    return found


def capture(out, only=None):
    out.mkdir(parents=True, exist_ok=True)
    failed = []
    for package, example, features in examples():
        if only and example not in only:
            continue
        command = ["cargo", "run", "-q", "-p", package, "--example", example]
        if features:
            command += ["--features", features]
        command += ["--", "--svg", str(out)]
        print("  " + " ".join(command[2:]))
        if subprocess.run(command, cwd=ROOT).returncode != 0:
            failed.append(example)
    return failed


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--check", action="store_true",
                        help="fail if a checked-in screenshot is stale or missing")
    parser.add_argument("--list", action="store_true", help="list the examples and exit")
    parser.add_argument("only", nargs="*", help="limit to these example names")
    args = parser.parse_args()

    if args.list:
        for package, example, features in examples():
            print(f"{example:32} {package:12} {features or ''}")
        return 0
    if not args.check:
        failed = capture(MEDIA, set(args.only))
        if failed:
            print("failed: " + ", ".join(failed), file=sys.stderr)
            return 1
        return 0

    with tempfile.TemporaryDirectory() as tmp:
        fresh = pathlib.Path(tmp)
        failed = capture(fresh, set(args.only))
        stale = [p.name for p in sorted(fresh.glob("*.svg"))
                 if not (MEDIA / p.name).exists()
                 or not filecmp.cmp(p, MEDIA / p.name, shallow=False)]
        if failed or stale:
            for name in failed:
                print(f"example failed: {name}", file=sys.stderr)
            for name in stale:
                print(f"stale screenshot: docs/media/guide/{name}", file=sys.stderr)
            print("Run python3 scripts/capture_guide.py", file=sys.stderr)
            return 1
    print("Guide screenshots match their examples")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
