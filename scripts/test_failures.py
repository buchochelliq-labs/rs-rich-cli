#!/usr/bin/env python3
"""Pull the failing tests out of a `cargo test` log.

A full workspace run prints tens of thousands of lines, and a failure in an
early test target ends up far above the end of the log, where log viewers
and tools that read only the tail never reach. CI saves the run's output
and, when it fails, runs this to print each failing test's own output and
cargo's summary at the end, and into the job summary:

    cargo test --all --no-fail-fast 2>&1 | tee test.log
    python3 scripts/test_failures.py test.log >> "$GITHUB_STEP_SUMMARY"

It prints Markdown. With nothing failing it says so and exits 0.
"""

import re
import sys

# Where each test target's run starts: `Running tests/x.rs (target/...)`,
# `Running unittests src/lib.rs (...)`, or a crate's doc-tests.
RUNNING = re.compile(r"^\s*(Running \S+(?: \S+)? \(.*\)|Doc-tests \S+)")
# A failing test's captured output starts with `---- name stdout ----`.
STDOUT = re.compile(r"^---- (.+) stdout ----$")
# libtest's list of failures, then the result line, ends a target's report.
FAILURES = re.compile(r"^failures:$")
RESULT = re.compile(r"^test result: ")
# Cargo's own lines that say which targets failed.
CARGO = re.compile(r"^(error: test failed, to rerun pass|error: \d+ targets? failed:|\s+`-p .+`$)")
# GitHub's timestamps and colour codes, which every line may carry.
TIMESTAMP = re.compile(r"^\d{4}-\d\d-\d\dT[\d:.]+Z ")
COLOUR = re.compile(r"\x1b\[[0-9;]*m")

# The job summary takes 1 MiB; keep each failure, and the whole, well short.
PER_TEST = 8000
TOTAL = 200_000


def clean(line):
    return COLOUR.sub("", TIMESTAMP.sub("", line.rstrip("\n")))


def failures(lines):
    """The failing tests as (target, name, output), and cargo's summary."""
    found = []
    summary = []
    target = None
    name = None
    output = []
    for line in map(clean, lines):
        running = RUNNING.match(line)
        if running:
            target = running.group(1)
            continue
        if CARGO.match(line):
            summary.append(line.strip())
        start = STDOUT.match(line)
        if start:
            if name is not None:
                found.append((target, name, output))
            name, output = start.group(1), []
            continue
        if name is not None:
            if FAILURES.match(line) or RESULT.match(line):
                found.append((target, name, output))
                name = None
            else:
                output.append(line)
    if name is not None:
        found.append((target, name, output))
    return found, summary


def report(found, summary):
    if not found and not summary:
        return "No failing tests in the log.\n"
    out = ["## Failing tests\n"]
    if summary:
        out.append("```\n" + "\n".join(summary) + "\n```\n")
    for target, name, output in found:
        text = "\n".join(output).strip("\n")
        if len(text) > PER_TEST:
            text = text[:PER_TEST] + f"\n… ({len(text) - PER_TEST} more characters)"
        out.append(f"### `{name}`\n\n{target or 'unknown target'}\n\n```\n{text}\n```\n")
    whole = "\n".join(out)
    if len(whole) > TOTAL:
        whole = whole[:TOTAL] + "\n… (cut)\n"
    return whole


def main(argv):
    if len(argv) != 2:
        print(__doc__.strip(), file=sys.stderr)
        return 2
    with open(argv[1], encoding="utf-8", errors="replace") as log:
        found, summary = failures(log)
    sys.stdout.write(report(found, summary))
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
