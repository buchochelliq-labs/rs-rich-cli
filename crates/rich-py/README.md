# rs-rich (Python)

Rich-compatible terminal rendering for Python, backed by
[rs-rich](https://github.com/buchochelliq-labs/rs-rich-cli), a Rust port of
[Rich](https://github.com/Textualize/rich).

```bash
pip install rs-rich
```

Move a Rich program over by changing its imports:

```python
from rs_rich.console import Console   # was: from rich.console import Console
from rs_rich.table import Table       # was: from rich.table import Table

table = Table(title="Star Wars Movies")
table.add_column("Released", justify="right", style="cyan", no_wrap=True)
table.add_column("Title", style="magenta")
table.add_row("Dec 20, 2019", "Star Wars: The Rise of Skywalker")

console = Console()
console.print(table)
```

All rendering happens in Rust. The Python package only maps Rich's classes
and arguments onto the Rust ones, so output matches Rich 15.0.0 byte for
byte on the supported surface.

**This is a first slice (0.0.1).** Anything not listed below is not
implemented yet, and raises `NotImplementedError` or `TypeError` rather than
rendering something different.

## Supported

| Module | Supported |
|---|---|
| `rs_rich` | `print(*objects, sep=" ")`, `get_console()` |
| `rs_rich.console` | `Console(file=, width=, height=, color_system=, force_terminal=, no_color=, record=, highlight=, emoji=, safe_box=)`; `print(*objects, sep=" ", justify=None)`, `rule(title, characters=, style=)`, `export_text(clear=, styles=)`; `width`, `height`, `is_terminal`, `color_system` |
| `rs_rich.text` | `Text(text, style, justify=, overflow=, no_wrap=)`, `Text.from_markup`, `append`, `stylize`, `plain`, `len()` |
| `rs_rich.style` | `Style(color=, bgcolor=, bold=, dim=, italic=, underline=, blink=, reverse=, conceal=, strike=, link=)`, `Style.parse`, `+`, `==` |
| `rs_rich.table` | `Table(*headers, title=, caption=, box=, show_header=, show_lines=, show_edge=, expand=, border_style=)`; `add_column(header, style=, header_style=, justify=, overflow=, width=, min_width=, max_width=, ratio=, no_wrap=)`, `add_row(*cells)` with `str` or `Text` cells, `row_count` |
| `rs_rich.panel` | `Panel(renderable, box, title=, title_align=, subtitle=, subtitle_align=, expand=, border_style=, width=, padding=)`, `Panel.fit` |
| `rs_rich.box` | Rich's box constants |
| `rs_rich.markup` | `escape` |
| `rs_rich.errors` | `ConsoleError`, `MarkupError`, `StyleSyntaxError` |

`Console.print` takes `str` (console markup), numbers, `None`, `Text`,
`Table` and `Panel`. A `Panel` holds any of those renderables.

## Known differences

- `Console.print` supports `end="\n"` only, `justify=` for strings only, and no `style=` argument.
- Consecutive `str` arguments are joined with `sep` and then read as one
  piece of markup, so a tag may span arguments. Rich reads each separately.
- Table cells are `str` or `Text`; renderables inside cells come later.
- `Console(width=None)` writing to a file that is not a terminal is
  `COLUMNS` columns wide, else 80, as in Rich. On a terminal the width comes
  from the process's terminal.
- Hyperlinks carry no `id=`; Rich's ids are random.

## Versions

The package has its own version (0.0.1) and is released from `python-v…`
tags. It bundles the rs-rich Rust crate from the same commit.
