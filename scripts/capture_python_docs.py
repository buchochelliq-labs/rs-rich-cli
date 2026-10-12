"""Regenerate the Python docs site's pictures from its example scripts.

Every `crates/rich-py/examples/docs/<page>.py` renders the examples of one
page of the Python site (`docs/python/<page>.md`; `ext_tables.py` is
`ext/tables.md`) with `rs_rich`, and writes one SVG per example,
`<page>-<example>.svg`, into the directory it is given. This script runs each
of them with this interpreter, which must have `rs_rich` installed, into
`docs/python/media`. Each script also checks that the code its page shows is
the code it ran (see `crates/rich-py/examples/docs/_shots.py`).

With `--check` it renders into a temporary directory and fails when any
checked-in picture differs, is missing, or is no longer made by any script,
which is how a stale image is caught. `--only NAME` (repeatable) limits it to
some pages: `table`, `ext/tables` or `ext_tables`.
"""

import argparse
import filecmp
import os
import pathlib
import subprocess
import sys
import tempfile

ROOT = pathlib.Path(__file__).resolve().parent.parent
SCRIPTS = ROOT / "crates" / "rich-py" / "examples" / "docs"
MEDIA = ROOT / "docs" / "python" / "media"
# What the scripts must not inherit from the terminal they run in.
DROP = ("COLUMNS", "LINES", "NO_COLOR", "FORCE_COLOR", "COLORTERM", "TERM_PROGRAM", "CI")


def scripts():
    """Every docs script, in name order (the shared `_shots.py` excluded)."""
    return [path for path in sorted(SCRIPTS.glob("*.py")) if not path.name.startswith("_")]


def key(name):
    return name.replace("/", "_").replace("-", "_").removesuffix(".py")


def capture(out, only):
    out.mkdir(parents=True, exist_ok=True)
    env = {name: value for name, value in os.environ.items() if name not in DROP}
    env["PYTHONIOENCODING"] = "utf-8"
    failed = []
    for script in scripts():
        if only and script.stem not in only:
            continue
        path = script.relative_to(ROOT).as_posix()
        print(f"  {path}")
        result = subprocess.run([sys.executable, path, str(out)], cwd=ROOT, env=env,
                                stdout=subprocess.DEVNULL, stderr=subprocess.PIPE, text=True)
        if result.returncode != 0:
            sys.stderr.write(result.stderr)
            failed.append(script.stem)
    return failed


def made_by(svg, stems):
    """Whether a picture's name belongs to one of these scripts' pages."""
    return any(svg.name.startswith(stem.replace("_", "-") + "-") for stem in stems)


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--check", action="store_true",
                        help="fail if a checked-in picture is stale, missing or orphaned")
    parser.add_argument("--only", action="append", default=[], metavar="NAME",
                        help="limit to this page (repeatable)")
    parser.add_argument("--list", action="store_true", help="list the scripts and exit")
    args = parser.parse_args()

    only = {key(name) for name in args.only}
    known = {script.stem for script in scripts()}
    if only - known:
        parser.error(f"no docs script for: {', '.join(sorted(only - known))}")
    if args.list:
        for script in scripts():
            print(script.stem)
        return 0
    if not args.check:
        failed = capture(MEDIA, only)
        if failed:
            print("failed: " + ", ".join(failed), file=sys.stderr)
            return 1
        return 0

    with tempfile.TemporaryDirectory() as tmp:
        fresh = pathlib.Path(tmp)
        failed = capture(fresh, only)
        stale = [p.name for p in sorted(fresh.glob("*.svg"))
                 if not (MEDIA / p.name).exists()
                 or not filecmp.cmp(p, MEDIA / p.name, shallow=False)]
        made = {p.name for p in fresh.glob("*.svg")}
        ran = only or known
        orphans = [p.name for p in sorted(MEDIA.glob("*.svg"))
                   if p.name not in made and (not only or made_by(p, ran))]
        if failed or stale or orphans:
            for name in failed:
                print(f"script failed: {name}", file=sys.stderr)
            for name in stale:
                print(f"stale picture: docs/python/media/{name}", file=sys.stderr)
            for name in orphans:
                print(f"picture no script makes: docs/python/media/{name}", file=sys.stderr)
            print("Run python scripts/capture_python_docs.py", file=sys.stderr)
            return 1
    print("Python docs pictures match their scripts")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
