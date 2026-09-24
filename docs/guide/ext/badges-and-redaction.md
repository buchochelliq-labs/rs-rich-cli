# Badges, size bars, formatters and redaction

Four small tools for status lines, reports and logs:

- **Badges** (`rich_ext::badge`): compact chips for statuses, labels, links
  and metadata. They still make sense without colour.
- **Size bars** (`rich_ext::size_bar`): a size against a total or a limit.
- **Formatters** (`rich_ext::format`): sizes, rates, durations, times,
  percentages and counts written the way people read them.
- **Redaction** (`rich_ext::redact`): masks secrets in strings, terminal
  output, rendered segments and exports.

The examples come from
[`guide_badges.rs`](https://github.com/buchochelliq-labs/rs-rich-cli/blob/main/crates/rich-ext/examples/guide_badges.rs).
None of these modules needs a feature:

```bash
cargo run -p rs-rich-ext --example guide_badges
```

The styles are theme keys (`badge.*`, `size_bar.*`). Pass
`rich_ext::extended_theme()` to the console builder to get them, or define
them in your own theme. Without them the same defaults are used.

## Badges

A `Badge` is one of four kinds:

| Constructor | Colour | Plain |
|---|---|---|
| `Badge::status(Status::Ok, "build")` | ` ✔ build ` on `badge.ok` | `[OK build]` |
| `Badge::status(Status::Error, "")` | ` ✖ error ` on `badge.error` | `[ERROR]` |
| `Badge::label("beta")` | ` beta ` on `badge.label` | `[beta]` |
| `Badge::link("docs", url)` | ` docs `, an OSC 8 link | `[docs <url>]` |
| `Badge::meta("version", "1.2.0")` | ` version ` ` 1.2.0 ` on `badge.key` and `badge.value` | `[version: 1.2.0]` |

`Badges` puts several in a row. When the row is too long it wraps between
badges, never inside one:

```rust
--8<-- "crates/rich-ext/examples/guide_badges.rs:badges"
```

![A row of badges](../../media/guide/guide_badges-badges.svg)

### Without colour

Colour is never the only signal. When the console has no colour system, or
colour is turned off (`NO_COLOR`, `no_color(true)`), each badge gets its
plain form. The text then carries the meaning:

```rust
--8<-- "crates/rich-ext/examples/guide_badges.rs:plain"
```

![The same badges without colour](../../media/guide/guide_badges-plain.svg)

- Status chips use the markers of [`a11y::Status`](accessibility.md#policies):
  a glyph with colour, an ASCII tag (`OK`, `WARN`, `ERROR`) without it or on
  an ASCII-only console. `.symbols(SymbolSet::Words)` gives `[ok: build]`.
- A link is an OSC 8 hyperlink wherever the console renders styles. This
  includes `NO_COLOR`, which removes only the colours. Where there are no
  styles at all, the URL is written after the text. `.show_url(true)` or
  `.show_url(false)` overrides this.
- `.style(..)` replaces a badge's theme key with another key
  (`"badge.warning"`) or with a style definition (`"black on magenta"`).
- `badge.plain()` and `AccessibleText` return the plain form, for logs and
  screen readers.

A `Badge` or `Badges` measures to its width, so it can go in a table cell
(`Cell::Renderable`). See the table below.

## Size bars

`SizeBar::new(used, total)` shows a part of a total, such as a file in a
bundle. `SizeBar::limit(used, limit)` shows a size that should stay under a
limit, such as a package against a registry's cap. It switches to the
`size_bar.high` style from 90% (`.warn_at(ratio)` changes that).

```rust
--8<-- "crates/rich-ext/examples/guide_badges.rs:sizes"
```

![Size bars against a limit and a total](../../media/guide/guide_badges-sizes.svg)

Going over a limit is never shown by colour alone:

- the part of the bar past the limit uses its own glyph, `▓`;
- the percentage is over 100%;
- the line ends with `over by …`.

On an ASCII-only console the bar is drawn with `#` (used), `.` (free) and
`!` (over). Sizes use `format::bytes`, or `format::bytes_binary` with
`.units(Units::Binary)`. The bar is 20 cells wide by default
(`.bar_width(n)`). It shrinks to 4 cells when the line does not fit. If even
that is too wide, the line is cut with an ellipsis rather than wrapped.
`.show_sizes(false)` and `.show_percent(false)` leave those parts out.

Bars and badges both work in table cells:

```rust
--8<-- "crates/rich-ext/examples/guide_badges.rs:table"
```

![A table with size bars and status badges](../../media/guide/guide_badges-table.svg)

## Formatters

`rich_ext::format` holds pure functions that return strings, so the results
work in any cell, label or log line. They use `.` as the decimal point, `,`
between thousands, English words and UTC.

```rust
--8<-- "crates/rich-ext/examples/guide_badges.rs:format"
```

![Each formatter and its output](../../media/guide/guide_badges-format.svg)

| Function | Gives |
|---|---|
| `bytes(n)` | Decimal units, as upstream's `filesize.decimal`: `1.5 MB` |
| `bytes_binary(n)` | Binary units: `1.5 MiB` |
| `rate(bytes_per_second)` | `2.4 MB/s`; negative or non-finite rates read `0 bytes/s` |
| `duration(d)` / `duration_with(d, ascii)` | Two largest units: `850µs`, `4.2s`, `3m 07s`, `2d 04h` (`us` when ASCII) |
| `clock(d)` | `H:MM:SS`, as progress columns show it |
| `relative(then, now)` | `just now`, `3 hours ago`, `in 2 days` |
| `timestamp(t)` | ISO 8601 UTC to the second |
| `percent(ratio, decimals)` | `42.5%`; non-finite reads `-` |
| `number(n)` | `1,234,567` |
| `compact(n)` | `1.2k`, `3.4M`, truncated so `1999` is never `2.0k` |

## Redaction

!!! warning "Experimental: check the output yourself"
    `rich_ext::redact` and `rich capture --redact` are **experimental**. The
    detectors, masks and API may change in any release.

    Redaction is best effort, not a guarantee. It only knows the secret shapes
    listed below, matches within one line, and cannot tell a secret from
    ordinary text when it looks like ordinary text. **Always read redacted
    output before you share, publish or store it.**

    If a secret gets through, or anything else does not work as expected,
    please [report a bug](https://github.com/buchochelliq-labs/rs-rich-cli/issues/new?template=bug_report.yml) with an example (with the real secret
    replaced).

A `Redactor` masks secrets before they reach the terminal, a log, an export
or a recording. `Redactor::secrets()` turns on the built-in detectors:

| Detector | Masks |
|---|---|
| `KeyValue` | The value in `key=value`, `key: value`, `"key": "value"` or `--key=value` when the key looks secret. A quoted value is masked up to its closing quote, spaces and commas included |
| `Bearer` | The token after `Bearer ` (12+ characters with a digit) |
| `TokenPrefix` | GitHub (`ghp_`, `gho_`, `ghu_`, `ghs_`, `ghr_`, `github_pat_`), GitLab (`glpat-`), Slack (`xox?-`), Stripe secret keys (`sk_live_`, `rk_test_`, …), npm (`npm_`) and `sk-` API keys |
| `AwsAccessKey` | `AKIA…` / `ASIA…` access key ids |
| `Jwt` | `eyJ….eyJ….…` JSON Web Tokens |
| `UrlCredentials` | The password in `scheme://user:password@host`, up to the last `@` before the host. A bare `user:password@host` without a scheme is not matched |

A key looks secret when it matches `redact::SECRET_KEYS` (`password`,
`secret`, `token`, `api_key`, `credential`, a whole-word `auth`, …). This is
the same list that `data::Redaction::secrets()` uses for structured
documents, so `author` is never masked. The set is kept small to avoid false
positives. It is pattern matching, so it can miss a secret that looks like
an ordinary word.

Add your own rules with `.pattern(regex)` or `.named_pattern(name, regex)`.
If the pattern has a group named `secret`, only that group is masked. The
mask is `********` by default. `.mask("[{kind}]")` writes the rule's name
instead, for example `[jwt]`. Matching fails closed: if a pattern gives up
at run time (too much backtracking), the rest of that line is masked.

```rust
--8<-- "crates/rich-ext/examples/guide_badges.rs:redact"
```

![A log panel with its secrets masked](../../media/guide/guide_badges-redact.svg)

### Where it applies

| Input | Method |
|---|---|
| A string or log line | `redact_str` |
| Text with ANSI escapes | `redact_ansi`: rules see the visible text, the escapes stay |
| A stream of chunks (a recording) | `redact_chunks`: a secret split across chunks is still found |
| Raw bytes from a pipe | `redact_byte_chunks`: a character split between reads stays whole, invalid bytes are kept |
| A command line | `redact_args`: also masks the value of a secret-named flag (`--token X`) |
| Rendered segments | `redact_segments`, or wrap a renderable in `Redacted` |
| A recording to export | `capture`, `export_text`, `export_html`, `export_html_classes`, `export_svg` |

In segments, a secret can span several segments with different styles.
Rules match on each line's text. Each part of the mask keeps the style of
the cells it covers, and the line keeps its width, so borders and columns
stay aligned. To get the same in strings, use `.preserve_width(true)`.

Hidden text is searched too. With ANSI text, that is the body of each
escape string: an OSC 8 hyperlink's URL, a window title, DCS and APC
payloads. With segments, it is each style's link target. There the plain
mask is used, with any control characters in it turned into `*`, so the
escape stays well formed.

The export helpers record what the closure prints, redact it, and export
it. The secret never reaches the file:

```rust
--8<-- "crates/rich-ext/examples/guide_badges.rs:export"
```

For another terminal theme, call `console.record_output(f)`, then
`redactor.redact_segments(..)`, then the core `rich::export` or `rich::svg`
function.

!!! warning "What redaction cannot see"
    Rules match within one line. If a renderable wraps a long secret onto
    two lines, neither half matches. When that can happen, redact the input
    with `redact_str` before you render it. Masks that keep their width also
    reveal the secret's length. A mask inside a binary escape payload, such
    as an inline image, can spoil the image. Eight-bit C1 controls are read
    as text, not as escapes.

### From the command line

`rich capture` takes the same detectors. Masking happens before the output
is shown, exported or recorded:

```bash
rich capture --redact --export-svg run.svg --cast run.cast -- ./deploy.sh
rich capture --redact-pattern 'order (?P<secret>\d{4})' -- ./report.sh
```

See [Using the CLI](../../cli.md#viewers).
