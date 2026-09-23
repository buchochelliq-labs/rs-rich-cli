#!/usr/bin/env python3
"""Differential renderer harness for Rust `rich` vs pinned Python `rich`.

Cases are JSON objects. The default corpus is intentionally small and stable so
ordinary CI can replay it quickly; generated runs can use `--generate`.
"""

from __future__ import annotations

import argparse
import copy
import importlib.metadata
import json
import os
from pathlib import Path
import random
import subprocess
import sys
import tomllib

from rich import box
from rich.align import Align
from rich.console import Console
from rich.padding import Padding
from rich.panel import Panel
from rich.rule import Rule
from rich.table import Table
from rich.text import Text

ROOT = Path(__file__).resolve().parent.parent
DEFAULT_CORPUS = ROOT / "scripts" / "fixtures" / "diff_rich_cases.jsonl"
COLOR_SYSTEMS = ["truecolor", "256", "standard"]
CAPABILITY_PROFILES = [
    {"name": "utf8", "safe_box": False, "ascii_only": False},
    {"name": "safe-box", "safe_box": True, "ascii_only": False},
]
OVERFLOWS = ["fold", "crop", "ellipsis", "ignore"]
JUSTIFY = ["default", "left", "center", "right", "full"]
ALPHABET = "abc xyz[]/#:-_é界🙂\t"
# Table cells, headers and titles are parsed as markup upstream; keep the
# generated words markup-safe so a case exercises layout, not markup errors
# (the `markup` kind already covers those).
WORD_ALPHABET = "abc xyzé界🙂-_"
KINDS = ["markup", "text", "panel", "table", "rule", "padding", "align"]
BOXES = ["square", "rounded", "heavy", "double", "ascii", "minimal"]
ALIGNS = ["left", "center", "right"]


def word(rng: random.Random, low: int = 0, high: int = 12) -> str:
    return "".join(rng.choice(WORD_ALPHABET) for _ in range(rng.randint(low, high)))


def pinned_rich_version() -> str:
    with (ROOT / "UPSTREAM.toml").open("rb") as stream:
        return tomllib.load(stream)["rich"]["version"]


def verify_rich_version() -> str:
    expected = pinned_rich_version()
    installed = importlib.metadata.version("rich")
    if installed != expected:
        raise SystemExit(
            f"wrong Python rich version: installed {installed}, expected {expected}"
        )
    return expected


def load_cases(path: Path) -> list[dict]:
    cases = []
    for number, raw in enumerate(path.read_text(encoding="utf-8").splitlines(), 1):
        line = raw.strip()
        if not line or line.startswith("#"):
            continue
        case = json.loads(line)
        case.setdefault("name", f"{path.name}:{number}")
        cases.append(case)
    return cases


