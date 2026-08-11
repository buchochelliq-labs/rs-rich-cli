"""Generate the vendored cell-width table from the installed rich 15.0.0.

docs/DIVERGENCES.md #1 delegated per-character widths to the `unicode-width`
crate on the premise that both implement the same East Asian Width rules, and
named "a concrete codepoint mismatch" as the trigger to vendor upstream's table
instead. Round 8 found 348 of them.
"""
import pathlib
import rich.cells as c

table = c.load_cell_table("auto")
widths = list(table.widths)
narrow_to_wide = sorted(table.narrow_to_wide)

out = pathlib.Path(
    r"C:\Users\nickn\OneDrive\Documents\GitHub\rs-rich-cli\crates\rich\src\cell_widths.rs"
)

lines = []
lines.append("//! Vendored character-width data from upstream `rich`.")
lines.append("//!")
lines.append(f"//! Generated from `rich._cell_widths` / `rich.cells.load_cell_table` at")
lines.append(f"//! Unicode {table.unicode_version}, shipped with rich 15.0.0. Do not edit by hand;")
lines.append("//! regenerate with `scripts/gen_cell_widths.py` when syncing upstream.")
lines.append("//!")
lines.append("//! We previously delegated to the `unicode-width` crate. It disagrees with")
lines.append("//! upstream on 348 code points in a 21,500-point sweep — spacing marks (Mc),")
lines.append("//! format characters (Cf), modifier symbols (Sk) — which misaligned every")
lines.append("//! table, panel and wrap point containing them.")
lines.append("")
lines.append("/// `(start, end, width)`, sorted and non-overlapping, from upstream's table.")
lines.append(f"pub(crate) static CELL_WIDTH_RANGES: [(u32, u32, u8); {len(widths)}] = [")
for start, end, width in widths:
    lines.append(f"    (0x{start:04X}, 0x{end:04X}, {width}),")
lines.append("];")
lines.append("")
lines.append("/// Characters that are one cell alone but two when followed by U+FE0F")
lines.append("/// (variation selector 16). Upstream's `narrow_to_wide`.")
lines.append(f"pub(crate) static NARROW_TO_WIDE: [char; {len(narrow_to_wide)}] = [")
for ch in narrow_to_wide:
    lines.append(f"    '\\u{{{ord(ch):04X}}}',")
lines.append("];")
lines.append("")

out.write_text("\n".join(lines), encoding="utf-8")
print(f"wrote {out.name}: {len(widths)} ranges, {len(narrow_to_wide)} narrow_to_wide, "
      f"unicode {table.unicode_version}")
