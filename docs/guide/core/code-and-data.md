# Code and data

Four renderables turn source text and values into readable terminal output:

| Type | Renders | Upstream |
|---|---|---|
| [`Syntax`](#syntax) | source code, highlighted | `rich.syntax.Syntax` |
| [`Markdown`](#markdown) | a CommonMark document | `rich.markdown.Markdown` |
| [`Json`](#json) | a JSON document, indented and coloured | `rich.json.JSON` |
| [`Pretty`](#pretty) | any `Debug` value, highlighted | `rich.pretty.Pretty` |

The examples use these imports:

```rust
--8<-- "crates/rich/examples/guide_code.rs:imports"
```

## Syntax

<div class="rich-playground-pair" markdown>

```rust
--8<-- "crates/rich/examples/guide_code.rs:syntax"
```

```text { .rich-playground data-kind="syntax" data-lang="rust" data-width="64" }
use std::collections::HashMap;

/// Count words, ignoring case.
fn count(text: &str) -> HashMap<String, usize> {
	let mut counts = HashMap::new();
	for word in text.split_whitespace() {
		*counts.entry(word.to_lowercase()).or_insert(0) += 1;
	}
	counts
}
```

</div>

![Rust source highlighted with the default theme](../../media/guide/guide_code-syntax.svg)

`Syntax::new(code, language)` takes a language name or file extension
(`"rust"`, `"rs"`, `"py"`, `"json"`, `"toml"`, `"sh"` …). An empty or unknown
language renders the code as plain text on the theme background.

<div class="rich-playground-pair" markdown>

```rust
--8<-- "crates/rich/examples/guide_code.rs:syntax_options"
```

```text { .rich-playground data-kind="syntax" data-lang="rs" data-theme="InspiredGitHub" data-padding="1" data-tab-size="2" data-word-wrap="true" data-width="48" }
let answer = compute_the_answer(life, universe, everything); // 42
	indented_with_a_tab();
```

</div>

![Word-wrapped code with padding on a light theme, and cropped code on Solarized](../../media/guide/guide_code-syntax-options.svg)

| Method | Default | Effect |
|---|---|---|
| `theme(name)` | `"base16-ocean.dark"` | a `syntect` theme: `base16-ocean.dark`, `base16-eighties.dark`, `base16-mocha.dark`, `base16-ocean.light`, `InspiredGitHub`, `Solarized (dark)`, `Solarized (light)`; unknown names fall back to the default |
| `word_wrap(bool)` | `false` | wrap long lines; off, they are cropped at the width |
| `padding(n)` | `0` | `n` cells of background on every side |
| `tab_size(n)` | `4` | tabs are expanded to spaces before highlighting |

`highlight()` returns the highlighted code as a `Text` instead of a padded
block — for embedding in your own layout:

```rust
--8<-- "crates/rich/examples/guide_code.rs:highlight"
```

!!! note "Colours differ from Python rich"

    Upstream highlights with Pygments; this port uses
    [`syntect`](https://docs.rs/syntect), which ships different grammars and
    themes. The layout matches; the colours do not
    ([divergence #18](../../DIVERGENCES.md)). Pygments style names such as
    `monokai` are not recognised.

### Line numbers, ranges and guides

```rust
use rich::{Console, Syntax};

let code = "fn main() {\n    let x = 1;\n    println!(\"{x}\");\n}\n";
let syntax = Syntax::new(code, "rust")
    .line_numbers(true)
    .line_range(Some(2), Some(3)) // show lines 2 and 3 only (1-based)
    .highlight_lines([2])         // a pointer in the gutter at line 2
    .indent_guides(true);
Console::new().print(&syntax);
```

`start_line(n)` numbers the first line `n`, and `code_width`,
`background_color` and `padding_sides` follow upstream's
`code_width`, `background_color` and four-sided `padding`. Upstream's
`dedent` and `Syntax.from_path` are not ported: strip the indent and read
the file yourself.

## Markdown

<div class="rich-playground-pair" markdown>

```rust
--8<-- "crates/rich/examples/guide_code.rs:markdown"
```

````text { .rich-playground data-kind="markdown" data-width="64" }
# Release notes

Markdown renders **bold**, *italic*, `code` and [links](https://docs.rs/rs-rich).

- bullet lists
  - nested
1. and numbered ones

> Block quotes, too.

| Crate | Lib name |
|-------|---------:|
| rs-rich | `rich` |
| rs-rich-ext | `rich_ext` |

```rust
fn main() { println!("fenced code is highlighted"); }
```
````

</div>

![A Markdown document with a heading, inline styles, lists, a quote, a table and a code block](../../media/guide/guide_code-markdown.svg)

Headings, paragraphs, emphasis, inline code, links, bullet and numbered lists
(nested), block quotes, horizontal rules, tables with column alignment, and
fenced code blocks (highlighted by `Syntax`) are all rendered, as upstream
does.

<div class="rich-playground-pair" markdown>

```rust
--8<-- "crates/rich/examples/guide_code.rs:markdown_options"
```

```text { .rich-playground data-kind="markdown" data-hyperlinks="false" data-justify="full" data-style="grey85" data-code-theme="base16-ocean.dark" data-inline-code-lexer="rust" data-width="64" }
Some *text* with a [link](https://example.com).
```

</div>

| Method | Default | Effect |
|---|---|---|
| `hyperlinks(bool)` | `true` | `true`: link text becomes an OSC 8 hyperlink. `false`: written as `text (url)` |
| `justify(Justify)` | `Left` | paragraph justification (headings keep their own) |
| `style(Style)` | none | base style for all text |
| `code_theme(name)` | the `Syntax` default | theme for fenced code blocks |
| `inline_code_lexer(lang)` | none | highlight `inline code` as this language |
| `inline_code_theme(name)` | the code theme | theme for highlighted inline code |

!!! tip "Hyperlinks off for piped output"

    An OSC 8 link is only written when the console has colour. With hyperlinks
    on, output redirected to a file keeps the link text and loses the URL.
    That is why upstream's `rich` CLI turns them off by default; do the same
    when your output may be piped.

Markdown styles come from the console's theme: `markdown.h1`,
`markdown.code`, `markdown.link`, `markdown.block_quote` and so on, each
looked up when the document renders. A theme entry replaces the default
style, so `markdown.h1 = italic` gives an italic heading with no colour. See
[divergence #9](../../DIVERGENCES.md) for the few elements that differ.

## JSON

<div class="rich-playground-pair" markdown>

```rust
--8<-- "crates/rich/examples/guide_code.rs:json"
```

```text { .rich-playground data-kind="json" data-width="48" }
{"name": "rs-rich", "version": "0.0.7", "tags": ["terminal", "ansi"],
 "stable": false, "downloads": 1204, "license": null}
```

</div>

![A JSON document, indented with keys, strings, numbers, booleans and null coloured](../../media/guide/guide_code-json.svg)

- `Json::new(text)` parses and returns `Err` for invalid JSON. The output is
  re-indented (two spaces) with keys, strings, numbers, booleans and `null`
  styled through the theme (`json.key`, `json.str`, …).
- `.no_wrap(true)` crops long lines instead of wrapping them — what upstream
  does for a `JSON` nested inside another renderable. The default wraps, which
  matches a top-level print.
- The `json-escape-safe` feature adds `.escape_safe(true)`, which never splits
  an escape sequence at a line break ([divergence #22](../../DIVERGENCES.md)).

Upstream's `indent`, `sort_keys` and `ensure_ascii` options are not ported;
format with `serde_json` first if you need them.

## Pretty

Python rich pretty-prints any object via its `repr`. Rust has no runtime
reflection, so [`Pretty`](https://docs.rs/rs-rich/latest/rich/pretty/struct.Pretty.html)
uses the value's `Debug` implementation and runs the repr highlighter over it.

```rust
--8<-- "crates/rich/examples/guide_code.rs:pretty"
```

![A Debug struct pretty-printed and highlighted, and a compact vector](../../media/guide/guide_code-pretty.svg)

- `Pretty::new(&value)` formats with `{:#?}` (multi-line); `Pretty::compact`
  with `{:?}`.
- Numbers, strings, `None`, paths and URLs are coloured. Rust's lowercase
  `true`/`false` are left plain, since the highlighter targets Python's
  spelling ([divergence #19](../../DIVERGENCES.md)).
- Line breaking is `Debug`'s, not upstream's width-aware layout: `max_length`,
  `max_string`, `max_depth` and `indent_guides` are not available.

## See also

- [Text and style](text-and-style.md) — themes that restyle these renderables
- [Tables](tables.md) — `Syntax`, `Markdown`, `Json` and `Pretty` can sit in cells
- The CLI renders files with these types: [Using the CLI](../../cli.md)
- API: [`Syntax`](https://docs.rs/rs-rich/latest/rich/syntax/struct.Syntax.html) ·
  [`Markdown`](https://docs.rs/rs-rich/latest/rich/markdown/struct.Markdown.html) ·
  [`Json`](https://docs.rs/rs-rich/latest/rich/json/struct.Json.html) ·
  [`Pretty`](https://docs.rs/rs-rich/latest/rich/pretty/struct.Pretty.html)
