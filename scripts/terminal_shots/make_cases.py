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
          ["rich --version", "rich doctor --no-config"], rows=25),
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

    def image(*groups):
        # Shown as typed shell continuations so the prompt line never wraps mid-word.
        shown = img + "".join(f" \\\n    {group}" for group in groups)
        return (shown, " ".join((img, *groups)))

    return [
        c("01-version-doctor", "Installed from the packaged crates: version and read-only diagnostics",
          ["rich --version", "rich doctor --no-config"], rows=25),
        c("02-image-quadrants", "Image: quadrant blocks, 2×2 pixels per cell",
          [image("--image-mode quadrants")], rows=19),
        c("03-image-ansi16-bayer", "Image: quadrants in the 16 theme colours with Bayer 4×4",
          [image("--image-mode quadrants --image-color ansi16 --image-dither bayer4x4")], rows=19),
        c("04-image-gray-tone", "Image: grayscale palette with brightness, contrast and gamma",
          [image("--image-mode blocks --image-color grayscale",
                 "--image-brightness 1.1 --image-contrast 1.4 --image-gamma 0.8")], rows=20),
        c("05-markdown-strike", "Markdown: tilde runs paired as markdown-it pairs them",
          ["rich --no-config strike.md --width 60"], cols=72, rows=10),
        c("06-watch-two-files", "Multi-file --watch: one live region per file; only the edited one repaints",
          [(watch, f"({watch} & p=$!; sh edit-status.sh; kill -INT $p; wait $p)")],
          cols=80, rows=16, timeoutMs=20000),
    ]


def write_rgba_png(path: Path, width: int, height: int, pixel) -> None:
    """A minimal RGBA PNG writer, so the alpha fixture needs no imaging library."""
    import struct
    import zlib

    def chunk(kind: bytes, data: bytes) -> bytes:
        return (struct.pack(">I", len(data)) + kind + data
                + struct.pack(">I", zlib.crc32(kind + data) & 0xFFFFFFFF))

    rows = b"".join(
        b"\x00" + b"".join(bytes(pixel(x, y)) for x in range(width)) for y in range(height)
    )
    path.write_bytes(
        b"\x89PNG\r\n\x1a\n"
        + chunk(b"IHDR", struct.pack(">IIBBBBB", width, height, 8, 6, 0, 0, 0))
        + chunk(b"IDAT", zlib.compress(rows, 9))
        + chunk(b"IEND", b"")
    )


def fixtures_011(work: Path) -> None:
    (work / "deploy.yaml").write_text(
        "service: api\nreplicas: 3\nimage:\n  name: rs-rich\n  tag: \"0.0.11\"\n"
        "ports: [8080, 8443]\nlimits:\n  cpu: 500m\n  memory: 256Mi\n"
    )
    (work / "old.toml").write_text("retries = 2\ntimeout = 30\nmode = \"fast\"\n")
    (work / "new.toml").write_text("retries = 3\ntimeout = 30\nmode = \"safe\"\nlog = true\n")
    (work / "text.txt").write_text("cafe\u0301 🦀 ok\n")
    (work / "codes.txt").write_text(
        "\x1b[1;31mError\x1b[0m \x1b]8;;https://example.com\x1b\\docs\x1b]8;;\x1b\\\n"
    )
    (work / "demo.theme").write_text("[styles]\nrelease = bold magenta\nrepr.number = bold cyan\n")

    # A pink disc and a translucent cyan bar on a fully transparent canvas.
    def pixel(x, y):
        if (x - 38) ** 2 + (y - 30) ** 2 < 24 * 24:
            return (255, 110, 190, 255)
        if x > 78 and 12 < y < 48:
            return (70, 220, 255, 210)
        return (0, 0, 0, 0)

    write_rgba_png(work / "alpha.png", 120, 60, pixel)


def fixtures_012(work: Path) -> None:
    (work / "release.mmd").write_text(
        "flowchart LR\n"
        "    core[rs-rich 0.0.8] --> api[plugin API 0.0.1]\n"
        "    api --> mermaid[rs-rich-mermaid]\n"
        "    api --> lumis[rs-rich-lumis]\n"
        "    core --> py{{rs_rich on PyPI}}\n"
        "    mermaid --> cli([rich 0.0.12])\n"
        "    lumis -.-> cli\n"
    )
    (work / "design.md").write_text(
        "# Release flow\n\n"
        "Fenced Mermaid in Markdown is drawn, not printed:\n\n"
        "```mermaid\nflowchart TD\n    tag[Tag on main] --> ci{CI green?}\n"
        "    ci -->|yes| publish[Publish]\n    ci -->|no| fix[Fix]\n```\n"
    )
    (work / "worker.rs").write_text(
        "/// Retry a job with backoff.\n"
        "fn retry(job: &Job, attempts: u32) -> Result<(), Error> {\n"
        "    for n in 0..attempts {\n"
        "        if job.run().is_ok() {\n"
        "            return Ok(());\n"
        "        }\n"
        "        sleep(Duration::from_millis(100 << n));\n"
        "    }\n"
        "    Err(Error::GaveUp(attempts))\n"
        "}\n"
    )
    (work / "service.log").write_text(
        "09:00:01 INFO  api listening on :8080\n"
        "09:00:04 WARN  cache miss rate 12%\n"
        "09:00:07 ERROR upstream timeout after 30s\n"
        "09:00:09 INFO  retrying upstream\n"
        "09:00:12 ERROR connection refused by db:5432\n"
        "09:00:15 INFO  recovered\n"
    )

    # A 24x16 pixel sprite: native sizing draws it at its own size, never enlarged.
    def pixel(x, y):
        if (x - 12) ** 2 + (y - 8) ** 2 < 7 * 7:
            return (255, 196, 0, 255) if y < 8 else (255, 120, 0, 255)
        if y > 13:
            return (60, 170, 90, 255)
        return (40, 60, 120, 255)

    write_rgba_png(work / "sprite.png", 24, 16, pixel)


