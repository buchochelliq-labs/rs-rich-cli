# CSV memory evidence

`after.json` holds five-sample measurements after owned-cell/collection transfer
and completed-row compaction. The source is GIF commit de7feead plus the CSV
changes in this PR; its exact binary SHA is in the JSON. Later GIF review changes
do not touch CSV behavior. Compare with the original 0.0.3 baseline in
`../v0.0.4-baseline/baseline.json`; all measured CSV output hashes match.

`paired-before.json` / `paired-after.json` are consecutive same-host runs of the
100k case to distinguish host timing variability from the memory result.
`comparisons.json` records 120 matching baseline/development output combinations,
including HTML and SVG exports. Memory is reduced, not bounded; decoration buffers
remain. See docs/benchmarks.md for method, values and limitations.

Independent sub-agent review found no correctness/parity blockers. Local
formatting, Clippy and workspace tests cover the new ownership seam and the CLI.
A fresh target directory was used after old cached Rust artifacts failed to link.

Reproduce behavioral comparisons with:

```bash
python3 .github/evidence/v0.0.4-csv/compare.py --baseline ./rich-0.0.3 \
  --binary ./rich-development --output comparisons.json
```

`measurements.csv` contains the recorded figures; `measurements.svg` is the
actual CLI rendering of that file. Reproduce with:

```bash
rich --csv .github/evidence/v0.0.4-csv/measurements.csv --width 64 \
  --title 'CSV memory measurements' --caption 'Real samples · KiB peak RSS' \
  --export-svg measurements.svg
```