def generated_cases(seed: int, count: int) -> list[dict]:
    rng = random.Random(seed)
    cases = []
    for index in range(count):
        kind = rng.choice(KINDS)
        source = "".join(rng.choice(ALPHABET) for _ in range(rng.randint(1, 32)))
        case = {
            "kind": kind,
            "name": f"generated_{seed}_{index}",
            "source": source,
            "width": rng.randint(4, 40),
            "color_system": rng.choice(COLOR_SYSTEMS),
        }
        case.update(rng.choice(CAPABILITY_PROFILES))
        if kind == "markup" and rng.random() < 0.35:
            case["source"] = f"[{rng.choice(['red', 'bold blue', '#ff8800'])}]{source}[/]"
        if kind == "text":
            case["overflow"] = rng.choice(OVERFLOWS)
            case["no_wrap"] = rng.choice([False, True])
            case["justify"] = rng.choice(JUSTIFY)
            if rng.random() < 0.4:
                case["style"] = rng.choice(["red", "bold blue", "on green"])
        if kind == "panel":
            case["box"] = rng.choice(BOXES)
            if rng.random() < 0.3:
                case["title"] = rng.choice(["title", "Box Title", ""])
        if kind == "table":
            columns = rng.randint(1, 3)
            case["source"] = ""
            case["columns"] = [
                {
                    "header": word(rng, 0, 8),
                    "justify": rng.choice(ALIGNS),
                    **({"no_wrap": True} if rng.random() < 0.2 else {}),
                    **({"min_width": rng.randint(1, 6)} if rng.random() < 0.15 else {}),
                    **({"max_width": rng.randint(2, 10)} if rng.random() < 0.15 else {}),
                    **({"ratio": rng.randint(1, 3)} if rng.random() < 0.15 else {}),
                }
                for _ in range(columns)
            ]
            case["rows"] = [
                [word(rng) for _ in range(columns)] for _ in range(rng.randint(0, 3))
            ]
            case["box"] = rng.choice(BOXES)
            for flag, chance in [
                ("show_header", 0.8),
                ("show_lines", 0.3),
                ("show_edge", 0.8),
                ("pad_edge", 0.8),
                ("expand", 0.3),
            ]:
                case[flag] = rng.random() < chance
            if rng.random() < 0.3:
                case["title"] = word(rng, 1, 10)
        if kind == "rule":
            case["source"] = word(rng, 0, 16)
            case["characters"] = rng.choice(["─", "=", "-~", "━", "*"])
            case["align"] = rng.choice(ALIGNS)
        if kind in ("padding", "align"):
            case["source"] = word(rng, 1, 30)
        if kind == "padding":
            case["pad"] = [rng.randint(0, 2) for _ in range(4)]
            if rng.random() < 0.3:
                case["style"] = rng.choice(["on blue", "red", "on #102030"])
        if kind == "align":
            case["align"] = rng.choice(ALIGNS)
        cases.append(case)
    return cases


def python_render(case: dict) -> str:
    console = Console(
        force_terminal=True,
        color_system=case["color_system"],
        width=case["width"],
        highlight=False,
        safe_box=case.get("safe_box", False),
        legacy_windows=False,
        no_color=False,
    )
    with console.capture() as capture:
        if case["kind"] == "markup":
            console.print(case["source"], end="")
        elif case["kind"] == "text":
            text = Text(
                case["source"],
                style=case.get("style", ""),
                overflow=case.get("overflow"),
                no_wrap=case.get("no_wrap"),
                justify=case.get("justify"),
            )
            console.print(text, end="")
        elif case["kind"] == "panel":
            box_name = case.get("box", "rounded")
            box_set = getattr(box, box_name.upper(), box.ROUNDED)
            # Text, not str: the Rust side does not parse markup in panel bodies,
            # and markup parsing has its own `markup` kind.
            panel = Panel(Text(case["source"]), box=box_set, title=case.get("title"))
            console.print(panel, end="")
        elif case["kind"] == "table":
            table = Table(
                box=getattr(box, case.get("box", "heavy_head").upper()),
                show_header=case.get("show_header", True),
                show_lines=case.get("show_lines", False),
                show_edge=case.get("show_edge", True),
                pad_edge=case.get("pad_edge", True),
                expand=case.get("expand", False),
                title=case.get("title"),
            )
            for column in case["columns"]:
                table.add_column(
                    column["header"],
                    justify=column.get("justify", "left"),
                    no_wrap=column.get("no_wrap", False),
                    min_width=column.get("min_width"),
                    max_width=column.get("max_width"),
                    ratio=column.get("ratio"),
                )
            for row in case["rows"]:
                table.add_row(*row)
            console.print(table, end="")
        elif case["kind"] == "rule":
            rule = Rule(
                case["source"],
                characters=case.get("characters", "─"),
                align=case.get("align", "center"),
            )
            console.print(rule, end="")
        elif case["kind"] == "padding":
            padding = Padding(
                Text(case["source"]), tuple(case["pad"]), style=case.get("style", "none")
            )
            console.print(padding, end="")
        elif case["kind"] == "align":
            console.print(Align(Text(case["source"]), case["align"]), end="")
        else:
            raise ValueError(f"unknown kind {case['kind']!r}")
    out = capture.get()
    # Block renderables end every line, including the last, with a newline;
    # the Rust side renders to a string without the final one.
    if case["kind"] != "markup" and case["kind"] != "text" and out.endswith("\n"):
        out = out[:-1]
    return out


