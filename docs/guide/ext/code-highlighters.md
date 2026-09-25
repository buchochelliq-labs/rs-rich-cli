# Code highlighters

`Syntax`, Markdown code blocks, `source_view` and the diff views all highlight
code through one extension point, core's `CodeHighlighter`. Two adapters ship
with the port, and you can write your own in about 40 lines.

The examples come from
[`guide_highlighters.rs`](https://github.com/buchochelliq-labs/rs-rich-cli/blob/main/crates/rich-ext/examples/guide_highlighters.rs):

```bash
cargo run -p rs-rich-ext --example guide_highlighters --features testing
```

## Choose one

| | syntect (default) | lumis (`rs-rich-lumis`) |
|---|---|---|
| Parsing | TextMate grammars, regex-based | tree-sitter grammars |
| Code | Pure Rust by default; the `onig` feature swaps in the Oniguruma C library | C (tree-sitter's runtime and grammars) |
| Languages | syntect's default set | 116 with default features; bundles select fewer |
| Themes | 7 syntect themes, plus `ansi_dark` and `ansi_light` | 256 Neovim themes (default `monokai`), plus `ansi_dark` and `ansi_light` |
| Speed | 2–4× faster with `onig` | a fixed cost per language, then the fastest on long files ([benchmarks](../../benchmarks.md#0012-code-highlighters)) |
| Binary | about 15.7 MB for the CLI | about 169 MB for the CLI with every language; a bundle is far smaller |
| Minimum Rust | the workspace's (1.90) | 1.91 |

syntect is the default everywhere and is what upstream parity is measured
with. Choose lumis for tree-sitter's more precise parsing, its languages or its
themes, and accept the larger build.

!!! warning "One tree-sitter per binary"
    lumis links tree-sitter's C runtime. Another crate that links a different
    version of that runtime (another tree-sitter highlighter, for example) cannot
    go into the same binary. That is why lumis lives in its own crate and is
    behind an off-by-default feature in the CLI.

In the CLI, choose with `--highlighter NAME` and `--code-theme NAME`, or the
`highlighter` and `code_theme` keys in `rich.toml`. `rich doctor` lists the
highlighters compiled in, their themes, and the one in use.

## Use one

Give a highlighter to one `Syntax`, or make it the default for every block a
console renders: register it, choose it by name and install the registry.

```rust
--8<-- "crates/rich-ext/examples/guide_highlighters.rs:use"
```

A `Syntax` with a highlighter of its own always uses it; the console's default
covers everything else, including Markdown code blocks. Theme names are each
highlighter's own.

## Write your own

An adapter implements four methods. `highlight` returns one line per element
of `code.split('\n')`, each with styled byte ranges: sorted, not overlapping,
inside the line and on character boundaries. An unknown language is plain
text, and an unknown theme is an error. This one only bolds (or colours) a few
Rust keywords:

```rust
--8<-- "crates/rich-ext/examples/guide_highlighters.rs:adapter"
```

Core checks what an adapter returns against the source. It drops bad spans,
renders missing lines unstyled, strips hyperlinks from styles and takes the
text from the source, so an adapter can change colours but never the text.
That is a safety net, not a licence: the conformance kit holds adapters to the
contract.

## Check it with the conformance kit

`rich_ext::testing::conformance` (the `testing` feature) runs the checks both
shipped adapters pass in CI:

- the default theme is listed and highlights (with `ansi_dark`, if listed),
  and an unknown theme is `UnknownTheme`;
- line counts match `split('\n')` for empty input, a missing final newline,
  CRLF, tabs and multi-byte text;
- spans are sorted, disjoint, in range and on character boundaries;
- an unknown language renders exactly as no language;
- rendered output carries no control characters beyond styling;
- 10,000 lines cost at most 40× what 1,000 lines do, a relative budget rather
  than a wall-clock one.

```rust
--8<-- "crates/rich-ext/examples/guide_highlighters.rs:conformance"
```

As a test:

```rust
--8<-- "crates/rich-ext/examples/guide_highlighters.rs:test"
```

`check_with(…, Options { all_themes: true, .. })` also runs every theme the
adapter lists. A failure lists every problem with the input that caused it.

## The ANSI themes

`ansi_dark` and `ansi_light` are upstream rich's `ANSI_DARK` and `ANSI_LIGHT`:
the terminal's own 16 colours, with no background, so highlighted code follows
the user's terminal palette. Both shipped adapters map their token names onto
upstream's Pygments token types (see
[Divergences #18](https://github.com/buchochelliq-labs/rs-rich-cli/blob/main/docs/DIVERGENCES.md)).

```rust
--8<-- "crates/rich-ext/examples/guide_highlighters.rs:ansi"
```
