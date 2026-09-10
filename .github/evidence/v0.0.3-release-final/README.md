# 0.0.3 final review: real CLI threshold output

Captured 2026-09-10 with redirected stdout, `NO_COLOR` unset,
`TERM=xterm-256color`, `COLUMNS=80`, `LINES=36`.

```bash
rich --diff crates/rich-art/tests/fixtures/halo-before.png \
  crates/rich-art/tests/fixtures/halo-after.png --width 64 --threshold 5.39 \
  --export-svg threshold-after.svg --export-html threshold-after.html \
  > threshold-after.txt 2> threshold-after.stderr
```

The before binary was locally installed from the prior reviewed RC runtime.
It exits 1 and prints `FAIL 5.4% changed, limit 5.4%`.
The after binary includes the final review fix, exits 0 and prints
`OK 5.4% changed, within 5.4%`. Both exports come directly from the CLI.
The comparison now uses the formatter's one-decimal rounding for both values.
A regression also proves that threshold 5.34 still fails with a printed 5.3% limit,
and unit coverage includes formatter ties at 0.25 and 0.75.

Workspace tests and Clippy pass. All 22 release regressions pass, including
real Git repositories with annotated, lightweight, mismatched and unmerged tags.
The strict docs build passes. Independent review covered the threshold fix,
tag-object enforcement and verification-only release recovery.
