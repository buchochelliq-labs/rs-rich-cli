# 0.0.3 runtime audit

Follow-up to release readiness, covering PR #98 and issues #59, #72 and #74.
Rendering reference: Python rich 15.0.0. CLI semantics reference: rich-cli 1.8.1.

| Finding | Disposition and verification |
|---|---|
| #98 escape boundaries lose suffix bytes | Recalculate from each actual break. Feature tests preserve every ASCII byte at widths 1–20 and reject partial cropped escapes. |
| #98 unconditional core divergence | Off-default `json-escape-safe` feature. Library requires `.escape_safe(true)`; default goldens are real Python output. CLI feature opts in. |
| #59 / #72 piped GIF animation | One first frame, no delay/control codes, even `--loop 0`; CLI diagnostic. Unit and process-timeout tests cover single/composed frames. |
| #72 ignored GIF flags | Reject decorators/alignment/pager and exports for explicit or inferred GIF paths; mode conflict names include GIF/diff. |
| #72 image auto downgrade | Emit stderr diagnostic when auto resolves to ASCII; explicit fallbacks retain their diagnostics. |
| #72 notebook outputs | Streams remain unlabelled; ignore display_data like pinned upstream (including PNG), without a spurious Out label. Adding image placeholders would be an extension. |
| #72 syntax and notebook tabs | Existing four-space syntax expansion retained; existing syntax tests and CLI suite pass. |
| #72 JSON/CSV/syntax alignment | Existing alignment works; tests exercise all three. Multiple flags now follow upstream left > right > center priority independent of order. |
| #72 rule truncation/decorators | Existing ellipsis and decorator behavior retained; refusing rule decorators would break functional flags. |
| #72 nested lists, strike, code-only list item | Existing fixes retained. Code-only item starts with a padding row; regression matches pinned Python. |
| #72 missing pager | Existing unpaged fallback matches pinned upstream. No new diagnostic divergence introduced. |
| #74 JSON integers / exponents | Preserve all integer digits; normalize -0; overflowing positive/negative exponent values render signed Infinity. Python golden and malformed-number tests. |
| #74 JSON depth / NaN | Existing iterative parser accepts deep inputs and non-finite literals; retained tests. |
| #74 CSV memory | Stream styled rows for undecorated CSV; retain source rows for measurement. Decorators, alignment, paging and exports still buffer. See measured evidence below. |
| #74 emoji widths, code padding, nested padding | Existing grapheme-aware widths and container/code padding retained; core tests and goldens pass. |
| #74 Markdown images / quoted rules / HTML / empty files | Adjacent same-cell images share a row; quoted-rule and ignored-HTML spacing match Python; empty Markdown emits zero bytes. Seven new Python goldens. |
| #74 narrow panels | Widths 2–4 already omit content. Pinned upstream ForceWidth also permits two corners at requested width 1 in a wider console; retained parity. |
| #74 docs | Shipped perceptual example says 5.4%; redirected Sixel fallback correctly described as ASCII. |
| #74 long-line performance note | No correctness defect claimed; wrapping optimization remains separate work. |

The same-host debug-build benchmark used 10 columns, 10,000 rows, a 1,300,080-byte
file, COLUMNS=100 and stdout redirected to /dev/null. Peak RSS fell from 236,008 KiB
to 24,656 KiB; elapsed time from 5.31 s to 2.27 s. These are measurements of this
fixture, not a constant-memory guarantee. Source rows still scale with input.
See the repository's `.github/evidence/v0.0.3-runtime/` for commands, outputs and
actual CLI screenshots.
