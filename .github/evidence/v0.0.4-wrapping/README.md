# 0.0.4 Text wrapping verification

The optimization converts ordered character break positions to UTF-8 byte
positions by scanning the next slice, instead of scanning each full prefix.
Wrapping, cropping, grapheme boundaries and style spans keep their existing
semantics.

`before.json` measures the published 0.0.3 binary at
`d9bfe617e835d7403744d54726e24f21784a7faa`. `after.json` measures the optimized
build. Both use the same release profile, host and harness with one warmup and
five measured runs, width 100, no color and redirected output. Timed-out
warmups have no median or output hash. Completed before/after cases must retain
identical output hashes. The 5 MiB cases finish after the change; before the
change their 10-second warmup times out, so no speedup ratio is claimed for them.

The legacy `wrap-ascii-*`, `wrap-words-5m` and `wrap-unicode` fixtures use `.txt`
files. Those select plain Syntax, not Text. New extensionless `text-*` cases
exercise Text directly. Original case names and baseline data are retained.

Reproduce the acceptance timings (repeat with both binaries):

```bash
python3 scripts/bench_v004.py --binary target/release/rich \
  --revision "$(git rev-parse HEAD)" --output after.json \
  --case wrap-markdown-1m --case wrap-markdown-5m \
  --case text-ascii-65536 --case text-ascii-262144 \
  --case text-ascii-1048576 --case text-ascii-5242880 \
  --case text-words-5m --case text-unicode
```

`compare.py` checks 192 fixture/width/decoration combinations against 0.0.3:
ASCII, multibyte and combining Unicode, emoji, whitespace and tabs, zero-width
characters, empty input and Markdown; widths 1, 7, 20 and 80. It asserts exact
stdout, stderr, exit status, HTML and SVG equality. Exports exercise colored
segments even with stdout redirected. `comparisons.json` retains their hashes.

```bash
python3 .github/evidence/v0.0.4-wrapping/compare.py \
  --baseline /path/to/0.0.3/rich --binary target/release/rich \
  --output comparisons.json
```

Two additional fixtures in `scripts/capture_golden.py` / `renderables.tsv` were
captured from real Rich 15.0.0. They check repeated styled multibyte folds, word
breaks and multiple hard lines. Capture with `TERM=xterm-256color` and unset
`NO_COLOR`: Rich treats `TERM=dumb` as an 80-column terminal even when an
explicit width is supplied without a height. Existing golden bytes are unchanged.

`demo.md`, `stdout.txt` and `demo.svg` show actual CLI rendering:

```bash
rich --markdown .github/evidence/v0.0.4-wrapping/demo.md --panel rounded \
  --width 64 --export-svg .github/evidence/v0.0.4-wrapping/demo.svg
```

Independent root review verified ordered break offsets, per-hard-line resets and
UTF-8 boundaries. The combined CSV/wrapping tree passes fmt, Clippy, workspace
tests and a strict docs build.