def cases_011(c, work):
    img = "rich --no-config image gradient.png --width 48 --height 14 --image-fit contain"
    alpha = "rich --no-config image alpha.png --image-mode quadrants --width 48 --height 12"
    secret = "echo deploy token=ghp_0123456789abcdefghijklmnopqrstuvwxyzAB"

    def image(*groups):
        shown = img + "".join(f" \\\n    {group}" for group in groups)
        return (shown, " ".join((img, *groups)))

    return [
        c("01-version-doctor", "Installed from the packaged crates: version and read-only diagnostics",
          ["rich --version", "rich doctor --no-config"], rows=25),
        c("02-inspect", "rich inspect: structured data as a tree",
          ["rich --no-config inspect deploy.yaml"], cols=80, rows=15),
        c("03-diff", "rich diff: a text diff with line numbers and a summary",
          ["rich --no-config diff old.toml new.toml"], cols=80, rows=13),
        c("04-view-search", "rich view: detect, highlight and search any file",
          ["rich --no-config view deploy.yaml --search rs-rich --no-pager"], cols=80, rows=13),
        c("05-hex-unicode", "rich hex and rich unicode: bytes and graphemes",
          ["rich --no-config hex text.txt", "rich --no-config unicode text.txt"], cols=90, rows=24),
        c("06-ansi-explain", "rich ansi explain: escape sequences in words",
          ["rich --no-config ansi explain codes.txt"], cols=90, rows=21),
        c("07-capture-redact", "rich capture --redact (experimental): check captures before sharing",
          [f"rich --no-config capture --redact -- sh -c '{secret}'"], cols=90, rows=7),
        c("08-image-atkinson-oklab", "Image: ANSI16 with Atkinson dithering and OKLab colour matching",
          [image("--image-mode quadrants --image-color ansi16",
                 "--image-dither atkinson --image-color-distance oklab")], rows=20),
        c("09-alpha-backgrounds", "Transparent pixels: checkerboard preview, then the terminal's own background",
          [f"{alpha} --image-background checkerboard",
           f"{alpha} --image-background default"], cols=72, rows=32),
        c("10-theme-file", "An upstream [styles] theme file with --theme-file",
          ["rich --no-config --theme-file demo.theme --print '[release]0.0.11[/] ships 12 workstreams'"],
          cols=80, rows=4),
    ]


def cases_012(c, work):
    return [
        c("01-version-doctor", "Installed from the packaged crates: version, highlighters and plugins",
          ["rich --version", "rich doctor --no-config"], rows=30),
        c("02-mermaid", "rich mermaid: a flowchart drawn as text, no browser needed",
          ["rich --no-config mermaid release.mmd"], cols=100, rows=24),
        c("03-markdown-mermaid", "A ```mermaid fence in Markdown is drawn in place",
          ["rich --no-config design.md"], cols=80, rows=22),
        c("04-code-theme", "--code-theme ansi_dark: code in the terminal's own palette",
          ["rich --no-config worker.rs --code-theme ansi_dark --line-numbers"], cols=80, rows=12),
        c("05-filter-highlight", "--filter keeps matching lines; --highlight marks matches",
          ["rich --no-config service.log --filter ERROR --highlight 'timeout|refused'",
           "rich --no-config inspect deploy.yaml --highlight '$.limits.*'"], cols=80, rows=22),
        c("06-image-native", "--image-fit native: an image at its own pixel size",
          ["rich --no-config image sprite.png --image-fit native --image-mode quadrants",
           "rich --no-config image sprite.png --image-fit native --image-mode half-block"],
          cols=60, rows=20),
    ]


CASE_SETS = {"0.0.9": cases_009, "0.0.10": cases_010, "0.0.11": cases_011, "0.0.12": cases_012}


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
    fixtures_011(work)
    fixtures_012(work)
    c = lambda *a, **k: case(*a, work=work, bin_dir=bin_dir, **k)
    print(json.dumps(CASE_SETS[args.release](c, work), indent=1))


if __name__ == "__main__":
    main()
