#!/usr/bin/env python3
"""End-to-end smoke test of every `rich` CLI command and render mode.

Creates its own fixtures in a temporary directory, runs each command once with
representative options, and checks the exit code, a key piece of output and
that any requested ``--export-svg`` file was written. Nothing touches the
network, and no terminal is needed: every case runs with redirected stdio.
Image and GIF cases are skipped when the binary was built without the ``art``
feature (detected from ``rich doctor --report json``).

    cargo build -p rs-rich-cli
    python3 scripts/smoke_cli.py                     # summary table; exit 1 on failure
    python3 scripts/smoke_cli.py --json              # machine-readable results
    python3 scripts/smoke_cli.py -k inspect -k diff  # only matching cases
    python3 scripts/smoke_cli.py --keep              # keep the fixture directory
    python3 scripts/smoke_cli.py --fixtures DIR      # only write the fixtures
    python3 scripts/smoke_cli.py --screenshots docs/media/guide

``--screenshots DIR`` regenerates every ``cli_*.svg`` used by the CLI guide
(``docs/guide/cli/``): the cases that carry a screenshot export it into DIR at a
fixed width, from the same fixtures, while still being checked.
"""

from __future__ import annotations

import argparse
import json
import os
import shutil
import struct
import subprocess
import sys
import tempfile
import time
import zlib
from dataclasses import dataclass, field
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
EXE = ".exe" if os.name == "nt" else ""
DEFAULT_BINARY = Path(os.environ.get("CARGO_TARGET_DIR", ROOT / "target")) / "debug" / f"rich{EXE}"

# Variables that change what the binary renders; a smoke run never inherits them.
SCRUBBED_ENV = (
    "NO_COLOR", "FORCE_COLOR", "COLORTERM", "PAGER", "MANPAGER", "RICH_SIXEL",
    "RICH_COLOR", "RICH_UNICODE", "RICH_HYPERLINKS", "RICH_GRAPHICS",
    "RICH_ANIMATION", "RICH_WIDTH", "RICH_HEIGHT", "RICH_ASCII_ONLY",
)


# ---------------------------------------------------------------------------
# Fixtures
# ---------------------------------------------------------------------------

NOTES_MD = """\
# Release notes

Version **1.2** makes the tool *faster* and adds `--csv`.

## Added

- Tables from CSV files
- A [changelog](https://example.com/changelog)

> Upgrading needs no configuration changes.

```python
print("hello")
```
"""

GREET_PY = '''\
def greet(name: str) -> str:
    """Return a friendly greeting."""
    return f"Hello, {name}!"


for person in ["Ada", "Grace"]:
    print(greet(person))
'''

GREET_V2_PY = '''\
def greet(name: str, excited: bool = False) -> str:
    """Return a friendly greeting."""
    mark = "!" if excited else "."
    return f"Hello, {name}{mark}"


for person in ["Ada", "Grace"]:
    print(greet(person))
'''

GREET_PATCH = """\
diff --git a/greet.py b/greet.py
--- a/greet.py
+++ b/greet.py
@@ -1,3 +1,4 @@
-def greet(name: str) -> str:
+def greet(name: str, excited: bool = False) -> str:
     \"\"\"Return a friendly greeting.\"\"\"
-    return f"Hello, {name}!"
+    mark = "!" if excited else "."
+    return f"Hello, {name}{mark}"
diff --git a/README.md b/README.md
--- a/README.md
+++ b/README.md
@@ -1,2 +1,2 @@
 # Greeter
-Says hello.
+Says hello, optionally with excitement.
"""

DATA_JSON = """\
{"name": "rs-rich", "version": "0.0.11", "tags": ["cli", "terminal"],
 "stable": false, "license": null, "downloads": {"week": 1204, "total": 88410}}
"""

TEAM_CSV = """\
name,role,commits,joined
Ada,author,120,2021-03-01
Grace,reviewer,98,2022-07-15
Linus,maintainer,311,2019-11-30
"""

