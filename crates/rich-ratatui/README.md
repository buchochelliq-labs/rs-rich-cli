# rs-rich-ratatui

[ratatui](https://ratatui.rs) interop for
[rs-rich](https://github.com/buchochelliq-labs/rs-rich-cli), the Rust port of
Python's `rich`, both ways. This crate is an rs-rich addition, not a port:
`rich` has no ratatui. It builds on core `rs-rich`'s public API and on
[`ratatui-core`](https://crates.io/crates/ratatui-core), the crate ratatui's
own types live in, so the `Buffer`, `Rect` and `Widget` here are the ones a
`ratatui` 0.30 app already uses.

- **rich in ratatui.** `RichWidget` is a ratatui `Widget` that draws any rich
  renderable (a `Table`, a `Panel`, `Markdown`, `Syntax`, a `Tree`, markup)
  into a ratatui `Buffer`, so a ratatui app can adopt rich's renderables one
  widget at a time.
- **ratatui in rich-interact.** `RatatuiComponent` (the `interact` feature)
  is an [`rs-rich-interact`](https://crates.io/crates/rs-rich-interact)
  component whose view is drawn by ratatui widgets, so an existing widget
  runs inside a rich-interact event loop, and under its headless driver in
  tests.
- **The conversions underneath.** `style` maps styles and colours both
  ways (`to_ratatui_style`, `to_rich_style`, `to_ratatui_color`,
  `to_rich_color`); `buffer` writes rich lines into a buffer
  (`lines_to_buffer`), reads a buffer or a rectangle of one back into rich
  lines (`buffer_to_lines`, `buffer_area_to_lines`), and walks a rectangle
  cell by cell without allocating (`cells`, with a `StyleCache` that
  converts each distinct style once).

```rust
use rich::Table;
use rich_ratatui::RichWidget;

let mut table = Table::new().title("Services");
table.add_column("service");
table.add_column("status");
table.add_row(&["api", "[green]ok[/]"]);
table.add_row(&["billing", "[bold red]down[/]"]);

terminal.draw(|frame| {
    frame.render_widget(RichWidget::new(&table), frame.area());
    // or markup: RichWidget::markup("[b]q[/] quit")
})?;
```

`examples/rich_in_ratatui.rs` draws a rich table and rich Markdown side by
side in a ratatui frame and prints it, with no terminal needed:

```bash
cargo run -p rs-rich-ratatui --example rich_in_ratatui
```

## What converts, and what is lost

Lossless: no colour, the terminal default (rich's `default` ⇄ ratatui's
`Reset`), the 16 named colours, the 256-colour palette, 24-bit RGB, and nine
attributes (bold, dim, italic, underline, blink, `blink2` as rapid blink,
reverse, `conceal` as hidden, strike), each on, explicitly off or unset. A
rich `Panel` drawn into a buffer and read back shows the same text and the
same SGR codes.

Lost:

- rich's colour **names** (`grey0` comes back as `color(16)`; the colour is
  the same) and its legacy Windows colours (back as the standard colour with
  the same number);
- rich's `underline2`, `frame`, `encircle` and `overline`, which ratatui has
  no modifier for;
- rich's hyperlinks and meta: a ratatui cell holds a symbol and a style;
- ratatui's underline colour, which rich has no equivalent for;
- in a buffer, `Reset` reads as unset, since every fresh cell is `Reset`.

rich and ratatui measure some characters (some emoji) differently. Each
segment is placed at the column rich measured for it, so a disagreement
cannot shift the rest of the line.

## Features

| Feature | Default | What it adds |
|---|---|---|
| `interact` | no | `RatatuiComponent`, through `rs-rich-interact` (and crossterm) |

Nothing else in the rs-rich workspace depends on ratatui. The default build
needs only `rs-rich` and `ratatui-core`.

Independent SemVer from 0.0.1; see the repository's `AGENTS.md`.
