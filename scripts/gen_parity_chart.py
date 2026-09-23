"""Generate the port-status chart on the docs site from `docs/PORTING.md`.

The chart is data, not decoration: it is parsed straight out of the parity table
so it cannot drift from it. Re-run after changing any module's status:

    python scripts/gen_parity_chart.py

Writes `docs/assets/port-status.svg` (a clickable stacked bar, each segment
linking to the section of the parity table it counts) and prints the totals.

The SVG is **inlined** into the page, not referenced with <img> or <object>:
links inside an embedded SVG resolve against the SVG's own URL and navigate
inside its frame, so an embedded chart looks clickable and goes nowhere. Its
hrefs are therefore relative to a *top-level* page (`../PORTING/`), which is
where it is included from.

Deliberately NOT a single "percent done" number. Most modules are partial, and
one number would hide that: a bar that is 78% amber says something true, while
"78% complete" would not. The figure quoted in prose is the share of upstream
modules with *some* working implementation, and the caption says so.
"""
import pathlib
import re
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
PORTING = ROOT / "docs" / "PORTING.md"
OUT = ROOT / "docs" / "assets" / "port-status.svg"

DONE, PARTIAL, NOT_STARTED = "🟢", "🟡", "⬜"

# Segment colours. Chosen to stay legible on the Material slate and default
# palettes, and distinguishable without relying on hue alone — each segment is
# also labelled with its count.
COLOURS = {
    DONE: "#2e7d32",
    PARTIAL: "#b8860b",
    NOT_STARTED: "#5f6368",
}
LABELS = {
    DONE: "Complete",
    PARTIAL: "Partial",
    NOT_STARTED: "Not started",
}


def parse():
    """Return [(section, [(module, status)])] from the parity tables."""
    sections = []
    current = None
    for line in PORTING.read_text(encoding="utf-8").splitlines():
        if line.startswith("## "):
            current = (line[3:].strip(), [])
            sections.append(current)
            continue
        if current is None or not line.startswith("|"):
            continue
        cells = [c.strip() for c in line.strip().strip("|").split("|")]
        if len(cells) < 3:
            continue
        # Skip the header and the |:---:| separator.
        if cells[0].lower().startswith("upstream") or set(cells[0]) <= set("-: "):
            continue
        status = next((s for s in (DONE, PARTIAL, NOT_STARTED) if s in line), None)
        if status is None:
            continue
        module = re.sub(r"`|\*\*", "", cells[0]).strip()
        current[1].append((module, status))
    return [s for s in sections if s[1]]


def anchor(title):
    """MkDocs/Python-Markdown slug for a heading."""
    slug = title.lower()
    slug = re.sub(r"[^\w\s-]", "", slug)
    return re.sub(r"[\s_]+", "-", slug).strip("-")


def label(title, limit=38):
    """Row label: heading text without Markdown, trimmed to fit beside the bar.

    The headings carry backticks and parentheticals for the page — `rich-cli`
    (tool — tracks upstream 1.8.1, see UPSTREAM.toml) — which render literally in
    SVG and run under the bar.
    """
    text = title.replace("`", "")
    text = re.sub(r"\s*\(.*?\)\s*$", "", text).strip()
    return text if len(text) <= limit else text[: limit - 1].rstrip() + "…"


