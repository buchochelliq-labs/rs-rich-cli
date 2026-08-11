#!/usr/bin/env python3
"""Benchmark this Rust `rich` CLI against the Python `rich-cli` it mirrors.

Usage:
    python scripts/bench_cli.py --setup-venv      # install rich-cli into .benchvenv
    python scripts/bench_cli.py                   # measure and print a table

Both binaries are spawned as subprocesses with stdout on a pipe, so neither is
charged for the terminal's own drawing and each takes its own non-TTY path.
Process spawn cost is included on both sides: it is real per-invocation cost.

Two things this harness exists to get right, both learned the hard way:

1. **The two CLIs do not share short flags.** Python's `-x` is `--lexer` (which
   takes an argument), not `--syntax`; its `--json` is capital `-J`. So every
   case carries a separate argv per implementation.

2. **Python's rich-cli prints a usage message and exits 0 for an unknown flag.**
   A run that dies early is fast for the wrong reason, and exit status will not
   tell you. Every case is therefore validated -- output must clear a per-case
   byte floor and must not look like usage text -- before it is timed. An
   earlier version of this script reported a 78-byte usage message against a
   122 KB render as "1.1x".
"""
from __future__ import annotations

import argparse
import json
import os
import statistics
import subprocess
import sys
import time
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
VENV = REPO / ".benchvenv"
FIXTURES = REPO / "target" / "bench-fixtures"

WARMUP = 3
RUNS = 15
WIDTH = "100"
MIN_BYTES = 200
# A one-line document legitimately renders to ~100 B while a usage message is
# ~78 B, so a single global floor either drops the real render or admits the
# error. Per-case floors keep both honest.
MIN_BYTES_BY_CASE = {"startup floor (tiny markdown)": 90}


def rust_binary() -> Path:
    exe = "rich.exe" if os.name == "nt" else "rich"
    path = REPO / "target" / "release" / exe
    if not path.exists():
        sys.exit(f"No release binary at {path}\nBuild it: cargo build --release -p rs-rich-cli")
    return path


def python_binary(explicit: str | None) -> Path:
    if explicit:
        return Path(explicit)
    scripts = "Scripts" if os.name == "nt" else "bin"
    exe = "rich.exe" if os.name == "nt" else "rich"
    path = VENV / scripts / exe
    if not path.exists():
        sys.exit(f"No Python rich-cli at {path}\nRun: python {Path(__file__).name} --setup-venv")
    return path


def setup_venv() -> None:
    """Install rich-cli into a dedicated venv.

    Deliberately NOT the ambient environment: rich-cli pins an older `rich`,
    and installing it alongside the pinned version in UPSTREAM.toml would move
    the library the golden fixtures are captured from.
    """
    print(f"creating {VENV}")
    subprocess.run([sys.executable, "-m", "venv", str(VENV)], check=True)
    scripts = "Scripts" if os.name == "nt" else "bin"
    py = VENV / scripts / ("python.exe" if os.name == "nt" else "python")
    subprocess.run([str(py), "-m", "pip", "install", "-q", "rich-cli"], check=True)
    ver = subprocess.run(
        [str(py), "-c",
         "from importlib.metadata import version;"
         "print(version('rich-cli'), version('rich'))"],
        capture_output=True, text=True, check=True).stdout.split()
    print(f"installed rich-cli {ver[0]} (bundling rich {ver[1]})")


def make_fixtures() -> dict[str, Path]:
    FIXTURES.mkdir(parents=True, exist_ok=True)
    tiny = FIXTURES / "tiny.md"
    tiny.write_text("# Hi\n", encoding="utf-8")

    big_json = FIXTURES / "big.json"
    if not big_json.exists():
        payload = [
            {"id": i, "name": f"item-{i}", "active": i % 3 == 0, "score": i * 1.5,
             "tags": [f"t{i % 7}", f"u{i % 11}"], "meta": {"depth": i % 5, "note": None}}
            for i in range(400)
        ]
        big_json.write_text(json.dumps(payload, indent=2), encoding="utf-8")

    return {"tiny_md": tiny, "big_json": big_json,
            "big_md": REPO / "docs" / "DIVERGENCES.md",
            "src_rs": REPO / "crates" / "rich" / "src" / "text.rs"}


