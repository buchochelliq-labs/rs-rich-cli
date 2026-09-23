# rs-rich-macros

Compile-time checked markup and derive macros for
[`rs-rich`](https://crates.io/crates/rs-rich), the Rust port of Python `rich`.

Use them through [`rs-rich-ext`](https://crates.io/crates/rs-rich-ext) with its
`macros` feature, which re-exports them and provides the runtime they expand to:

```toml
rs-rich-ext = { version = "0.0.9", features = ["macros"] }
```

- `richf!("[bold]{name}[/] has {count:>3} items")` builds a `Text`. Unbalanced or
  unclosed tags, unknown style names and unknown theme keys are compile errors.
  Interpolated values are escaped, so user data is never read as markup.
- `style!("bold red on white")` is a compile-time checked `Style`.
- `theme_key!("repr.number")` is a compile-time checked theme key.
- `markup!("[green]ok[/]")` checks a markup literal and returns it as `&str`.
- `#[derive(Rich)]` renders a struct or enum as labelled fields, a panel or a
  table row, with `#[rich(skip, label = "…", style = "…", display, format =
  "…", justify = "…", order = N)]` on fields and `#[rich(title = "…", panel,
  table)]` on the type.

Independent SemVer from `0.0.1`; see the repository's `AGENTS.md`.
