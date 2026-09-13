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

from rich.console import Console
from rich.text import Text

ROOT = Path(__file__).resolve().parent.parent
DEFAULT_CORPUS = ROOT / "scripts" / "fixtures" / "diff_rich_cases.jsonl"
COLOR_SYSTEMS = ["truecolor", "256", "standard"]
OVERFLOWS = ["fold", "crop", "ellipsis", "ignore"]
JUSTIFY = ["default", "left", "center", "right", "full"]
ALPHABET = "abc xyz[]/#:-_é界🙂\t"


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
        kind = rng.choice(["markup", "text"])
        source = "".join(rng.choice(ALPHABET) for _ in range(rng.randint(0, 32)))
        case = {
            "kind": kind,
            "name": f"generated_{seed}_{index}",
            "source": source,
            "width": rng.randint(1, 30),
            "color_system": rng.choice(COLOR_SYSTEMS),
        }
        if kind == "markup" and rng.random() < 0.35:
            case["source"] = f"[{rng.choice(['red', 'bold blue', '#ff8800'])}]{source}[/]"
        if kind == "text":
            case["overflow"] = rng.choice(OVERFLOWS)
            case["no_wrap"] = rng.choice([False, True])
            case["justify"] = rng.choice(JUSTIFY)
            if rng.random() < 0.4:
                case["style"] = rng.choice(["red", "bold blue", "on green"])
        cases.append(case)
    return cases


def python_render(case: dict) -> str:
    console = Console(
        force_terminal=True,
        color_system=case["color_system"],
        width=case["width"],
        highlight=False,
        safe_box=False,
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
        else:
            raise ValueError(f"unknown kind {case['kind']!r}")
    return capture.get()


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


def mismatches(cases: list[dict], rust_command: list[str], mutate_oracle: bool) -> list[dict]:
    py_outputs = []
    for case in cases:
        try:
            py_outputs.append({"ok": True, "output": python_render(case)})
        except Exception as error:
            py_outputs.append({"ok": False, "error": str(error)})
    if mutate_oracle and py_outputs and py_outputs[0].get("ok"):
        py_outputs[0]["output"] += "<mutation>"
    rust_outputs = rust_render(cases, rust_command)
    found = []
    for case, py, rust in zip(cases, py_outputs, rust_outputs, strict=True):
        if not py.get("ok"):
            found.append({"case": case, "python_error": py.get("error"), "rust": rust})
        elif not rust.get("ok"):
            found.append({"case": case, "python": py["output"], "rust_error": rust.get("error")})
        elif py["output"] != rust["output"]:
            found.append({"case": case, "python": py["output"], "rust": rust["output"]})
    return found


def shrink(case: dict, rust_command: list[str], mutate_oracle: bool) -> dict:
    current = copy.deepcopy(case)
    source = current.get("source", "")
    changed = True
    while changed and len(source) > 1:
        changed = False
        for index in range(len(source)):
            candidate = source[:index] + source[index + 1 :]
            trial = {**current, "source": candidate}
            if mismatches([trial], rust_command, mutate_oracle):
                current, source, changed = trial, candidate, True
                break
    return current


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--corpus", type=Path, default=DEFAULT_CORPUS)
    parser.add_argument("--generate", type=int, default=0, metavar="N")
    parser.add_argument("--seed", type=int, default=1)
    parser.add_argument("--self-test-mutation", action="store_true")
    parser.add_argument(
        "--rust-command",
        nargs=argparse.REMAINDER,
        default=["cargo", "run", "-q", "-p", "rs-rich", "--example", "diff_render"],
    )
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    version = verify_rich_version()
    cases = load_cases(args.corpus)
    if args.generate:
        cases.extend(generated_cases(args.seed, args.generate))
    found = mismatches(cases, args.rust_command, args.self_test_mutation)
    if not found:
        print(f"ok: {len(cases)} cases matched Python rich {version}")
        return 0
    for item in found:
        shrunk = shrink(item["case"], args.rust_command, args.self_test_mutation)
        print(json.dumps({"mismatch": item, "shrunk": shrunk}, ensure_ascii=False))
    return 1


if __name__ == "__main__":
    raise SystemExit(main())
