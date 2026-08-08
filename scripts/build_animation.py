#!/usr/bin/env python3
"""Stitch a sequence of exported SVG frames into one animated SVG.

Each frame is a complete SVG produced by `Console::export_svg`. They share a
viewBox and a style block (the exporter is deterministic given a fixed
`unique_id`), so the frames can be stacked as sibling groups and cycled with a
CSS animation that shows exactly one at a time.

Why not a GIF: a GIF of terminal output means rasterising text, which needs a
font toolchain, picks a font that is not the reader's, and produces a blurry
artifact that cannot be diffed. An animated SVG stays sharp at any zoom, has
selectable text, needs no dependencies, and reviews as a text diff.

    python scripts/build_animation.py 'frames/progress-*.svg' out.svg --duration 3.0

Frames are ordered by the trailing integer in the filename, not lexically —
`spinner-10.svg` must follow `spinner-9.svg`.
"""

from __future__ import annotations

import argparse
import glob
import pathlib
import re
import sys

# The exporter emits `<svg …>` … `</svg>` with a `<style>` block inside. Pull the
# style out once (it is identical across frames) and the drawable content per
# frame.
STYLE_RE = re.compile(r"<style>(.*?)</style>", re.S)
SVG_OPEN_RE = re.compile(r"<svg\b[^>]*>", re.S)
VIEWBOX_RE = re.compile(r'viewBox="([^"]+)"')


def frame_sort_key(path: str) -> tuple:
    """Order by the trailing integer, so 10 follows 9 rather than 1."""
    numbers = re.findall(r"(\d+)", pathlib.Path(path).stem)
    return (int(numbers[-1]) if numbers else 0, path)


def split_frame(text: str) -> tuple[str, str, str]:
    """Return (viewBox, style_body, inner_content) for one exported SVG."""
    open_tag = SVG_OPEN_RE.search(text)
    if not open_tag:
        raise SystemExit("not an SVG: no <svg> element")
    view_box = VIEWBOX_RE.search(open_tag.group(0))
    if not view_box:
        raise SystemExit("SVG has no viewBox; cannot compose frames")

    style = STYLE_RE.search(text)
    style_body = style.group(1) if style else ""

    inner = text[open_tag.end():]
    inner = inner[: inner.rindex("</svg>")]
    if style:
        inner = inner.replace(style.group(0), "", 1)
    return view_box.group(1), style_body, inner


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("pattern", help="glob matching the frame SVGs")
    parser.add_argument("output", help="path to write the animated SVG to")
    parser.add_argument(
        "--duration",
        type=float,
        default=2.0,
        help="seconds for one full loop (default: 2.0)",
    )
    args = parser.parse_args()

    paths = sorted(glob.glob(args.pattern), key=frame_sort_key)
    if not paths:
        raise SystemExit(f"no frames matched {args.pattern!r}")

    view_box = None
    style_body = ""
    frames: list[str] = []
    for path in paths:
        vb, style, inner = split_frame(pathlib.Path(path).read_text(encoding="utf-8"))
        if view_box is None:
            view_box, style_body = vb, style
        elif vb != view_box:
            # Different-sized frames would jump around; better to fail loudly
            # than to ship an image that visibly jitters.
            raise SystemExit(
                f"frame {path} has viewBox {vb!r}, expected {view_box!r} — "
                "render every frame at the same width"
            )
        frames.append(inner)

    count = len(frames)
    step = 100.0 / count
    # Hold each frame for its whole slot, then swap instantly: `opacity` jumps
    # at the boundary rather than fading, which is what terminal output does.
    keyframes = []
    for index in range(count):
        start = index * step
        end = start + step
        keyframes.append(
            f"@keyframes f{index}{{"
            f"0%,{start:.4f}%{{opacity:0}}"
            f"{start:.4f}%,{end:.4f}%{{opacity:1}}"
            f"{end:.4f}%,100%{{opacity:0}}}}"
        )
    frame_rules = "".join(
        f".frame{index}{{opacity:0;animation:f{index} {args.duration}s steps(1,end) infinite}}"
        for index in range(count)
    )

    groups = "\n".join(
        f'<g class="frame{index}">{inner}</g>' for index, inner in enumerate(frames)
    )

    out = (
        f'<svg class="rich-terminal" viewBox="{view_box}" '
        f'xmlns="http://www.w3.org/2000/svg">\n'
        f"<style>\n{style_body}\n"
        f"{''.join(keyframes)}\n{frame_rules}\n"
        # Anything that cannot animate (a still image in a feed reader, a PDF
        # export) shows the last frame rather than a blank box.
        f"@media (prefers-reduced-motion: reduce){{"
        f"{''.join(f'.frame{i}{{animation:none;opacity:0}}' for i in range(count - 1))}"
        f".frame{count - 1}{{animation:none;opacity:1}}}}\n"
        f"</style>\n{groups}\n</svg>\n"
    )

    output = pathlib.Path(args.output)
    output.parent.mkdir(parents=True, exist_ok=True)
    # Pin LF. Without this the platform default is used, so regenerating on
    # Windows rewrites every byte and each commit carries a whole-file diff.
    output.write_text(out, encoding="utf-8", newline="\n")
    print(f"  {output}  ({count} frames, {args.duration}s loop)", file=sys.stderr)


if __name__ == "__main__":
    main()