EVENTS_JSONL = """\
{"timestamp": "2026-09-23T10:00:00Z", "level": "info", "message": "server started", "port": 8080}
{"timestamp": "2026-09-23T10:00:02Z", "level": "warning", "message": "slow request", "path": "/api", "ms": 870}
{"timestamp": "2026-09-23T10:00:05Z", "level": "error", "message": "database unreachable", "retry": true}
"""

DEPLOY_YAML = """\
name: web
replicas: 3
database:
  host: db.internal
  password: hunter2
servers:
  - name: alpha
    port: 8080
  - name: beta
    port: 8081
"""

DEPLOY_PROD_YAML = """\
name: web
replicas: 5
database:
  host: db.prod.internal
  password: hunter2
servers:
  - name: alpha
    port: 8080
"""

NOTEBOOK = {
    "cells": [
        {"cell_type": "markdown", "metadata": {},
         "source": ["# Analysis\n", "Adding up the *commits* column."]},
        {"cell_type": "code", "execution_count": 1, "metadata": {},
         "outputs": [{"name": "stdout", "output_type": "stream", "text": ["529\n"]}],
         "source": ["commits = [120, 98, 311]\n", "print(sum(commits))"]},
    ],
    "metadata": {"kernelspec": {"display_name": "Python 3", "language": "python", "name": "python3"},
                 "language_info": {"name": "python"}},
    "nbformat": 4,
    "nbformat_minor": 5,
}

CAPTURE_ANS = (
    "\x1b[1;31mERROR\x1b[0m disk full\n"
    "\x1b[32mok\x1b[0m \x1b]8;;https://example.com\x1b\\docs\x1b]8;;\x1b\\\n"
)

RICH_TOML = """\
version = 1

[defaults]
theme = "night"

[profiles.ci]
no_color = true
width = 60
panel = "rounded"

[themes.night]
notice = "bold cyan"
warning = "bold yellow"
"""


def bench_run(created: str, table_ns: float, markdown_ns: float) -> str:
    def measurement(name: str, ns: float) -> dict:
        return {"name": name, "samples": 50, "mean": ns, "median": ns, "stddev": ns * 0.01,
                "p95": ns * 1.02, "min": ns * 0.98, "max": ns * 1.04, "unit": "ns"}

    return json.dumps({"schema_version": 1, "created": created, "host": None,
                       "measurements": [measurement("table/80", table_ns),
                                        measurement("markdown/80", markdown_ns)]}, indent=2) + "\n"


def png(width: int, height: int, pixel) -> bytes:
    """An 8-bit RGBA PNG from ``pixel(x, y) -> (r, g, b, a)``; stdlib only."""
    raw = bytearray()
    for y in range(height):
        raw.append(0)  # filter: none
        for x in range(width):
            raw.extend(pixel(x, y))

    def chunk(kind: bytes, data: bytes) -> bytes:
        return (struct.pack(">I", len(data)) + kind + data
                + struct.pack(">I", zlib.crc32(kind + data) & 0xFFFFFFFF))

    header = struct.pack(">IIBBBBB", width, height, 8, 6, 0, 0, 0)
    return (b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", header)
            + chunk(b"IDAT", zlib.compress(bytes(raw), 9)) + chunk(b"IEND", b""))


