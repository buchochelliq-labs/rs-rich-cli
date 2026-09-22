#!/usr/bin/env python3
"""Build the fixture directory and cases.json for shoot.js.

Every screenshot is a real PTY run of the given `rich` binary; shoot.js only
replays the captured bytes into xterm.js. Run from this directory:

    python3 make_cases.py --bin-dir DIR --work WORKDIR --image PNG > cases.json
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
    # Echo each command as a prompt line, then run it in the same PTY.
    script = "; ".join(
        f"printf '\\033[1;32m$\\033[0m %s\\n' {shlex.quote(c)}; {c}" for c in commands
    )
    return {"name": name, "title": title, "cmd": "sh", "args": ["-c", script],
            "cwd": str(work), "cols": cols, "rows": rows,
            "env": {"PATH": f"{bin_dir}:/usr/bin:/bin"}, **extra}


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--bin-dir", type=Path, required=True)
    parser.add_argument("--work", type=Path, required=True)
    parser.add_argument("--image", type=Path, required=True)
    args = parser.parse_args()
    work, bin_dir = args.work.resolve(), args.bin_dir.resolve()
    fixtures(work, args.image)
    notice = "'[notice]notice: Ready[/]  [warning]warning: Check config[/]'"
    batch = ("rich --no-config --batch --batch-preserve-dirs --batch-input-root input "
             "--batch-name-template '{index}-{stem}.{output_ext}'")
    img = ("rich --no-config image gradient.png --image-mode blocks --width 48 "
           "--height 14 --image-fit contain")
    c = lambda *a, **k: case(*a, work=work, bin_dir=bin_dir, **k)
    cases = [
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
    print(json.dumps(cases, indent=1))


if __name__ == "__main__":
    main()
