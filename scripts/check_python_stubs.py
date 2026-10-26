#!/usr/bin/env python3
"""Check rs_rich's type stubs against the installed wheel.

Runs mypy's `stubtest` on every `rs_rich` module: each class, function,
parameter and default the stubs (`_native.pyi`, and the Python modules,
which are their own stubs) declare must be what the compiled package has at
runtime, and everything public the package has must be in the stubs. Then
runs `mypy --strict` on `crates/rich-py/tests/typing_usage.py`, which uses
the main APIs as a program would, so a stub that is present but wrong for
callers fails too, and runs that program, so what type-checks also works.

    python scripts/check_python_stubs.py     # rs_rich must be installed

Three kinds of difference are accepted, not reported:

- **A runtime default shown as `...`.** PyO3 writes a parameter's default
  into the signature only when it is a literal; one taken from a Rust
  constant reads `...`, so the stub's value cannot be compared.
- **A runtime default of `None` where the stub has Rich's default.** The
  bindings take `None` for "Rich's default" (`style: StyleType = "none"`
  in Rich is `style=None` in Rust); the stub keeps Rich's signature, which
  is what a program written for Rich type-checks against.
- **What `crates/rich-py/stubtest-allowlist.txt` names**, each with its
  reason: names that exist only for type checkers (aliases and protocols)
  and the few signatures PyO3 cannot express.

PEP 800's `@disjoint_base` is not checked: every PyO3 class is one, and
type checkers do not use it yet.
"""

import os
from pathlib import Path
import re
import subprocess
import sys

ROOT = Path(__file__).resolve().parent.parent
ALLOWLIST = ROOT / "crates/rich-py/stubtest-allowlist.txt"
USAGE = ROOT / "crates/rich-py/tests/typing_usage.py"

ACCEPTED = [
    re.compile(r'runtime parameter "\w+" has a default value of Ellipsis, '),
    re.compile(r'runtime parameter "\w+" has a default value of None, which is different from stub parameter default '),
    re.compile(r'runtime parameter "\w+" has a default value of type None, which is incompatible with stub parameter type '),
]


def stubtest_errors(output):
    """stubtest's output as [(header, block)], one per reported problem."""
    blocks, current = [], None
    for line in output.splitlines():
        if line.startswith("error: "):
            current = [line]
            blocks.append(current)
        elif current is not None and not line.startswith("Found ") and not line.startswith("Success"):
            current.append(line)
    return [(block[0], "\n".join(block)) for block in blocks]


def accepted(header):
    return any(pattern.search(header) for pattern in ACCEPTED)


def main():
    env = {**os.environ, "PYTHONDONTWRITEBYTECODE": "1"}
    stubtest = subprocess.run(
        [sys.executable, "-m", "mypy.stubtest", "rs_rich", "--ignore-disjoint-bases",
         "--allowlist", str(ALLOWLIST)],
        capture_output=True, text=True, cwd=ROOT / "crates/rich-py", env=env,
    )
    output = stubtest.stdout + stubtest.stderr
    errors = stubtest_errors(output)
    if stubtest.returncode != 0 and not errors:
        print(output)
        print("stubtest failed without reporting a stub error", file=sys.stderr)
        return 1
    reported = [block for header, block in errors if not accepted(header)]
    for block in reported:
        print(block, end="\n\n")
    skipped = len(errors) - len(reported)
    print(f"stubtest: {len(reported)} errors, {skipped} accepted differences in defaults", file=sys.stderr)

    mypy = subprocess.run(
        [sys.executable, "-m", "mypy", "--strict", "--no-incremental", str(USAGE)],
        capture_output=True, text=True, cwd=ROOT / "crates/rich-py", env=env,
    )
    print(mypy.stdout.strip() or mypy.stderr.strip(), file=sys.stderr)
    # What type-checks must also run.
    usage = subprocess.run([sys.executable, str(USAGE)], capture_output=True, text=True, cwd=ROOT, env=env)
    if usage.returncode != 0:
        print(usage.stdout + usage.stderr, file=sys.stderr)
    return 1 if reported or mypy.returncode != 0 or usage.returncode != 0 else 0


if __name__ == "__main__":
    sys.exit(main())
