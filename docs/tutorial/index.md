# Tutorial

Six short chapters that build from a single styled line to a live progress
display and the command-line tool.

Every code block is real, compiled code — the examples are checked against the
library, and the images are rendered by it.

<div class="grid cards" markdown>

- **[1. Your first output](01-hello.md)** — the `Console`, and why it behaves
  differently when piped.
- **[2. Markup and style](02-markup.md)** — tags, themes, and automatic
  highlighting.
- **[3. Tables](03-tables.md)** — columns, justification, wrapping and overflow.
- **[4. Layout](04-layout.md)** — panels, trees, columns, rules, alignment.
- **[5. Progress and live output](05-live.md)** — bars, spinners, refreshing
  in place.
- **[6. The CLI](06-cli.md)** — rendering files, and exporting HTML/SVG.

</div>

## Following along

```bash
cargo new rich-tutorial && cd rich-tutorial
cargo add rs-rich
```

Remember: the package is `rs-rich`, the `use` line is `rich`.
