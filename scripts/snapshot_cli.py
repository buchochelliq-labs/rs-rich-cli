#!/usr/bin/env python3
"""Run deterministic CLI render snapshots.

Cases are JSON objects with ``name``, ``args`` and optional ``stdin``.  The
runner always pins width, color and terminal capability environment variables,
so a snapshot never inherits the developer's terminal.  Use ``--update`` to
write snapshots; normal CI use is a check and prints a bounded unified diff.
"""

from __future__ import annotations

import argparse
import difflib
import json
import os
from pathlib import Path
import subprocess
import sys
import time

try:
    import fcntl
    import pty
    import select
    import struct
    import termios
except ImportError:
    pty = None

ROOT = Path(__file__).resolve().parent.parent
DEFAULT_CASES = ROOT / "scripts" / "fixtures" / "cli_snapshots.jsonl"
DEFAULT_SNAPSHOTS = ROOT / "scripts" / "fixtures" / "snapshots"

PROFILES = {
    "plain": {"TERM": "dumb", "NO_COLOR": "1", "RICH_ASCII_ONLY": "1"},
    "standard": {"TERM": "xterm-256color", "NO_COLOR": ""},
    "truecolor": {"TERM": "xterm-256color", "COLORTERM": "truecolor", "NO_COLOR": ""},
}


def load_cases(path: Path) -> list[dict]:
    cases = []
    for number, raw in enumerate(path.read_text(encoding="utf-8").splitlines(), 1):
        if not raw.strip() or raw.lstrip().startswith("#"):
            continue
        case = json.loads(raw)
        if not isinstance(case.get("name"), str) or not isinstance(case.get("args"), list):
            raise ValueError(f"{path}:{number}: expected string name and list args")
        cases.append(case)
    return cases


def environment(profile: str, width: int) -> dict[str, str]:
    try:
        values = PROFILES[profile]
    except KeyError as error:
        raise ValueError(f"unknown capability profile {profile!r}") from error
    env = dict(os.environ)
    env.update(values)
    env["COLUMNS"] = str(width)
    env["LINES"] = "25"
    env["PYTHONUTF8"] = "1"
    return env


def prepare_args(case_args: list[str], width: int) -> list[str]:
    args = [str(value) for value in case_args]
    if "--width" not in args and "-w" not in args:
        if "--" in args:
            terminator_index = args.index("--")
            args = args[:terminator_index] + ["--width", str(width)] + args[terminator_index:]
        else:
            args.extend(["--width", str(width)])
    return args


def run_pty(command: list[str], args: list[str], stdin_data: str, env: dict[str, str], width: int) -> tuple[int, str, str]:
    master, slave = pty.openpty()
    try:
        winsize = struct.pack("HHHH", 25, width, 0, 0)
        fcntl.ioctl(slave, termios.TIOCSWINSZ, winsize)
    except Exception:
        pass

    try:
        child = subprocess.Popen(
            [*command, *args],
            cwd=ROOT,
            stdin=subprocess.PIPE if stdin_data else subprocess.DEVNULL,
            stdout=slave,
            stderr=subprocess.PIPE,
            env=env,
        )
    finally:
        os.close(slave)

    if stdin_data and child.stdin:
        try:
            child.stdin.write(stdin_data.encode("utf-8"))
            child.stdin.flush()
        except BrokenPipeError:
            pass
        finally:
            child.stdin.close()

    stdout_bytes = bytearray()
    deadline = time.monotonic() + 15
    while time.monotonic() < deadline:
        try:
            r, _, _ = select.select([master], [], [], 0.05)
            if master in r:
                chunk = os.read(master, 65536)
                if not chunk:
                    break
                stdout_bytes.extend(chunk)
            elif child.poll() is not None:
                while True:
                    r, _, _ = select.select([master], [], [], 0.02)
                    if master in r:
                        chunk = os.read(master, 65536)
                        if not chunk:
                            break
                        stdout_bytes.extend(chunk)
                    else:
                        break
                break
        except OSError as e:
            if getattr(e, "errno", None) == 5:  # EIO on Linux
                break
            break

    os.close(master)
    _, stderr = child.communicate(timeout=5)
    return child.returncode, stdout_bytes.decode("utf-8", errors="replace"), stderr.decode("utf-8", errors="replace")


def render(command: list[str], case: dict, width: int, profile: str) -> str:
    args = prepare_args(case["args"], width)
    env = environment(profile, width)
    stdin_data = case.get("stdin", "")

    if profile != "plain" and pty is not None:
        returncode, stdout, stderr = run_pty(command, args, stdin_data, env, width)
    else:
        process = subprocess.run(
            [*command, *args],
            cwd=ROOT,
            input=stdin_data,
            text=True,
            encoding="utf-8",
            capture_output=True,
            env=env,
        )
        returncode = process.returncode
        stdout = process.stdout
        stderr = process.stderr

    record = {
        "status": returncode,
        "stdout": stdout.replace("\r\n", "\n"),
        "stderr": stderr.replace("\r\n", "\n"),
    }
    return json.dumps(record, ensure_ascii=False, indent=2) + "\n"


def diff(expected: str, actual: str, name: str) -> str:
    return "".join(
        difflib.unified_diff(
            expected.splitlines(keepends=True),
            actual.splitlines(keepends=True),
            fromfile=f"{name}.expected",
            tofile=f"{name}.actual",
            n=3,
        )
    )


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--cases", type=Path, default=DEFAULT_CASES)
    parser.add_argument("--snapshots", type=Path, default=DEFAULT_SNAPSHOTS)
    parser.add_argument("--width", type=int, default=80)
    parser.add_argument("--profile", choices=sorted(PROFILES), default="plain")
    parser.add_argument("--update", action="store_true")
    parser.add_argument("--command", nargs="+", default=["cargo", "run", "-q", "-p", "rs-rich-cli", "--"])
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    if args.width < 1:
        raise SystemExit("--width must be at least 1")
    cases = load_cases(args.cases)
    args.snapshots.mkdir(parents=True, exist_ok=True)
    failures = 0
    for case in cases:
        path = args.snapshots / f"{case['name']}.json"
        actual = render(args.command, case, args.width, args.profile)
        if args.update:
            path.write_text(actual, encoding="utf-8")
            continue
        if not path.exists():
            print(f"missing snapshot: {path}", file=sys.stderr)
            failures += 1
            continue
        output = diff(path.read_text(encoding="utf-8"), actual, case["name"])
        if output:
            print(output, end="")
            failures += 1
    return 1 if failures else 0


if __name__ == "__main__":
    raise SystemExit(main())