def scene(badge: bool):
    """A 160x100 gradient with a disc; ``badge`` adds a white square to find."""
    def pixel(x: int, y: int):
        if (x - 45) ** 2 + (y - 50) ** 2 <= 28 ** 2:
            return (250, 205, 40, 255)
        if badge and 100 <= x < 140 and 30 <= y < 70:
            return (245, 245, 245, 255)
        return (20 + x * 60 // 159, 30 + y * 50 // 99, 90 - x * 40 // 159, 255)
    return pixel


def logo(x: int, y: int):
    """A disc on a transparent canvas, for ``--image-background``."""
    inside = (x - 32) ** 2 + ((y - 16) * 2) ** 2 <= 26 ** 2
    return (90, 200, 250, 255) if inside else (0, 0, 0, 0)


def gif(width: int, height: int, palette, frames, delay_cs: int) -> bytes:
    """An animated GIF; stdlib only.

    LZW with 7-bit colour indices and a clear code every 100 pixels keeps every
    code 8 bits wide, so the "compressed" stream is just the indices.
    """
    out = bytearray(b"GIF89a")
    out += struct.pack("<HHBBB", width, height, 0xF6, 0, 0)  # 128-entry global table
    for colour in list(palette) + [(0, 0, 0)] * (128 - len(palette)):
        out += bytes(colour)
    out += b"\x21\xFF\x0BNETSCAPE2.0\x03\x01\x00\x00\x00"  # loop forever
    for frame in frames:
        out += b"\x21\xF9\x04\x00" + struct.pack("<H", delay_cs) + b"\x00\x00"
        out += b"\x2C" + struct.pack("<HHHHB", 0, 0, width, height, 0) + b"\x07"
        codes = bytearray()
        for index, colour in enumerate(frame):
            if index % 100 == 0:
                codes.append(128)  # clear
            codes.append(colour)
        codes.append(129)  # end of information
        for start in range(0, len(codes), 255):
            block = codes[start:start + 255]
            out += bytes([len(block)]) + block
        out += b"\x00"
    return bytes(out + b"\x3B")


def rolling_ball() -> bytes:
    width, height = 60, 20
    frames = []
    for step in range(6):
        cx = 5 + step * 10
        frames.append([
            1 if (x - cx) ** 2 + (y - 9) ** 2 <= 20 else (2 if y >= 16 else 0)
            for y in range(height) for x in range(width)
        ])
    return gif(width, height, [(20, 24, 60), (250, 200, 40), (60, 140, 90)], frames, 10)


def write_fixtures(directory: Path) -> None:
    """Every input the cases use, written into ``directory``."""
    text = {
        "notes.md": NOTES_MD,
        "greet.py": GREET_PY,
        "greet_v2.py": GREET_V2_PY,
        "greet.patch": GREET_PATCH,
        "data.json": DATA_JSON,
        "broken.json": '{"name": "rs-rich",\n',
        "team.csv": TEAM_CSV,
        "events.jsonl": EVENTS_JSONL,
        "deploy.yaml": DEPLOY_YAML,
        "deploy-prod.yaml": DEPLOY_PROD_YAML,
        "analysis.ipynb": json.dumps(NOTEBOOK, indent=1) + "\n",
        "capture.ans": CAPTURE_ANS,
        "rich.toml": RICH_TOML,
        "bench-base.json": bench_run("2026-09-01T00:00:00Z", 1000.0, 2000.0),
        "bench-new.json": bench_run("2026-09-02T00:00:00Z", 1300.0, 1500.0),
        "docs/intro.md": "# Intro\n\nWelcome.\n",
        "docs/usage.md": "# Usage\n\nRun `rich FILE`.\n",
    }
    binary = {
        "before.png": png(160, 100, scene(False)),
        "after.png": png(160, 100, scene(True)),
        "logo.png": png(64, 32, logo),
        "ball.gif": rolling_ball(),
    }
    directory.mkdir(parents=True, exist_ok=True)
    (directory / "docs").mkdir(exist_ok=True)
    (directory / "site").mkdir(exist_ok=True)
    for name, content in text.items():
        (directory / name).write_text(content, encoding="utf-8", newline="\n")
    for name, content in binary.items():
        (directory / name).write_bytes(content)


# ---------------------------------------------------------------------------
# Cases
# ---------------------------------------------------------------------------

@dataclass
class Case:
    """One invocation. ``expect`` must appear in ``stream`` ("stdout"/"stderr")."""

    name: str
    args: list[str]
    expect: str
    code: int = 0
    stream: str = "stdout"
    stdin: str | None = None
    art: bool = False
    # A `cli_<shot>.svg` screenshot, exported at `width` columns.
    shot: str | None = None
    width: int = 80
    # Files (relative to the fixture directory) the command must create.
    creates: list[str] = field(default_factory=list)
    env: dict[str, str] = field(default_factory=dict)


CASES: list[Case] = [
    # Render modes
    # Markup is piped in for screenshots: an SVG is titled after its RESOURCE,
    # and literal markup would lend it a title cut at the last "/".
    Case("print", ["print", "[bold magenta]Hello[/] from [green]rich[/]!"], "Hello from rich"),
    Case("print-stdin", ["print", "-"], "Hello from rich",
         stdin="[bold magenta]Hello[/] from [green]rich[/]!\n", shot="print", width=40),
    Case("markdown", ["notes.md"], "Release notes", shot="markdown", width=64),
    Case("markdown-command", ["markdown", "notes.md", "--hyperlinks"], "changelog"),
    Case("syntax", ["greet.py"], "def greet", shot="syntax", width=64),
    Case("syntax-command", ["syntax", "greet.py", "--width", "50"], "print(greet"),
    Case("json", ["data.json"], '"downloads"', shot="json", width=50),
    Case("csv", ["team.csv", "--title", "Team"], "Linus", shot="csv", width=56),
    Case("csv-stdin", ["csv", "-"], "Grace", stdin=TEAM_CSV),
    Case("ipynb", ["analysis.ipynb"], "In [1]", shot="ipynb", width=64),
    # Streaming modes cannot be exported, so they have no screenshot.
    Case("jsonl", ["jsonl", "events.jsonl"], '"database unreachable"'),
    Case("log", ["log", "events.jsonl"], "server started"),
    Case("log-rich", ["log", "events.jsonl", "--log-presentation", "rich"], "slow request"),
    Case("log-stdin", ["log", "-"], "server started", stdin=EVENTS_JSONL),
    Case("rule", ["rule", "Chapter 1"], "Chapter 1", shot="rule", width=50),
    Case("format-auto-json", ["--format", "auto"], '"ok"', stdin='{"ok": true, "items": [1, 2]}\n',
         shot="format_auto", width=40),
    Case("format-auto-yaml", ["--format", "auto"], "replicas", stdin=DEPLOY_YAML),
    Case("format-auto-plain", ["--format", "auto"], "just words", stdin="just words\n"),
    # Structured data
    Case("inspect", ["inspect", "deploy.yaml"], "servers", shot="inspect", width=50),
    Case("inspect-select", ["inspect", "deploy.yaml", "--select", "$.servers[*].name"], "beta",
         shot="inspect_select", width=50),
    Case("inspect-find", ["inspect", "deploy.yaml", "--find", "alpha"], "1 match",
         shot="inspect_find", width=50),
    Case("inspect-flatten", ["inspect", "deploy.yaml", "--flatten"], "database.host",
         shot="inspect_flatten", width=50),
    Case("inspect-redact", ["inspect", "deploy.yaml", "--redact"], "********",
         shot="inspect_redact", width=50),
    Case("inspect-compare", ["inspect", "deploy.yaml", "--compare", "deploy-prod.yaml"], "replicas",
         shot="inspect_compare", width=60),
    Case("inspect-json-stdin", ["inspect", "-", "--format", "json"], "ok", stdin='{"ok": true}\n'),
    # Text diffs
    Case("diff-text", ["diff", "greet.py", "greet_v2.py"], "lines changed", shot="diff_text",
         width=80),
    Case("diff-side-by-side", ["diff", "greet.py", "greet_v2.py", "--side-by-side"], "│",
         shot="diff_side_by_side", width=110),
    Case("diff-patch", ["diff", "-"], "2 files changed", stdin=GREET_PATCH, shot="diff_patch",
         width=80),
    Case("diff-threshold", ["diff", "greet.py", "greet_v2.py", "--threshold", "10"], "FAIL",
         code=5, shot="diff_threshold", width=80),
    # Images, GIFs and image diffs (need the `art` feature)
    Case("image", ["image", "before.png", "--image-mode", "blocks", "--width", "48"], "▀",
         art=True, shot="image", width=50),
    Case("image-ascii", ["image", "before.png", "--image-mode", "ascii", "--width", "48"], "M",
         art=True, shot="image_ascii", width=50),
    Case("image-quadrants", ["image", "before.png", "--image-mode", "quadrants", "--width", "48"],
         "▀", art=True, shot="image_quadrants", width=50),
    Case("image-braille", ["image", "before.png", "--image-mode", "braille", "--width", "48"],
         "⣿", art=True),
    Case("image-fit", ["image", "before.png", "--image-mode", "blocks", "--width", "30",
                       "--height", "12", "--image-fit", "cover", "--image-anchor", "right"],
         "▀", art=True, shot="image_fit", width=32),
    Case("image-background", ["image", "logo.png", "--image-mode", "blocks", "--width", "40",
                              "--image-background", "#542080"], "▀", art=True,
         shot="image_background", width=42),
    Case("image-color", ["image", "before.png", "--image-mode", "blocks", "--width", "48",
                         "--image-color", "ansi16", "--image-dither", "bayer4x4"], "▀",
         art=True, shot="image_ansi16", width=50),
    Case("image-transform", ["image", "before.png", "--image-mode", "blocks", "--width", "24",
                             "--image-rotate", "90", "--image-grayscale",
                             "--image-contrast", "1.4"], "▀",
         art=True, shot="image_transform", width=26),
    Case("gif", ["gif", "ball.gif", "--gif-mode", "blocks", "--width", "48"], "first frame",
         stream="stderr", art=True),
    Case("diff-images", ["diff", "before.png", "after.png", "--image-mode", "blocks",
                         "--width", "64"], "changed perceptibly", art=True, shot="diff_images",
         width=70),
    Case("diff-images-gate", ["diff", "before.png", "after.png", "--image-mode", "none",
                              "--threshold", "2"], "FAIL", code=5, art=True),
    # Escape sequences
    Case("ansi-explain", ["ansi", "explain", "capture.ans"], "bold on, fg red",
         shot="ansi_explain", width=90),
    Case("ansi-inline", ["ansi", "explain", "capture.ans", "--ansi-inline"], "⟨"),
    Case("sanitize", ["--sanitize", "capture.ans", "--syntax"], "␛"),
    # Decoration, layout and export
    Case("panel", ["print", "-", "--panel", "rounded", "--title", "CI", "--caption", "main",
                   "--padding", "1,2", "--panel-style", "green"], "Build passed",
         stdin="[b]Build passed[/]\n3 targets, 0 warnings\n", shot="panel", width=44),
    Case("align", ["print", "Centered", "--center", "--width", "24", "--panel", "heavy",
                   "--style", "bold white on #303060"], "Centered", shot="align", width=60),
    Case("export-html", ["notes.md", "--export-html", "notes.html"], "Release notes",
         creates=["notes.html"]),
    Case("theme", ["--config", "rich.toml", "print", "-"], "Ready",
         stdin="[notice]Ready[/] [warning]2 warnings[/]\n", shot="theme", width=40),
    Case("theme-style", ["--config", "rich.toml", "--theme-style", "notice=bold green",
                         "print", "[notice]Ready[/]"], "Ready"),
    # Pager and watch run once when stdout is redirected
    Case("pager", ["--pager", "notes.md"], "Release notes", env={"PAGER": "cat"}),
    Case("watch-snapshot", ["--watch", "notes.md", "data.json"], '"downloads"'),
    # Batch
    Case("batch-dry-run", ["--batch", "--markdown", "--export-html", "site/page.html", "--dry-run",
                           "docs"], "Dry run: 2 file(s), 0 error(s)"),
    Case("batch", ["--report", "json", "--batch", "--markdown", "--export-html", "site/page.html",
                   "--jobs", "2", "docs"], '"completed":2', stream="stderr",
         creates=["site/intro.html", "site/usage.html"]),
    # Configuration
    Case("config-validate", ["config", "validate", "--config", "rich.toml", "--profile", "ci"],
         '"valid": true'),
    Case("config-show", ["config", "show", "--config", "rich.toml", "--profile", "ci"],
         '"panel": "rounded"'),
    Case("config-explain", ["config", "explain", "width", "--config", "rich.toml", "--profile",
                            "ci", "--width", "72"], "command line"),
    Case("config-reference", ["config", "reference"], "rich configuration"),
    Case("config-invalid", ["config", "validate", "--config", "broken.json"], "", code=2,
         stream="stderr"),
    # Generated help, completions and docs
    Case("help", ["--help"], "RESOURCE"),
    Case("version", ["--version"], "rs-rich-cli"),
    Case("completions-bash", ["completions", "bash"], "complete -F"),
    Case("completions-zsh", ["completions", "zsh"], "#compdef rich"),
    Case("completions-fish", ["completions", "fish"], "complete -c 'rich'"),
    Case("completions-powershell", ["completions", "powershell"], "Register-ArgumentCompleter"),
    Case("docs-markdown", ["docs", "markdown"], "# rich"),
    Case("docs-config", ["docs", "config"], "export_svg"),
    Case("docs-man", ["docs", "man", "--output", "man"], "rich.1", creates=["man/rich.1"]),
    # Diagnostics, benchmarks, reports and exit codes
    Case("doctor", ["doctor"], "Capability"),
    Case("doctor-json", ["doctor", "--report", "json"], '"features"'),
    Case("bench-compare", ["bench", "compare", "bench-base.json", "bench-new.json",
                           "--threshold", "10"], "regression", code=5),
    Case("report-ok", ["--report", "json", "json", "data.json"], '"ok":true', stream="stderr"),
    Case("exit-usage", ["--no-such-flag"], "no-such-flag", code=2, stream="stderr"),
    Case("exit-input", ["--report", "json", "missing.md"], '"code":"input"', code=3,
         stream="stderr"),
    Case("exit-data", ["--report", "json", "json", "broken.json"], '"code":"data"', code=4,
         stream="stderr"),
    # The guided tour
    Case("demo-list", ["--demo-list"], "workflows"),
    Case("demo-core", ["--demo", "--demo-section", "core", "--no-color"], "markdown"),
]


# ---------------------------------------------------------------------------
# Running
# ---------------------------------------------------------------------------

@dataclass
class Result:
    name: str
    command: str
    status: str  # pass, fail or skip
    exit_code: int | None
    seconds: float
    detail: str = ""


def base_env(home: Path) -> dict[str, str]:
    env = {k: v for k, v in os.environ.items() if k not in SCRUBBED_ENV}
    # No user or project config leaks in: discovery sees an empty home.
    env.update({"HOME": str(home), "USERPROFILE": str(home), "XDG_CONFIG_HOME": str(home),
                "LINES": "25", "TERM": "xterm-256color", "PYTHONUTF8": "1"})
    return env


def features(binary: Path, env: dict[str, str]) -> dict:
    """The build's Cargo features, from `rich doctor --report json`."""
    process = subprocess.run([str(binary), "doctor", "--report", "json", "--no-config"],
                             capture_output=True, text=True, encoding="utf-8", env=env, timeout=60)
    if process.returncode != 0:
        raise SystemExit(f"doctor failed ({process.returncode}): {process.stderr.strip()}")
    return json.loads(process.stdout).get("features", {})


def run_case(case: Case, binary: Path, workdir: Path, env: dict[str, str],
             svg_dir: Path | None, timeout: float) -> Result:
    args = list(case.args)
    svg = None
    if case.shot and svg_dir is not None:
        svg = svg_dir / f"cli_{case.shot}.svg"
        args += ["--export-svg", str(svg)]
    command = "rich " + " ".join(_quote(a) for a in case.args)
    case_env = dict(env, COLUMNS=str(case.width), **case.env)
    start = time.monotonic()
    try:
        process = subprocess.run([str(binary), *args], cwd=workdir, input=case.stdin,
                                 capture_output=True, text=True, encoding="utf-8",
                                 errors="replace", env=case_env, timeout=timeout,
                                 stdin=None if case.stdin is not None else subprocess.DEVNULL)
    except subprocess.TimeoutExpired:
        return Result(case.name, command, "fail", None, time.monotonic() - start,
                      f"timed out after {timeout:g}s")
    seconds = time.monotonic() - start
    problems = []
    if process.returncode != case.code:
        problems.append(f"exit {process.returncode}, expected {case.code}")
    output = process.stdout if case.stream == "stdout" else process.stderr
    if case.expect and case.expect not in output:
        problems.append(f"{case.stream} lacks {case.expect!r}")
    for relative in case.creates:
        if not (workdir / relative).is_file():
            problems.append(f"did not write {relative}")
    if svg is not None and (not svg.is_file() or not svg.read_text(encoding="utf-8").startswith("<svg")):
        problems.append(f"did not write {svg.name}")
    if problems and process.stderr.strip():
        problems.append("stderr: " + process.stderr.strip().splitlines()[-1][:200])
    return Result(case.name, command, "fail" if problems else "pass", process.returncode,
                  seconds, "; ".join(problems))


def _quote(arg: str) -> str:
    if arg and all(c.isalnum() or c in "-_./=,:#" for c in arg):
        return arg
    return "'" + arg.replace("'", "'\\''").replace("\n", "\\n") + "'"


def print_table(results: list[Result]) -> None:
    width = max(len(r.name) for r in results)
    print(f"{'case':<{width}}  {'status':<6}  {'time':>7}  command")
    print(f"{'-' * width}  ------  -------  -------")
    for r in results:
        print(f"{r.name:<{width}}  {r.status.upper():<6}  {r.seconds:>6.2f}s  {r.command[:90]}")
        if r.detail:
            print(f"{'':<{width}}  {'':<6}  {'':>7}  ↳ {r.detail}")
    counts = {s: sum(r.status == s for r in results) for s in ("pass", "fail", "skip")}
    total = sum(r.seconds for r in results)
    print(f"\n{counts['pass']} passed, {counts['fail']} failed, {counts['skip']} skipped "
          f"in {total:.1f}s")


def parse_args(argv: list[str] | None = None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__,
                                     formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--binary", type=Path, default=DEFAULT_BINARY,
                        help=f"the rich binary to test (default: {DEFAULT_BINARY})")
    parser.add_argument("--json", action="store_true", help="print machine-readable results")
    parser.add_argument("--keep", action="store_true", help="keep the fixture directory")
    parser.add_argument("-k", dest="filters", action="append", default=[], metavar="TEXT",
                        help="only run cases whose name contains TEXT (repeatable)")
    parser.add_argument("--list", action="store_true", help="list the cases and exit")
    parser.add_argument("--timeout", type=float, default=60.0, help="seconds per case (default 60)")
    parser.add_argument("--fixtures", type=Path, metavar="DIR",
                        help="write the fixtures to DIR and exit")
    parser.add_argument("--screenshots", type=Path, metavar="DIR",
                        help="also write each case's cli_*.svg screenshot to DIR")
    return parser.parse_args(argv)


def main(argv: list[str] | None = None) -> int:
    args = parse_args(argv)
    if args.fixtures:
        write_fixtures(args.fixtures)
        print(f"wrote fixtures to {args.fixtures}")
        return 0
    cases = [c for c in CASES if not args.filters or any(f in c.name for f in args.filters)]
    if args.list:
        for case in cases:
            print(f"{case.name:<24} rich {' '.join(_quote(a) for a in case.args)}")
        return 0
    binary = args.binary.resolve()
    if not binary.is_file():
        print(f"no binary at {binary}; build it with `cargo build -p rs-rich-cli` "
              "or pass --binary", file=sys.stderr)
        return 2

    workdir = Path(tempfile.mkdtemp(prefix="rich-smoke-"))
    try:
        home = workdir / "home"
        home.mkdir()
        write_fixtures(workdir)
        env = base_env(home)
        has_art = bool(features(binary, env).get("art"))
        svg_dir = args.screenshots.resolve() if args.screenshots else workdir / "svg"
        svg_dir.mkdir(parents=True, exist_ok=True)
        results = []
        for case in cases:
            if case.art and not has_art:
                results.append(Result(case.name, "rich " + " ".join(case.args), "skip", None, 0.0,
                                      "binary built without the art feature"))
                continue
            results.append(run_case(case, binary, workdir, env, svg_dir, args.timeout))
        if args.json:
            counts = {s: sum(r.status == s for r in results) for s in ("pass", "fail", "skip")}
            print(json.dumps({"binary": str(binary), "art": has_art, "summary": counts,
                              "results": [dict(r.__dict__, seconds=round(r.seconds, 3))
                                          for r in results]}, indent=2))
        else:
            print_table(results)
        if args.keep:
            print(f"fixtures kept in {workdir}", file=sys.stderr)
        return 1 if any(r.status == "fail" for r in results) else 0
    finally:
        if not args.keep:
            shutil.rmtree(workdir, ignore_errors=True)


if __name__ == "__main__":
    raise SystemExit(main())
