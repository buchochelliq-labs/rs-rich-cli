# Python bindings

`rs-rich` on PyPI puts the Rust port behind Rich's Python API. A Rich program
moves over by changing its imports; the rendering, down to the last byte, is
the Rust port's.

```bash
pip install rs-rich
```

```python
from rs_rich.console import Console   # was: from rich.console import Console
from rs_rich.table import Table       # was: from rich.table import Table

table = Table(title="Star Wars Movies")
table.add_column("Released", justify="right", style="cyan", no_wrap=True)
table.add_column("Title", style="magenta")
table.add_row("Dec 20, 2019", "Star Wars: The Rise of Skywalker")

Console().print(table)
```

The package and its import name are `rs-rich` and `rs_rich`. It never claims
the `rich` namespace, so both can be installed side by side.

## What 0.0.1 covers

This is the first slice (#197). Everything else is left out on purpose: an
unsupported argument or object raises `NotImplementedError` or `TypeError`
rather than rendering something different from Rich.

| Module | Supported |
|---|---|
| `rs_rich` | `print(*objects, sep=" ")`, `get_console()` |
| `rs_rich.console` | `Console(file=, width=, height=, color_system=, force_terminal=, no_color=, record=, highlight=, emoji=, safe_box=)`; `print(*objects, sep=" ", justify=None)`, `rule(title, characters=, style=)`, `export_text(clear=, styles=)`; `file`, `width`, `height`, `is_terminal`, `color_system` |
| `rs_rich.text` | `Text(text, style, justify=, overflow=, no_wrap=)`, `Text.from_markup`, `append`, `stylize`, `plain`, `len()` |
| `rs_rich.style` | `Style(color=, bgcolor=, bold=, dim=, italic=, underline=, blink=, reverse=, conceal=, strike=, link=)`, `Style.parse`, `+`, `==` |
| `rs_rich.table` | `Table(*headers, title=, caption=, box=, show_header=, show_lines=, show_edge=, expand=, border_style=)`; `add_column(header, style=, header_style=, justify=, overflow=, width=, min_width=, max_width=, ratio=, no_wrap=)`, `add_row(*cells)` with `str` or `Text` cells, `row_count` |
| `rs_rich.panel` | `Panel(renderable, box, title=, title_align=, subtitle=, subtitle_align=, expand=, border_style=, width=, padding=)`, `Panel.fit` |
| `rs_rich.box` | Rich's box constants |
| `rs_rich.markup` | `escape` |
| `rs_rich.errors` | `MarkupError`, `StyleSyntaxError` |

`Console.print` takes `str` (console markup), numbers, `None`, `Text`,
`Table` and `Panel`. A `Panel` holds any of those renderables.

**Known differences:**

- `Console.print` supports `end="\n"` only, `justify=` for strings only, and no `style=`.
- Consecutive `str` arguments are joined with `sep` before their markup is
  read, so a tag may span arguments; Rich reads each separately.
- Table cells are `str` or `Text`.
- Hyperlinks carry no `id=`, as in the Rust port
  ([Divergences #20](DIVERGENCES.md)); Rich's ids are random.

## How it stays compatible

- **All rendering is Rust.** The Python modules only re-export classes from
  the compiled `rs_rich._native` module. That module converts Python
  arguments into the core `rich` types (character offsets into byte offsets,
  keyword styles into a style definition) and writes the rendered result to
  the console's `file`.
- **Byte comparison with Rich 15.0.0.** `crates/rich-py/tests/test_compat.py`
  runs each program once with `rich` and once with `rs_rich` and compares the
  output byte for byte, in truecolor and without colour, including
  `export_text`. `test_api.py` runs Rich's README table example with only its
  imports changed.

```bash
python -m venv .venv && . .venv/bin/activate
pip install maturin "rich==15.0.0" pytest
cd crates/rich-py && maturin develop && pytest tests
```

## Wheels and releases

- **Wheels.** One abi3 wheel per platform covers CPython 3.9 and later. The
  targets are Linux (x86-64 and arm64, manylinux), macOS (arm64 and x86-64)
  and Windows (x86-64). The `python` workflow builds every wheel on each
  change to core or the bindings, then installs it and renders with it on
  Python 3.9 and 3.13.
- **Releases.** A `python-vX.Y.Z` tag on `main` runs `pypi-release.yml`,
  which checks that the tag matches `pyproject.toml`'s version. It then builds
  the wheels and the sdist and publishes them with PyPI Trusted Publishing,
  from the `pypi` environment, with no token secret. See
  [Branching and releases](BRANCHING.md#python-package-pypi).
- **Versioning.** The package is versioned on its own, starting at 0.0.1. It
  bundles the Rust crates from its tag's commit, and `crates/rich-py` is never
  published to crates.io.