def rust_render(cases: list[dict], command: list[str]) -> list[dict]:
    payload = "\n".join(json.dumps(case, ensure_ascii=False) for case in cases) + "\n"
    env = {**os.environ, "NO_COLOR": "", "TERM": "xterm-256color"}
    proc = subprocess.run(
        command,
        cwd=ROOT,
        input=payload,
        text=True,
        encoding="utf-8",
        capture_output=True,
        env=env,
    )
    if proc.returncode != 0:
        raise SystemExit(proc.stderr)
    return [json.loads(line) for line in proc.stdout.splitlines() if line.strip()]


def python_render_all(cases: list[dict]) -> list[dict]:
    """Render every case, one interpreter per colour system.

    rich memoises a `Style`'s rendered SGR codes on the instance, whatever the
    colour system, and `Style.parse` caches instances. Rendering `#ff8800`
    under `standard` and then `truecolor` in one process prints `91` twice, so
    a shared interpreter reports mismatches that are the oracle's own.
    """
    outputs: list[dict | None] = [None] * len(cases)
    groups: dict[str, list[int]] = {}
    for index, case in enumerate(cases):
        groups.setdefault(case.get("color_system", "truecolor"), []).append(index)
    for indices in groups.values():
        payload = "\n".join(json.dumps(cases[i], ensure_ascii=False) for i in indices) + "\n"
        proc = subprocess.run(
            [sys.executable, str(Path(__file__).resolve()), "--python-worker"],
            input=payload,
            text=True,
            encoding="utf-8",
            capture_output=True,
            env={**os.environ, "PYTHONUTF8": "1"},
        )
        if proc.returncode != 0:
            raise SystemExit(proc.stderr)
        results = [json.loads(line) for line in proc.stdout.splitlines() if line.strip()]
        for index, result in zip(indices, results, strict=True):
            outputs[index] = result
    return outputs  # type: ignore[return-value]


def python_worker() -> int:
    for line in sys.stdin:
        if not line.strip():
            continue
        case = json.loads(line)
        try:
            result = {"ok": True, "output": python_render(case)}
        except Exception as error:
            result = {"ok": False, "error": f"{type(error).__name__}: {error}"}
        print(json.dumps(result, ensure_ascii=False), flush=True)
    return 0


def outcome(py: dict, rust: dict) -> str | None:
    """Classify one comparison; None means the outputs agree."""
    if not py.get("ok") and not rust.get("ok"):
        return None
    if not py.get("ok"):
        return "python_error"
    if not rust.get("ok"):
        return "rust_error"
    return None if py["output"] == rust["output"] else "output"


def mismatches(cases: list[dict], rust_command: list[str], mutate_oracle: bool) -> list[dict]:
    py_outputs = python_render_all(cases)
    if mutate_oracle and py_outputs and py_outputs[0].get("ok"):
        py_outputs[0]["output"] += "<mutation>"
    rust_outputs = rust_render(cases, rust_command)
    found = []
    for case, py, rust in zip(cases, py_outputs, rust_outputs, strict=True):
        kind = outcome(py, rust)
        if kind == "python_error":
            found.append({"case": case, "kind": kind, "python_error": py.get("error"), "rust": rust})
        elif kind == "rust_error":
            found.append({"case": case, "kind": kind, "python": py["output"], "rust_error": rust.get("error")})
        elif kind == "output":
            found.append({"case": case, "kind": kind, "python": py["output"], "rust": rust["output"]})
    return found


