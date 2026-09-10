# 0.0.3 UAT closeout

Reviewed 2026-09-10 against RC commit `2c24a878`, containing merged PR #110.
Issue states describe completed source work; registry publication is a separate
release step.

| Report | Disposition | Evidence or remaining scope |
|---|---|---|
| [#59](https://github.com/buchochelliq-labs/rs-rich-cli/issues/59) | Closed: completed | Redirected GIF/Stage output emits one frame without animation controls or waiting, including infinite repeats. |
| [#60](https://github.com/buchochelliq-labs/rs-rich-cli/issues/60) | Closed: completed | MANPAGER/PAGER precedence and native Windows `more.com` launch/output tests pass. |
| [#72](https://github.com/buchochelliq-labs/rs-rich-cli/issues/72) | Closed: completed | Tabs, applicable alignment, GIF validation/output, notebook stream labels, rule truncation, JSON and Markdown regressions are covered. Notebook display-data omission and pager fallback match upstream. |
| [#65](https://github.com/buchochelliq-labs/rs-rich-cli/issues/65) | Open: GIF block rendering | The correctness and documentation findings are resolved or verified upstream behavior; GIF half-block rendering remains deferred. |
| [#74](https://github.com/buchochelliq-labs/rs-rich-cli/issues/74) | Open: further performance work | Correctness findings are resolved. Source rows remain buffered for CSV measurement; decorated/aligned/paged/exported output also buffers. Further CSV memory and long-line optimization are deferred. |
| [#62](https://github.com/buchochelliq-labs/rs-rich-cli/issues/62) | Open: diagnostic/encoding polish | Awkward image-extension punctuation, low-level error before image hint, and UTF-16 diagnostics/support remain deferred. |
| [PR #99](https://github.com/buchochelliq-labs/rs-rich-cli/pull/99) | Closed: superseded | PR #107 already introduced tested TOML parsing through `read_upstream_version.py`. |

## Verification

All required PR #110 checks passed, including native Windows paging, default and
feature builds, MSRV and golden parity. Independent review compared eight CLI
Markdown cases byte-for-byte with isolated Python Rich 15.0.0: nested lists,
quoted rule, HTML followed by a paragraph, empty Markdown, code-only list item,
image marker, strikethrough and fenced-code padding.

The same-host CSV sample improved from 236,008 to 24,656 KiB peak RSS and 5.31 to
2.27 seconds for a 10,000-row/1,300,080-byte input. This is a measured improvement,
not a constant-memory guarantee. See the [runtime audit](runtime-audit-0.0.3.md).

The [release notes](releases/0.0.3.md) include actual CLI screenshots.
[Known issues](known-issues.md) records remaining limitations and upstream
behavior that is deliberately retained.
