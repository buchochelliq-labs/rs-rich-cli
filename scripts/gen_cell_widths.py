"""Generate the vendored cell-width tables from the installed rich 15.0.0.

docs/DIVERGENCES.md #1 delegated per-character widths to the `unicode-width`
crate on the premise that both implement the same East Asian Width rules, and
named "a concrete codepoint mismatch" as the trigger to vendor upstream's table
instead. Round 8 found 348 of them.

Upstream ships *every* Unicode version from 4.1.0 to 17.0.0 and picks between
them with the `UNICODE_VERSION` environment variable (`rich._unicode_data.load`),
so we vendor every one of them: a terminal pinned to an older Unicode measures
emoji differently, and a port that only knows the newest table cannot follow.

Run with the interpreter that has rich 15.0.0 installed:

    python scripts/gen_cell_widths.py
"""

import pathlib

from rich._unicode_data import load as load_cell_table
from rich._unicode_data._versions import VERSIONS

out = pathlib.Path(__file__).resolve().parent.parent / "crates" / "rich" / "src" / "cell_widths.rs"

tables = {version: load_cell_table(version) for version in VERSIONS}

# Every shipped version carries the same `narrow_to_wide` set, so one copy does.
narrow_to_wide = sorted(tables[VERSIONS[-1]].narrow_to_wide)
for version, table in tables.items():
    assert sorted(table.narrow_to_wide) == narrow_to_wide, (
        f"unicode {version} has its own narrow_to_wide set; the single shared "
        f"NARROW_TO_WIDE array in cell_widths.rs is no longer valid"
    )


def ident(version: str) -> str:
    return "UNICODE_" + version.replace(".", "_")


lines = []
lines.append("//! Vendored character-width data from upstream `rich`.")
lines.append("//!")
lines.append("//! Generated from `rich._unicode_data`, shipped with rich 15.0.0. Do not edit by")
lines.append("//! hand; regenerate with `scripts/gen_cell_widths.py` when syncing upstream.")
lines.append("//!")
lines.append("//! We previously delegated to the `unicode-width` crate. It disagrees with")
lines.append("//! upstream on 348 code points in a 21,500-point sweep — spacing marks (Mc),")
lines.append("//! format characters (Cf), modifier symbols (Sk) — which misaligned every")
lines.append("//! table, panel and wrap point containing them.")
lines.append("//!")
lines.append(f"//! All {len(VERSIONS)} tables upstream ships are here, because `UNICODE_VERSION`")
lines.append("//! selects between them at runtime and a terminal pinned to an older Unicode")
lines.append("//! measures emoji differently from the newest one.")
lines.append("")

for version in VERSIONS:
    widths = list(tables[version].widths)
    lines.append(f"/// `(start, end, width)` ranges, sorted and non-overlapping, for Unicode {version}.")
    lines.append(f"static {ident(version)}: [(u32, u32, u8); {len(widths)}] = [")
    for start, end, width in widths:
        lines.append(f"    (0x{start:04X}, 0x{end:04X}, {width}),")
    lines.append("];")
    lines.append("")

lines.append("/// The Unicode versions upstream ships, oldest first — `_versions.VERSIONS`.")
lines.append(f"pub(crate) static VERSIONS: [&str; {len(VERSIONS)}] = [")
for version in VERSIONS:
    lines.append(f'    "{version}",')
lines.append("];")
lines.append("")
lines.append("/// The width table for each entry of [`VERSIONS`], in the same order.")
lines.append(f"pub(crate) static TABLES: [&[(u32, u32, u8)]; {len(VERSIONS)}] = [")
for version in VERSIONS:
    lines.append(f"    &{ident(version)},")
lines.append("];")
lines.append("")
lines.append("/// Characters that are one cell alone but two when followed by U+FE0F")
lines.append("/// (variation selector 16). Upstream's `narrow_to_wide`, which is identical")
lines.append("/// across every shipped Unicode version — the generator asserts that.")
lines.append(f"pub(crate) static NARROW_TO_WIDE: [char; {len(narrow_to_wide)}] = [")
for ch in narrow_to_wide:
    lines.append(f"    '\\u{{{ord(ch):04X}}}',")
lines.append("];")
lines.append("")

out.write_text("\n".join(lines), encoding="utf-8")
total = sum(len(tables[v].widths) for v in VERSIONS)
print(
    f"wrote {out.name}: {len(VERSIONS)} tables, {total} ranges, "
    f"{len(narrow_to_wide)} narrow_to_wide"
)
