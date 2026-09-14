"""Run the full release-prep validation list in a platform-safe order.

The command list is intentionally serial. On Windows, concurrent Cargo commands
that build the same binary can race on target/debug/*.exe and fail with
"Access is denied" even when the tree is healthy.
"""

from __future__ import annotations

import argparse
import os
from pathlib import Path
import subprocess
import sys


ROOT = Path(__file__).resolve().parent.parent


def cli_binary() -> str:
    return str(Path("target") / "debug" / ("rich.exe" if os.name == "nt" else "rich"))


def commands(tag: str) -> list[list[str]]:
    python = sys.executable
    return [
        ["cargo", "fmt", "--all", "--check"],
        ["cargo", "clippy", "--all-targets", "--", "-D", "warnings"],
        ["cargo", "clippy", "--all-targets", "--all-features", "--", "-D", "warnings"],
        ["cargo", "clippy", "-p", "rs-rich-cli", "--no-default-features", "--all-targets", "--", "-D", "warnings"],
        ["cargo", "test", "--all"],
        ["cargo", "test", "-p", "rs-rich-cli", "--no-default-features"],
        [python, "-m", "unittest", "discover", "-s", "scripts", "-p", "test_release.py", "-v"],
        [python, "-m", "unittest", "discover", "-s", "scripts", "-p", "test_release_readiness.py", "-v"],
        [python, "scripts/gen_versions.py", "--check"],
        ["cargo", "build", "-p", "rs-rich-cli", "--locked"],
        [python, "scripts/gen_cli_reference.py", "--binary", cli_binary(), "--check"],
        ["cargo", "check", "--workspace", "--locked"],
        [python, "scripts/release.py", "plan", tag],
    ]


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--tag",
        required=True,
        help="release tag to audit with scripts/release.py plan",
    )
    parser.add_argument(
        "--list",
        action="store_true",
        help="print the serialized command list without running it",
    )
    args = parser.parse_args()

    for step in commands(args.tag):
        print("+ " + subprocess.list2cmdline(step), flush=True)
        if not args.list:
            subprocess.run(step, cwd=ROOT, check=True)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
