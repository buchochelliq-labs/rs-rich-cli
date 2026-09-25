# rs-rich-lumis

The [lumis](https://lumis.sh) syntax highlighter for
[`rs-rich`](https://crates.io/crates/rs-rich), the Rust port of Python's `rich`:
tree-sitter grammars for parsing and Neovim themes for colour.

`LumisHighlighter` implements `rich`'s `CodeHighlighter`, so it can replace the
built-in syntect highlighter anywhere `rich` highlights code:

```rust
use rich::syntax::Syntax;
use rich_lumis::LumisHighlighter;

let code = Syntax::new("fn main() {}", "rust")
    .highlighter(LumisHighlighter::shared())
    .theme("dracula");
```

- **Themes** are lumis's own names (`monokai` by default, `dracula`,
  `github_light`, `onedark`, …), plus `ansi_dark` and `ansi_light`: upstream
  rich's ANSI themes in the terminal's 16 colours.
- **Languages** are lumis's: over 100 with default features. For a smaller
  build, use `default-features = false` and one of the bundles: `bundle-web`,
  `bundle-web-extra`, `bundle-system` or `bundle-backend`.
- **As a plugin**, `LumisPlugin` registers it as the code highlighter `"lumis"`
  through [`rs-rich-plugin-api`](https://crates.io/crates/rs-rich-plugin-api).

It is its own crate because lumis links tree-sitter's C runtime and its
grammars. Only builds that ask for it pay for that. Requires Rust 1.91 (lumis's
minimum).
