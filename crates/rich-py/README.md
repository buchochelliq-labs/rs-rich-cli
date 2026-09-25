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

**Not everything is bound yet.** Anything not listed below raises
`NotImplementedError` or `TypeError` rather than rendering something
different.

## Supported

| Module | Supported |
|---|---|
| `rs_rich` | `print`, `get_console`, `print_json`, `reconfigure` |
| `rs_rich.console` | `Console` with Rich's constructor; `print` (every argument), `log`, `out`, `rule`, `line`, `input`, `print_json`, `capture`, `measure`, `render`, `render_lines`, `render_str`, `get_style`, themes, `export_text`/`export_html`/`export_svg` and `save_*`; `ConsoleOptions` |
| Your own classes | `__rich__`, `__rich_console__`, `__rich_measure__`, anywhere a renderable goes |
| `rs_rich.segment`, `rs_rich.measure` | `Segment`, `Measurement` |
| `rs_rich.text` | `Text(text, style, justify=, overflow=, no_wrap=)`, `Text.from_markup`, `append`, `stylize`, `plain`, `len()` |
| `rs_rich.style` | `Style(color=, bgcolor=, bold=, dim=, italic=, underline=, blink=, reverse=, conceal=, strike=, link=)`, `Style.parse`, `+`, `==` |
| `rs_rich.theme`, `rs_rich.terminal_theme` | `Theme`, `TerminalTheme` and Rich's palettes |
| `rs_rich.table` | `Table(*headers, title=, caption=, box=, show_header=, show_lines=, show_edge=, expand=, border_style=)`; `add_column(...)`, `add_row(*renderables)`, `row_count` |
| `rs_rich.panel` | `Panel(renderable, box, title=, title_align=, subtitle=, subtitle_align=, expand=, border_style=, width=, padding=)`, `Panel.fit` |
| `rs_rich.box` | Rich's box constants |
| `rs_rich.markup` | `escape` |
| `rs_rich.errors` | Rich's exceptions |

The other Rich modules exist and are being filled in. See the documentation's
Python section for the known differences.

## Versions

The package has its own version (0.0.1) and is released from `python-v…`
tags. It bundles the rs-rich Rust crate from the same commit.