def svg(sections):
    total = sum(len(mods) for _, mods in sections)
    counts = {s: 0 for s in COLOURS}
    for _, mods in sections:
        for _, st in mods:
            counts[st] += 1

    width, bar_h, pad = 720, 46, 8
    rows_y = 96
    row_h = 26
    height = rows_y + row_h * len(sections) + 30

    parts = [
        f'<svg xmlns="http://www.w3.org/2000/svg" '
        f'xmlns:xlink="http://www.w3.org/1999/xlink" '
        f'viewBox="0 0 {width} {height}" width="100%" '
        f'role="img" aria-labelledby="pstitle pdesc">',
        '<title id="pstitle">Port status by upstream module</title>',
        f'<desc id="pdesc">Of {total} upstream modules, '
        f'{counts[DONE]} are complete, {counts[PARTIAL]} are partial and '
        f'{counts[NOT_STARTED]} are not started. Each bar segment and each row '
        f'links to that section of the parity table.</desc>',
        '<style>'
        '.seg{cursor:pointer}'
        '.seg:hover{opacity:.82}'
        '.lbl{font:600 13px system-ui,-apple-system,Segoe UI,sans-serif;fill:#fff}'
        '.cap{font:13px system-ui,-apple-system,Segoe UI,sans-serif;fill:currentColor}'
        '.sm{font:12px system-ui,-apple-system,Segoe UI,sans-serif;fill:currentColor}'
        '.rowbg{fill:currentColor;opacity:.07}'
        '</style>',
    ]

    # --- the overall stacked bar, one segment per status -------------------
    x = 0.0
    for status in (DONE, PARTIAL, NOT_STARTED):
        n = counts[status]
        if not n:
            continue
        w = width * n / total
        pct = 100 * n / total
        parts.append(
            f'<a href="../PORTING/" '
            f'aria-label="{LABELS[status]}: {n} of {total} modules">'
            f'<rect class="seg" x="{x:.1f}" y="24" width="{w:.1f}" height="{bar_h}" '
            f'fill="{COLOURS[status]}">'
            f'<title>{LABELS[status]}: {n} modules ({pct:.0f}%)</title></rect>'
        )
        if w > 74:
            parts.append(
                f'<text class="lbl" x="{x + w / 2:.1f}" y="{24 + bar_h / 2 + 5:.0f}" '
                f'text-anchor="middle">{LABELS[status]} {n}</text>'
            )
        parts.append("</a>")
        x += w

    parts.append(
        f'<text class="cap" x="0" y="16">{total} upstream modules mapped · '
        f'{counts[DONE]} complete · {counts[PARTIAL]} partial · '
        f'{counts[NOT_STARTED]} not started</text>'
    )

    # --- one row per section, each a link into the table --------------------
    y = rows_y
    for title, mods in sections:
        done = sum(1 for _, s in mods if s == DONE)
        part = sum(1 for _, s in mods if s == PARTIAL)
        started = done + part
        frac = started / len(mods)
        parts.append(f'<a href="../PORTING/#{anchor(title)}" '
                     f'aria-label="{label(title)}: {started} of {len(mods)} modules started">')
        parts.append(f'<rect class="rowbg" x="0" y="{y - 14}" width="{width}" height="{row_h - 4}" rx="3"/>')
        parts.append(f'<text class="sm" x="8" y="{y + 2}">{label(title)}</text>')
        bx, bw = 330, 300
        parts.append(f'<rect x="{bx}" y="{y - 9}" width="{bw}" height="12" rx="6" '
                     f'fill="currentColor" opacity=".12"/>')
        parts.append(f'<rect class="seg" x="{bx}" y="{y - 9}" width="{bw * frac:.1f}" '
                     f'height="12" rx="6" fill="{COLOURS[DONE if frac == 1 else PARTIAL]}">'
                     f'<title>{label(title)}: {started} of {len(mods)} started, {done} complete</title></rect>')
        parts.append(f'<text class="sm" x="{bx + bw + 10}" y="{y + 2}">'
                     f'{started}/{len(mods)}</text>')
        parts.append("</a>")
        y += row_h

    parts.append("</svg>")
    return "\n".join(parts)


def main():
    sections = parse()
    if not sections:
        print("no parity rows found — has docs/PORTING.md changed shape?", file=sys.stderr)
        return 1
    OUT.parent.mkdir(parents=True, exist_ok=True)
    OUT.write_text(svg(sections), encoding="utf-8")

    total = sum(len(m) for _, m in sections)
    done = sum(1 for _, m in sections for _, s in m if s == DONE)
    part = sum(1 for _, m in sections for _, s in m if s == PARTIAL)
    todo = total - done - part
    print(f"wrote {OUT.relative_to(ROOT)}")
    print(f"  modules      {total}")
    print(f"  complete     {done:>3}  ({100 * done / total:.0f}%)")
    print(f"  partial      {part:>3}  ({100 * part / total:.0f}%)")
    print(f"  not started  {todo:>3}  ({100 * todo / total:.0f}%)")
    print(f"  implemented  {done + part:>3}  ({100 * (done + part) / total:.0f}%)")
    for title, mods in sections:
        started = sum(1 for _, s in mods if s != NOT_STARTED)
        print(f"    {title:<42} {started}/{len(mods)}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