def candidates(case: dict):
    """Smaller variants of `case`: one character, cell, row or column fewer."""
    source = case.get("source", "")
    for index in range(len(source)):
        yield {**case, "source": source[:index] + source[index + 1 :]}
    rows = case.get("rows") or []
    for r in range(len(rows)):
        yield {**case, "rows": rows[:r] + rows[r + 1 :]}
    for r, row in enumerate(rows):
        for c, cell in enumerate(row):
            for index in range(len(cell)):
                smaller = [list(x) for x in rows]
                smaller[r][c] = cell[:index] + cell[index + 1 :]
                yield {**case, "rows": smaller}
    columns = case.get("columns") or []
    if len(columns) > 1:
        for c in range(len(columns)):
            yield {
                **case,
                "columns": columns[:c] + columns[c + 1 :],
                "rows": [row[:c] + row[c + 1 :] for row in rows],
            }
    for c, column in enumerate(columns):
        header = column.get("header", "")
        for index in range(len(header)):
            smaller = [dict(x) for x in columns]
            smaller[c]["header"] = header[:index] + header[index + 1 :]
            yield {**case, "columns": smaller}
        for option in ("no_wrap", "min_width", "max_width", "ratio"):
            if option in column:
                smaller = [dict(x) for x in columns]
                del smaller[c][option]
                yield {**case, "columns": smaller}
    for option in ("title", "style"):
        if option in case:
            yield {k: v for k, v in case.items() if k != option}


def shrink(case: dict, kind: str, rust_command: list[str], mutate_oracle: bool) -> dict:
    """Greedy delta reduction that keeps the *same* failure kind, so a layout
    mismatch cannot wander off into an unrelated markup error."""
    current = copy.deepcopy(case)
    changed = True
    while changed:
        changed = False
        for trial in candidates(current):
            found = mismatches([trial], rust_command, mutate_oracle)
            if found and found[0]["kind"] == kind:
                current, changed = trial, True
                break
    return current


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--corpus", type=Path, default=DEFAULT_CORPUS)
    parser.add_argument("--generate", type=int, default=0, metavar="N")
    parser.add_argument("--seed", type=int, default=1)
    parser.add_argument("--self-test-mutation", action="store_true")
    parser.add_argument(
        "--write-failures",
        type=Path,
        metavar="PATH",
        help="append each shrunk failing case to PATH as corpus JSONL",
    )
    parser.add_argument("--no-shrink", action="store_true", help="report failures unshrunk")
    parser.add_argument(
        "--max-shrink",
        type=int,
        default=25,
        metavar="N",
        help="shrink at most N failures (one per case kind first); the rest are reported unshrunk",
    )
    parser.add_argument("--python-worker", action="store_true", help=argparse.SUPPRESS)
    parser.add_argument(
        "--rust-command",
        nargs=argparse.REMAINDER,
        default=["cargo", "run", "-q", "-p", "rs-rich", "--example", "diff_render"],
    )
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    if args.python_worker:
        return python_worker()
    version = verify_rich_version()
    cases = load_cases(args.corpus)
    if args.generate:
        cases.extend(generated_cases(args.seed, args.generate))
    found = mismatches(cases, args.rust_command, args.self_test_mutation)
    if not found:
        print(f"ok: {len(cases)} cases matched Python rich {version}")
        return 0
    # Shrinking re-renders every candidate, so bound it: first one failure per
    # case kind (so every family gets a minimal reproducer), then the rest in
    # order up to --max-shrink.
    seen_kinds: set[str] = set()
    first, rest = [], []
    for index, item in enumerate(found):
        kind = item["case"]["kind"]
        (rest if kind in seen_kinds else first).append(index)
        seen_kinds.add(kind)
    order = first + rest
    to_shrink = set() if args.no_shrink else set(order[: max(args.max_shrink, 0)])
    for index, item in enumerate(found):
        shrunk = (
            shrink(item["case"], item["kind"], args.rust_command, args.self_test_mutation)
            if index in to_shrink
            else item["case"]
        )
        print(json.dumps({"mismatch": item, "shrunk": shrunk}, ensure_ascii=False))
        if args.write_failures:
            with args.write_failures.open("a", encoding="utf-8") as stream:
                stream.write(json.dumps(shrunk, ensure_ascii=False) + "\n")
    print(f"FAIL: {len(found)} of {len(cases)} cases differ from Python rich {version}", file=sys.stderr)
    return 1


if __name__ == "__main__":
    raise SystemExit(main())