def cases(f: dict[str, Path]):
    """(label, rust argv, python argv) -- short flags differ between the two."""
    return [
        ("startup floor (tiny markdown)", ["-m", str(f["tiny_md"])],  ["-m", str(f["tiny_md"])]),
        ("markdown 19 KB",                ["-m", str(f["big_md"])],   ["-m", str(f["big_md"])]),
        ("json 60 KB",                    ["-j", str(f["big_json"])], ["-J", str(f["big_json"])]),
        ("syntax highlight 46 KB .rs",    ["-x", str(f["src_rs"])],   ["--syntax", str(f["src_rs"])]),
        ("rule (no input file)",          ["--rule", "hello"],        ["--rule", "hello"]),
    ]


def env() -> dict[str, str]:
    e = dict(os.environ)
    e.pop("NO_COLOR", None)      # some shells set this globally; it would skew both
    e["PYTHONUTF8"] = "1"        # without it, non-ASCII dies on a cp1252 console
    return e


def run_once(exe: Path, args: list[str], capture: bool, e: dict[str, str]):
    argv = [str(exe), *args, "-w", WIDTH]
    sink = subprocess.PIPE if capture else subprocess.DEVNULL
    t0 = time.perf_counter()
    proc = subprocess.run(argv, stdout=sink, stderr=subprocess.PIPE, env=e)
    return (time.perf_counter() - t0) * 1000.0, proc


def validate(exe: Path, args: list[str], label: str, who: str, e) -> str | None:
    _, proc = run_once(exe, args, capture=True, e=e)
    out = proc.stdout or b""
    floor = MIN_BYTES_BY_CASE.get(label, MIN_BYTES)
    if proc.returncode != 0:
        return f"{who} exited {proc.returncode}"
    if len(out) < floor:
        return f"{who} produced {len(out)} B (floor {floor}) - not a render"
    if b"Usage:" in out[:200]:
        return f"{who} printed a usage message"
    return None


def measure(exe: Path, args: list[str], e) -> list[float]:
    for _ in range(WARMUP):
        run_once(exe, args, False, e)
    return [run_once(exe, args, False, e)[0] for _ in range(RUNS)]


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__,
                                 formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--setup-venv", action="store_true", help="install rich-cli, then exit")
    ap.add_argument("--python", help="path to the Python rich-cli executable")
    ap.add_argument("--json-out", type=Path, help="also write raw results here")
    args = ap.parse_args()

    if args.setup_venv:
        setup_venv()
        return 0

    rust, py, e = rust_binary(), python_binary(args.python), env()
    fixtures = make_fixtures()

    rows, excluded = [], []
    for label, rargs, pargs in cases(fixtures):
        bad = validate(rust, rargs, label, "rust", e) or validate(py, pargs, label, "python", e)
        if bad:
            excluded.append((label, bad))
            continue
        rs, ps = measure(rust, rargs, e), measure(py, pargs, e)
        rows.append({"case": label,
                     "rust_min": min(rs), "rust_med": statistics.median(rs),
                     "py_min": min(ps), "py_med": statistics.median(ps),
                     "speedup": statistics.median(ps) / statistics.median(rs)})

    print(f"| case | Rust (median) | Python (median) | speedup |")
    print(f"|---|---|---|---|")
    for r in rows:
        print(f"| {r['case']} | {r['rust_med']:.1f} ms | {r['py_med']:.1f} ms | {r['speedup']:.1f}x |")
    for label, why in excluded:
        print(f"| {label} | — | — | EXCLUDED: {why} |")
    print()
    print(f"{RUNS} runs after {WARMUP} warmup, width {WIDTH}, stdout piped (both non-TTY).")

    if args.json_out:
        args.json_out.write_text(json.dumps({"rows": rows, "excluded": excluded}, indent=2),
                                 encoding="utf-8")
    return 1 if excluded else 0


if __name__ == "__main__":
    raise SystemExit(main())
