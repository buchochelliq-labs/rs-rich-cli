# rs-rich (Python)

Rich-compatible terminal rendering for Python, backed by
[rs-rich](https://github.com/buchochelliq-labs/rs-rich-cli), a Rust port of
[Rich](https://github.com/Textualize/rich).

```bash
pip install rs-rich
```

Documentation: <https://buchochelliq-labs.github.io/rs-rich-cli/python/>, with a
[getting started guide](https://buchochelliq-labs.github.io/rs-rich-cli/python/getting-started/)
and an API reference for every module.

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
byte (the documentation's Python section lists the few known differences).
What the port cannot do (Jupyter output, a handful of `Table` options) raises
`NotImplementedError` rather than rendering something different.

## What is in it

| | Modules |
|---|---|
| All of Rich's API | `rs_rich` (`print`, `print_json`, `inspect`, ...), `console`, `text`, `style`, `color`, `theme`, `markup`, `emoji`, `segment`, `measure`, `box`, `errors`, `terminal_theme`, `table`, `panel`, `rule`, `padding`, `align`, `constrain`, `styled`, `bar`, `spinner`, `columns`, `containers`, `layout`, `tree`, `markdown`, `syntax`, `pretty`, `json`, `highlighter`, `traceback`, `live`, `live_render`, `status`, `screen`, `pager`, `progress`, `progress_bar`, `prompt`, `logging` |
| rs-rich's extensions | `rs_rich.ext` and its 38 submodules: diagnostics, structured data, diffs and test reports, transforms, workflows, tables and badges, terminal capabilities, inspectors, live layouts, CLI docs, testing and QA |
| Images and diagrams | `rs_rich.art` (images, Sixel, FIGlet, GIFs, image diffs; Pillow images when Pillow is installed), `rs_rich.mermaid` |
| Plugins | `rs_rich.plugins`: write highlighters, code highlighters, themes, boxes, renderers, fence renderers and transforms in Python, checked by the Rust plugin host |
| The command line | `python -m rs_rich` and the `rich-rs` script: rs-rich's `rich` command |

Your own classes render through `__rich__`, `__rich_console__` and
`__rich_measure__`, anywhere a renderable goes.

The `lumis` (tree-sitter) code highlighter is a separate build
(`maturin build --features lumis`), and Mermaid's `mmdc` backend needs
`--features mmdc` and Mermaid's own CLI.

## Versions

The package has its own version (0.0.1) and is released from `python-v…`
tags. It bundles the rs-rich Rust crate from the same commit.
