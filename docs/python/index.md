# Python bindings

`rs-rich` on PyPI puts the Rust port behind Rich's Python API. A Rich program
moves over by changing its imports. The rendering, down to the last byte, is
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

Console(width=50).print(table)
```

```text
                 Star Wars Movies                 
┏━━━━━━━━━━━━━━┳━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━┓
┃     Released ┃ Title                           ┃
┡━━━━━━━━━━━━━━╇━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━┩
│ Dec 20, 2019 │ Star Wars: The Rise of          │
│              │ Skywalker                       │
└──────────────┴─────────────────────────────────┘
```

The package is `rs-rich` and it imports as `rs_rich`. It never claims the
`rich` namespace, so both can be installed side by side.

## The API

The package covers all of Rich 15.0.0's API, plus the port's own crates. The
modules have Rich's names, so imports translate one for one, and every page
below has runnable examples whose output is checked by the tests.

### Rich's modules

| Rich | rs_rich | Reference |
|---|---|---|
| `rich.print`, `rich.get_console`, `rich.reconfigure`, `rich.print_json`, `rich.inspect` | the same names on `rs_rich` | [Console](console.md#the-global-console) |
| `rich.console` (`Console`, `ConsoleOptions`, `Capture`, `Group`, `group`, ...) | `rs_rich.console` | [Console](console.md) |
| `rich.segment`, `rich.measure`, `__rich__`, `__rich_console__`, `__rich_measure__` | `rs_rich.segment`, `rs_rich.measure`, the same methods | [The render protocol](protocol.md) |
| `rich.text` (`Text`, `Span`, `Lines`), `rich.emoji` | `rs_rich.text`, `rs_rich.emoji` | [Text](text.md) |
| `rich.style`, `rich.theme` | `rs_rich.style`, `rs_rich.theme` | [Style](style.md) |
| `rich.color` | `rs_rich.color` | [Color](color.md) |
| `rich.box`, `rich.markup`, `rich.errors`, `rich.terminal_theme` | `rs_rich.box`, `rs_rich.markup`, `rs_rich.errors`, `rs_rich.terminal_theme` | [Boxes, markup and errors](box-markup-errors.md) |
| `rich.table` | `rs_rich.table` | [Table](table.md) |
| `rich.panel` | `rs_rich.panel` | [Panel](panel.md) |
| `rich.rule`, `rich.padding`, `rich.align`, `rich.constrain`, `rich.styled`, `rich.bar`, `rich.spinner` | the same under `rs_rich` | [Rules, padding, alignment and bars](rule.md) |
| `rich.columns`, `rich.containers`, `rich.layout` | the same under `rs_rich` | [Layout, columns and groups](layout.md) |
| `rich.tree` | `rs_rich.tree` | [Tree](tree.md) |
| `rich.markdown` | `rs_rich.markdown` | [Markdown](markdown.md) |
| `rich.syntax` | `rs_rich.syntax` | [Syntax](syntax.md) |
| `rich.pretty`, `rich.json`, `rich.highlighter`, `rich.inspect` | the same under `rs_rich` | [Pretty, JSON, inspect and highlighters](pretty.md) |
| `rich.traceback` | `rs_rich.traceback` | [Traceback](traceback.md) |
| `rich.live`, `rich.live_render`, `rich.status`, `rich.screen`, `rich.pager` | the same under `rs_rich` | [Live, status, screen and pager](live.md) |
| `rich.progress`, `rich.progress_bar` | `rs_rich.progress`, `rs_rich.progress_bar` | [Progress](progress.md) |
| `rich.prompt` | `rs_rich.prompt` | [Prompts](prompt.md) |
| `rich.logging` | `rs_rich.logging` | [Logging](logging.md) |

Your own classes render as they do with Rich, through `__rich__`,
`__rich_console__` and `__rich_measure__`, anywhere a renderable goes.

### The port's own crates

| Crate | rs_rich | Reference |
|---|---|---|
| `rs-rich-ext` (38 modules: diagnostics, data, diffs, workflows, tables, terminals, testing and QA, ...) | `rs_rich.ext`, `rs_rich.ext.<module>` | [Extensions](ext/index.md) |
| `rs-rich-art` (images, FIGlet, GIFs, image diffs) | `rs_rich.art` | [Art](art.md) |
| `rs-rich-mermaid` | `rs_rich.mermaid` | [Mermaid](mermaid.md) |
| `rs-rich-plugin-api` and the extension registry | `rs_rich.plugins` | [Plugins](plugins.md) |
| `rs-rich-cli` (the `rich` command) | `python -m rs_rich`, the `rich-rs` script, `rs_rich.cli.main` | [The command line](cli.md) |

The little Rich has that the port cannot do (Jupyter output) raises
`NotImplementedError` rather than rendering something different from Rich.
[Compatibility](compatibility.md) lists what is covered, the known
differences, and how the byte comparison with Rich 15.0.0 works.

Every class ships with type stubs (`rs_rich/_native.pyi`), so editors and type
checkers see the signatures documented here.

## How it works

- **All rendering is Rust.** The Python modules only re-export classes from
  the compiled `rs_rich._native` module.
- **The native module is glue.** It converts Python arguments into the core
  `rich` crate's types: character offsets into byte offsets, keyword styles
  into a style definition. It then writes core's output to the console's
  `file`.
- **Objects are specifications.** A `Table` or `Panel` stores what it was
  given and becomes a core object only when printed, so a table can still gain
  rows after it has been put in a panel.
- **Your objects render in place.** When core reaches one of your objects
  (in a table cell, say), it calls back into Python for its `__rich_console__`
  or `__rich_measure__`, with the GIL held and no lock taken, and an exception
  raised there comes out of `print`.

## Wheels and releases

- **Wheels.** One abi3 wheel per platform covers CPython 3.9 and later: Linux
  (x86-64 and arm64, manylinux), macOS (arm64 and x86-64) and Windows
  (x86-64). The `lumis` (tree-sitter) code highlighter is a separate, larger
  build (`--features lumis`); Mermaid's `mmdc` backend needs `--features mmdc`
  and Mermaid's own CLI. The `python` workflow builds each wheel on every change to core or
  the bindings, then installs it and renders with it on Python 3.9 and 3.13.
- **Releases.** A `python-vX.Y.Z` tag on `main` runs `pypi-release.yml`. It
  checks the tag against `pyproject.toml`'s version, builds the wheels and the
  sdist, runs the whole test suite (compatibility with Rich 15.0.0 included)
  against the Linux x86-64 wheel, and only then publishes them with PyPI Trusted Publishing from the `pypi`
  environment, with no token secret. See
  [Branching and releases](../BRANCHING.md#python-package-pypi).
- **Versions.** The package has its own version, starting at 0.0.1. It
  bundles the Rust crates from its tag's commit, and `crates/rich-py` is never
  published to crates.io.

## Building from source

```bash
python -m venv .venv && . .venv/bin/activate
pip install maturin "rich==15.0.0" pytest
cd crates/rich-py
maturin develop
pytest tests
```

The tests need Rich 15.0.0 only for the byte comparison (and Pillow, if
installed, for the art tests that take Pillow images). Install it in its own
virtualenv, never alongside `rich-cli` (see `AGENTS.md`). `tests/test_cli.py`
compares `python -m rs_rich` with the `rich` binary: it builds it with cargo,
or uses `RS_RICH_CLI_BIN`.
