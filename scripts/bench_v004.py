#!/usr/bin/env python3
"""Repeatable Linux CLI performance corpus; no network or Python oracle needed.

Build with cargo build --release -p rs-rich-cli --locked. Pass the binary and
its source revision explicitly. Requires cc. Measurements include process and
native sampler startup;
they are not in-process microbenchmarks. Output hashes detect rendering drift.
"""

import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import signal
import statistics
import subprocess
import tempfile
import time


def digest(data):
    return hashlib.sha256(data).hexdigest()


def fixtures(root):
    cases = []

    def add(name, data, suffix, flags=(), stdin=False):
        path = root / (name + suffix)
        path.write_bytes(data)
        cases.append((name, path, list(flags), stdin))

    add("startup", b"# Hello\n", ".md", ["--markdown"])
    source = b'fn sample(value: usize) -> String { format!("value: {}", value + 1) }\n'
    for size in (46_000, 199_000):
        add(f"syntax-{size}", source * (size // len(source)), ".rs", ["--syntax"])
    # Real-source controls expose cache overhead on varied, nonrepeating input.
    # The checkout supplies fixtures; compare binaries from the same checkout.
    repository = Path(__file__).resolve().parents[1]
    for name, source_path in (("cli", "crates/rich-cli/src/main.rs"),
                              ("text", "crates/rich/src/text.rs")):
        add(f"syntax-real-{name}", (repository / source_path).read_bytes(),
            ".rs", ["--syntax"])
    for rows in (10_000, 50_000, 100_000):
        data = b"id,name,note\n" + b"".join(
            f'{i},item-{i},"note, {i % 17}"\n'.encode() for i in range(rows))
        add(f"csv-{rows}", data, ".csv", ["--csv"])
        if rows == 10_000:
            add("csv-stdin-10000", data, ".csv", ["--csv"], stdin=True)
            add("csv-panel-10000", data, ".csv", ["--csv", "--panel", "rounded"])
    for size in (65_536, 262_144, 1_048_576, 5_242_880):
        add(f"wrap-ascii-{size}", b"x" * size, ".txt")
    add("wrap-words-5m", b"hello world " * (5_242_880 // 12), ".txt")
    add("wrap-markdown-1m", b"hello world " * (1_048_576 // 12), ".md", ["--markdown"])
    add("wrap-markdown-5m", b"hello world " * (5_242_880 // 12), ".md", ["--markdown"])
    add("wrap-unicode", ("🙂e\u0301漢字" * 4096).encode(), ".txt")
    return cases


def execute(command, env, timeout, stdout, stdin):
    started = time.perf_counter()
    with subprocess.Popen(command, env=env, stdin=stdin, stdout=stdout,
                          stderr=subprocess.PIPE, start_new_session=True) as child:
        try:
            _, error = child.communicate(timeout=timeout)
        except subprocess.TimeoutExpired:
            try:
                os.killpg(child.pid, signal.SIGKILL)
            except ProcessLookupError:
                pass
            child.communicate()
            return {"timeout_s": timeout}
        elapsed = time.perf_counter() - started
        if child.returncode:
            raise RuntimeError(f"exit {child.returncode}: {error.decode(errors='replace')}")
    return {"wall_ms": elapsed * 1000}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", required=True, type=Path)
    parser.add_argument("--revision", required=True)
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--runs", type=int, default=5)
    parser.add_argument("--timeout", type=float, default=10)
    parser.add_argument("--case", action="append", help="Select exact case names")
    args = parser.parse_args()
    if args.runs < 1 or args.timeout <= 0:
        parser.error("runs and timeout must be positive")
    binary = args.binary.resolve(strict=True)
    if platform.system() != "Linux" or not hasattr(os, "wait4"):
        parser.error("this RSS harness requires Linux wait4")
    env = dict(os.environ, TERM="xterm-256color", COLUMNS="100", LINES="36")
    env.pop("NO_COLOR", None)
    results = {"revision": args.revision, "binary_sha256": digest(binary.read_bytes()),
               "platform": platform.platform(), "cpu": next((line.split(":", 1)[1].strip()
                   for line in Path("/proc/cpuinfo").read_text().splitlines()
                   if line.startswith("model name")), platform.processor() or "unknown"),
               "timeout_s": args.timeout,
               "runs": args.runs, "warmup_runs": 1, "width": 100,
               "color": "--no-color", "stdout": "redirected", "cases": []}
    args.output.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="rs-rich-bench-") as directory:
        root = Path(directory)
        sampler = root / "bench-exec"
        subprocess.run(["cc", "-O2", "-Wall", "-Wextra", "-Werror", "-o", str(sampler),
                        str(Path(__file__).with_name("bench_exec.c"))], check=True)
        corpus = fixtures(root)
        if args.case and set(args.case) - {case[0] for case in corpus}:
            parser.error("unknown case name")
        for name, path, flags, use_stdin in corpus:
            if args.case and name not in args.case:
                continue
            command = [str(binary), *flags, "-" if use_stdin else str(path),
                       "--width", "100", "--no-color"]
            entry = {"name": name, "input_bytes": path.stat().st_size,
                     "input_sha256": digest(path.read_bytes()), "flags": flags,
                     "stdin": use_stdin, "samples": []}
            print(f"{name}: validating and warming up", flush=True)
            output = root / "stdout"
            with path.open("rb") as source, output.open("wb") as sink:
                warmup = execute(command, env, args.timeout, sink,
                                 source if use_stdin else subprocess.DEVNULL)
            if "timeout_s" in warmup:
                entry.update(warmup)
            else:
                data = output.read_bytes()
                if not data or b"Usage:" in data[:200]:
                    raise RuntimeError(f"{name}: output is empty or usage, not a render")
                entry.update(output_bytes=len(data), output_sha256=digest(data))
                rss_file = root / "rss"
                for _ in range(args.runs):
                    with path.open("rb") as source:
                        sample = execute([str(sampler), str(rss_file), *command], env,
                                         args.timeout, subprocess.DEVNULL,
                                         source if use_stdin else subprocess.DEVNULL)
                    if "timeout_s" in sample:
                        entry.update(sample)
                        break
                    sample["peak_rss_kib"] = int(rss_file.read_text().strip())
                    entry["samples"].append(sample)
                if len(entry["samples"]) == args.runs:
                    times = [sample["wall_ms"] for sample in entry["samples"]]
                    entry.update(median_ms=statistics.median(times), min_ms=min(times),
                                 max_ms=max(times), peak_rss_kib=max(
                                     sample["peak_rss_kib"] for sample in entry["samples"]))
            results["cases"].append(entry)
            args.output.write_text(json.dumps(results, indent=2) + "\n")
            print(json.dumps({k: v for k, v in entry.items() if k in
                              ("name", "median_ms", "peak_rss_kib", "timeout_s")}), flush=True)


if __name__ == "__main__":
    main()
