# Getting started

This page takes you from installing `rs-rich` to using it: first output, moving
an existing Rich program over, and the features the Rust port adds on top of
Rich. Every example below runs in the test suite, and its output is checked.

## Install

```bash
pip install rs-rich
```

- **Python** 3.9 or later (CPython). One wheel per platform covers every
  version.
- **Platforms with wheels:** Linux (x86-64, arm64), macOS (arm64, x86-64) and
  Windows (x86-64). Elsewhere pip builds from the source distribution, which
  needs a Rust toolchain.
- **No dependencies.** Pillow is optional: install it if you want to pass
  Pillow images to `rs_rich.art`.
- **Next to Rich.** The package imports as `rs_rich` and never touches the
  `rich` namespace, so `rich` and `rs-rich` can be installed together.

Check the install:

```bash
python -c "import rs_rich; print(rs_rich.__version__)"
python -m rs_rich --version
```

### Optional builds

Two features are left out of the standard wheels because of their size or
their external dependencies. Build them from source with
[maturin](https://www.maturin.rs):

```bash
git clone https://github.com/buchochelliq-labs/rs-rich-cli
cd rs-rich-cli/crates/rich-py
pip install maturin
maturin build --release --features lumis   # the tree-sitter code highlighter
maturin build --release --features mmdc    # Mermaid through Mermaid's own CLI
pip install target/wheels/rs_rich-*.whl
```

`mmdc` also needs Mermaid's CLI (`npm install -g @mermaid-js/mermaid-cli`).
Without it, Mermaid flowcharts are drawn as text.

## First output

```python
from rs_rich.console import Console

console = Console(width=40)
console.print("Hello, [bold magenta]World[/]!", ":sparkles:")
console.print({"name": "rs-rich", "version": [0, 0, 1], "rust": True})
```

```text
Hello, World! ✨
{
    'name': 'rs-rich',
    'version': [0, 0, 1],
    'rust': True
}
```

`Console` takes the same arguments as Rich's. Markup, emoji codes and
highlighting work as they do in Rich, and containers are pretty-printed. The
global `rs_rich.print` works too.

## Move a Rich program over

Change `rich` to `rs_rich` in the imports. Nothing else changes:

| Rich | rs-rich |
|---|---|
| `from rich.console import Console` | `from rs_rich.console import Console` |
| `from rich.table import Table` | `from rs_rich.table import Table` |
| `from rich import print` | `from rs_rich import print` |
| `from rich.progress import track` | `from rs_rich.progress import track` |
| `rich.traceback.install()` | `rs_rich.traceback.install()` |

To try it without editing each file, alias the module where the program
starts:

```py
import sys
import rs_rich
sys.modules.setdefault("rich", rs_rich)  # before anything imports rich
```

Output is byte-for-byte Rich 15.0.0's, with a few documented exceptions, such
as syntax-highlighting colours coming from syntect instead of Pygments. See
[Compatibility](compatibility.md) for the full list.

## A short tour

Renderables nest the same way they do in Rich:

```python
from rs_rich.console import Console
from rs_rich.panel import Panel
from rs_rich.table import Table
from rs_rich.tree import Tree

table = Table("Crate", "Version", title="Cohort")
table.add_row("rs-rich", "0.0.8")
table.add_row("rs-rich-ext", "0.0.10")

tree = Tree("rs_rich")
tree.add("console").add("Console")
tree.add("table").add("Table")

console = Console(width=40)
console.print(Panel.fit(table, title="Table in a panel"))
console.print(tree)
```

```text
╭──── Table in a panel ─────╮
│          Cohort           │
│ ┏━━━━━━━━━━━━━┳━━━━━━━━━┓ │
│ ┃ Crate       ┃ Version ┃ │
│ ┡━━━━━━━━━━━━━╇━━━━━━━━━┩ │
│ │ rs-rich     │ 0.0.8   │ │
│ │ rs-rich-ext │ 0.0.10  │ │
│ └─────────────┴─────────┘ │
╰───────────────────────────╯
rs_rich
├── console
│   └── Console
└── table
    └── Table
```

Markdown, Syntax, JSON, Pretty and tracebacks all work:

```python
from rs_rich.console import Console
from rs_rich.markdown import Markdown

console = Console(width=40, color_system=None)
console.print(Markdown("# Notes\n\n- fast\n- **byte-identical** to Rich"))
```

```text
                 Notes                  

 • fast                                 
 • byte-identical to Rich               
```

Live displays run in threads, as in Rich (not run here, because they draw
over time):

```py
import time
from rs_rich.progress import track

for step in track(range(20), description="Working..."):
    time.sleep(0.05)
```

```py
import logging
from rs_rich.logging import RichHandler

logging.basicConfig(level="INFO", handlers=[RichHandler()], format="%(message)s")
logging.getLogger("app").info("rendered by Rust")
```

Your own classes render through Rich's protocol (`__rich__`,
`__rich_console__`, `__rich_measure__`), including inside tables and panels.
See [The render protocol](protocol.md).

## Beyond Rich

The package also exposes the Rust crates' own features. Rich has none of
these.

**Images** in ASCII, Braille, half-blocks, quadrants or Sixel
([Art](art.md)):

```python
from rs_rich import art
from rs_rich.console import Console

def gradient(w, h):
    pixels = bytearray()
    for y in range(h):
        for x in range(w):
            pixels += bytes((x * 255 // (w - 1), y * 255 // (h - 1), 128, 255))
    return art.ArtImage.frombytes("RGBA", (w, h), pixels)

image = gradient(32, 16)
console = Console(width=40, color_system=None)
console.print(art.ImageArt(image, mode="ascii", width=32))
```

```

**Mermaid** flowcharts drawn as text ([Mermaid](mermaid.md)):

```python
from rs_rich.console import Console
from rs_rich.mermaid import Mermaid

console = Console(width=50, color_system=None)
console.print(Mermaid("graph LR\n  A[Start] --> B{Ready?}\n  B -->|yes| C((Go))"))
console.print(Mermaid("graph TD\n  A --> B", ascii=True))
```

```

**Extensions** (`rs_rich.ext`): diagnostics and stack traces, structured data
(JSON, YAML, TOML, XML, INI, dotenv), diffs and test reports, workflow views,
redaction, inspectors and more. See [Extensions](ext/index.md).

**Plugins** (`rs_rich.plugins`): write a highlighter, theme, renderer, Markdown
fence renderer or text transform in Python, and register it with the same
host the Rust plugins use. See [Plugins](plugins.md).

**The command line:** the wheel ships the `rich` CLI as `rich-rs` and
`python -m rs_rich` ([The command line](cli.md)):

```bash
rich-rs README.md
rich-rs data.json --panel rounded
python -m rs_rich mermaid flow.mmd
```

## Next steps

- [Python bindings overview](index.md): every module and its page.
- [Console](console.md): every `print` option, capture, export and themes.
- [Compatibility](compatibility.md): what is covered and the known differences.
