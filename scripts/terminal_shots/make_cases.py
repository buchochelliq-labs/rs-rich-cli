#!/usr/bin/env python3
"""Build the fixture directory and cases.json for shoot.js.

Every screenshot is a real PTY run of the given `rich` binary; shoot.js only
replays the captured bytes into xterm.js. Run from this directory:

    python3 make_cases.py --bin-dir DIR --work WORKDIR --image PNG [--release 0.0.10] > cases.json
    npm install && CHROMIUM=/path/to/chrome node shoot.js cases.json OUTDIR
"""
import argparse
import json
import shlex
import shutil
from pathlib import Path


def fixtures(work: Path, image: Path) -> None:
    if work.exists():
        shutil.rmtree(work)
    (work / "input/api").mkdir(parents=True)
    (work / "input/guides").mkdir(parents=True)
    shutil.copy(image, work / "gradient.png")
    (work / "rich.toml").write_text(
        '[themes.night]\nnotice = "bold cyan"\nwarning = "bold yellow"\n'
        '"markdown.h1" = "bold magenta"\n'
    )
    (work / "input/README.md").write_text("# Release notes\n\n- themes\n- doctor\n- batch templates\n")
    (work / "input/api/meta.json").write_text('{"name": "rs-rich-cli", "version": "0.0.9", "ok": true}\n')
    (work / "input/api/items.json").write_text('{"items": [1, 2, 3], "nested": {"a": null}}\n')
    (work / "input/guides/intro.md").write_text("# Guide\n\nUse `rich --batch`.\n")
    (work / "input/guides/cohort.csv").write_text(
        "name,crate,version\ncore,rs-rich,0.0.5\next,rs-rich-ext,0.0.7\n"
        "art,rs-rich-art,0.0.7\ncli,rs-rich-cli,0.0.9\n"
    )
    for i in range(1, 25):
        (work / f"input/guides/page{i}.md").write_text(f"# Page {i}\n\nBody {i}\n")


def case(name, title, commands, work, bin_dir, cols=100, rows=24, **extra):
    # Echo each command as a prompt line, then run it in the same PTY. A command
    # may be a (shown, run) pair when the real invocation needs shell plumbing
    # (backgrounding, a scripted edit) that would only clutter the prompt line.
    pairs = [c if isinstance(c, tuple) else (c, c) for c in commands]
    script = "; ".join(
        f"printf '\\033[1;32m$\\033[0m %s\\n' {shlex.quote(shown)}; {run}" for shown, run in pairs
    )
    return {"name": name, "title": title, "cmd": "sh", "args": ["-c", script],
            "cwd": str(work), "cols": cols, "rows": rows,
            "env": {"PATH": f"{bin_dir}:/usr/bin:/bin"}, **extra}


def fixtures_010(work: Path) -> None:
    (work / "strike.md").write_text(
        "# Strikethrough like markdown-it\n\n"
        "- `~~done~~` → ~~done~~\n"
        "- `a ~~~x~~~ b` → a ~~~x~~~ b\n"
        "- `~~one~~ and ~~two~~` → ~~one~~ and ~~two~~\n"
    )
    (work / "status.json").write_text('{"build": "running", "step": 1}\n')
    (work / "notes.md").write_text("# Notes\n\n- watching two files\n")
    (work / "edit-status.sh").write_text(
        "sleep 1.5\nprintf '{\"build\": \"passed\", \"step\": 2}\\n' > status.json\nsleep 1.5\n"
    )


def cases_009(c, work):
    notice = "'[notice]notice: Ready[/]  [warning]warning: Check config[/]'"
    batch = ("rich --no-config --batch --batch-preserve-dirs --batch-input-root input "
             "--batch-name-template '{index}-{stem}.{output_ext}'")
    img = ("rich --no-config image gradient.png --image-mode blocks --width 48 "
           "--height 14 --image-fit contain")
    return [
        c("01-version-doctor", "Installed from the packaged crates: version and read-only diagnostics",
          ["rich --version", "rich doctor --no-config"], rows=11),
        c("02-themes", "Named theme from config, then an explicit --theme-style override",
          [f"rich --config rich.toml --theme night --print {notice}",
           f"rich --config rich.toml --theme night --theme-style 'notice=bold green' --print {notice}",
           "rich --config rich.toml --theme night config show | grep -A4 theme_styles"], rows=13),
        c("03-batch-dry-run", "Batch dry run: preserved directories and a filename template",
          [f"{batch} --export-html out --dry-run input | head -14"], cols=110, rows=17),
        c("04-batch-jobs", "Parallel batch export with --jobs 4; progress drawn on terminal stderr",
          [f"{batch} --export-html out-run --jobs 4 input >/dev/null",
           "find out-run -type f | wc -l"], cols=110, rows=16, timeoutMs=60000),
        c("05-image-default", "Image: truecolor half-blocks (default)", [img], cols=72, rows=18),
        c("06-image-ansi256-bayer", "Image: ANSI256 + ordered Bayer 4x4 dithering",
          [f"{img} --image-color ansi256 --image-dither bayer4x4"], cols=72, rows=18),
        c("07-image-rotate-gray", "Image: rotate 90, flip horizontally, grayscale",
          [f"{img} --image-rotate 90 --image-flip-horizontal --image-grayscale"], cols=72, rows=18),
        c("08-demo-list", "Guided tour sections", ["rich --demo-list"], rows=6),
    ]


def cases_010(c, work):
    img = "rich --no-config image gradient.png --width 48 --height 14 --image-fit contain"
    watch = "rich --no-config --watch --watch-debounce 0.1 status.json notes.md"
    return [
        c("01-version-doctor", "Installed from the packaged crates: version and read-only diagnostics",
          ["rich --version", "rich doctor --no-config"], rows=11),
        c("02-image-quadrants", "Image: quadrant blocks, 2×2 pixels per cell",
          [f"{img} --image-mode quadrants"], cols=72, rows=18),
        c("03-image-ansi16-bayer", "Image: quadrants in the 16 theme colours with Bayer 4×4",
          [f"{img} --image-mode quadrants --image-color ansi16 --image-dither bayer4x4"],
          cols=72, rows=18),
        c("04-image-gray-tone", "Image: grayscale palette with brightness, contrast and gamma",
          [f"{img} --image-mode blocks --image-color grayscale --image-brightness 1.1 "
           "--image-contrast 1.4 --image-gamma 0.8"], cols=100, rows=18),
        c("05-markdown-strike", "Markdown: tilde runs paired as markdown-it pairs them",
          ["rich --no-config strike.md --width 60"], cols=72, rows=10),
        c("06-watch-two-files", "Multi-file --watch: one live region per file; only the edited one repaints",
          [(watch, f"({watch} & p=$!; sh edit-status.sh; kill -INT $p; wait $p)")],
          cols=80, rows=16, timeoutMs=20000),
    ]


CASE_SETS = {"0.0.9": cases_009, "0.0.10": cases_010}


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--bin-dir", type=Path, required=True)
    parser.add_argument("--work", type=Path, required=True)
    parser.add_argument("--image", type=Path, required=True)
    parser.add_argument("--release", choices=sorted(CASE_SETS), default="0.0.9",
                        help="which release's screenshot set to build (default 0.0.9)")
    args = parser.parse_args()
    work, bin_dir = args.work.resolve(), args.bin_dir.resolve()
    fixtures(work, args.image)
    fixtures_010(work)
    c = lambda *a, **k: case(*a, work=work, bin_dir=bin_dir, **k)
    print(json.dumps(CASE_SETS[args.release](c, work), indent=1))


if __name__ == "__main__":
    main()
