"""Make the world map that `rich_ext::chart` draws (`CanvasPainter::map`).

The coastlines are Natural Earth's (https://www.naturalearthdata.com), which
are in the public domain: "All versions of Natural Earth raster + vector map
data found on this website are in the public domain ... No permission is
needed to use Natural Earth. Crediting the authors is unnecessary." We credit
it anyway, here and in the docs of `crates/rich-ext/src/chart/world.rs`:
made with Natural Earth.

Sources, from Natural Earth's own repository at a pinned release (GeoJSON
files with the same lines as the shapefiles on naturalearthdata.com):

- the 1:110m coastlines (`ne_110m_coastline`) for `MapResolution::Low`;
- the 1:50m coastlines (`ne_50m_coastline`) for `MapResolution::High`.

Each set of lines is simplified (Ramer-Douglas-Peucker, in degrees) to about
a budget of points, lines smaller than the tolerance are dropped, and what is
left is rounded to a hundredth of a degree. The output is little-endian
`i16` pairs, longitude then latitude, in hundredths of a degree, with the
pair (-32768, -32768) between two lines. It goes to
`crates/rich-ext/src/chart/world_110m.bin` and `world_50m.bin`, which the
crate includes with `include_bytes!`.

The binaries are checked in and this script is not run in CI. Run it again
to change the budgets or the Natural Earth release:

    python3 scripts/world_map.py                 # download, then write the binaries
    python3 scripts/world_map.py --input DIR     # read DIR/ne_110m_coastline.geojson and 50m
    python3 scripts/world_map.py --check         # fail if the binaries would change
"""

import argparse
import json
import math
import pathlib
import struct
import sys
import urllib.request

ROOT = pathlib.Path(__file__).resolve().parent.parent
OUT = ROOT / "crates" / "rich-ext" / "src" / "chart"
RELEASE = "v5.1.2"
URL = ("https://raw.githubusercontent.com/nvkelso/natural-earth-vector/"
       f"{RELEASE}/geojson/{{name}}.geojson")
# (source, output, point budget). The budgets keep the binaries small: they
# ship in the crate's package.
MAPS = [
    ("ne_110m_coastline", "world_110m.bin", 1200),
    ("ne_50m_coastline", "world_50m.bin", 5000),
]
BREAK = -32768


def load(name, input_dir):
    """The lines of a GeoJSON coastline file, as lists of (lon, lat)."""
    if input_dir:
        data = json.loads((pathlib.Path(input_dir) / f"{name}.geojson").read_text("utf-8"))
    else:
        url = URL.format(name=name)
        print(f"  {url}", file=sys.stderr)
        with urllib.request.urlopen(url, timeout=120) as response:
            data = json.load(response)
    lines = []
    for feature in data["features"]:
        geometry = feature["geometry"]
        if geometry["type"] == "LineString":
            parts = [geometry["coordinates"]]
        elif geometry["type"] == "MultiLineString":
            parts = geometry["coordinates"]
        else:
            continue
        for part in parts:
            line = [(float(p[0]), float(p[1])) for p in part]
            if len(line) >= 2:
                lines.append(line)
    return lines


def distance(p, a, b):
    """The distance from `p` to the segment from `a` to `b`."""
    (px, py), (ax, ay), (bx, by) = p, a, b
    dx, dy = bx - ax, by - ay
    length = dx * dx + dy * dy
    if length == 0.0:
        return math.hypot(px - ax, py - ay)
    t = max(0.0, min(1.0, ((px - ax) * dx + (py - ay) * dy) / length))
    return math.hypot(px - (ax + t * dx), py - (ay + t * dy))


def importance(line):
    """For each point, the largest tolerance at which Ramer-Douglas-Peucker
    keeps it: the ends always, a point between them while its distance and
    every enclosing split's are above the tolerance. Keeping the points
    above a tolerance is then the simplification at that tolerance."""
    keep = [0.0] * len(line)
    keep[0] = keep[-1] = math.inf
    stack = [(0, len(line) - 1, math.inf)]
    while stack:
        first, last, ceiling = stack.pop()
        if last - first < 2:
            continue
        far, best = first + 1, -1.0
        for i in range(first + 1, last):
            d = distance(line[i], line[first], line[last])
            if d > best:
                far, best = i, d
        keep[far] = min(best, ceiling)
        stack.append((first, far, keep[far]))
        stack.append((far, last, keep[far]))
    return keep


def extent(line):
    xs = [p[0] for p in line]
    ys = [p[1] for p in line]
    return max(max(xs) - min(xs), max(ys) - min(ys))


def simplify(lines, tolerance, ranks):
    """The lines at `tolerance`, rounded to hundredths of a degree, without
    repeated points; lines smaller than the tolerance are dropped."""
    out = []
    for line, keep, size in ranks:
        if size < tolerance:
            continue
        kept = []
        for (x, y), rank in zip(line, keep):
            if rank > tolerance:
                q = (max(-18000, min(18000, round(x * 100))), max(-9000, min(9000, round(y * 100))))
                if not kept or kept[-1] != q:
                    kept.append(q)
        if len(kept) >= 2:
            out.append(kept)
    return out


def fit(lines, budget):
    """The simplification with about `budget` points, by bisecting the
    tolerance."""
    ranks = [(line, importance(line), extent(line)) for line in lines]
    low, high = 0.0, 10.0
    for _ in range(60):
        middle = (low + high) / 2
        if sum(len(line) for line in simplify(lines, middle, ranks)) > budget:
            low = middle
        else:
            high = middle
    return simplify(lines, high, ranks), high


def encode(lines):
    out = bytearray()
    for i, line in enumerate(lines):
        if i:
            out += struct.pack("<hh", BREAK, BREAK)
        for x, y in line:
            out += struct.pack("<hh", x, y)
    return bytes(out)


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--input", help="read the GeoJSON files from this directory")
    parser.add_argument("--check", action="store_true",
                        help="fail if a checked-in binary differs from what would be written")
    args = parser.parse_args()
    stale = []
    for name, output, budget in MAPS:
        lines, tolerance = fit(load(name, args.input), budget)
        data = encode(lines)
        points = sum(len(line) for line in lines)
        print(f"{output}: {len(lines)} lines, {points} points, {len(data)} bytes "
              f"(tolerance {tolerance:.4f} degrees)")
        path = OUT / output
        if args.check:
            if not path.exists() or path.read_bytes() != data:
                stale.append(output)
        else:
            path.write_bytes(data)
    if stale:
        print("stale: " + ", ".join(stale) + "; run python3 scripts/world_map.py", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
