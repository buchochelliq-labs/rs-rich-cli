# Mermaid diagrams

```python
from rs_rich import mermaid
```

`rs_rich.mermaid` is the `rs-rich-mermaid` crate from Python. Rich has no
counterpart.

## Mermaid

```text
Mermaid(source, *, backend="text", ascii=None, mmdc=None)
```

A renderable Mermaid diagram. Flowcharts (`graph` / `flowchart`, in any
direction, with the common node shapes, edge strokes, heads and labels) are
drawn as text. Anything the text renderer cannot draw (another diagram type,
a syntax error, a chart too large to lay out) is shown as its source, in a
code block, under a one-line note saying why. A diagram wider than the
console is cropped, with a note.

`ascii=True` draws with ASCII only; `None` follows the console.

```python
from rs_rich.console import Console
from rs_rich.mermaid import Mermaid

console = Console(width=50, color_system=None)
console.print(Mermaid("graph LR\n  A[Start] --> B{Ready?}\n  B -->|yes| C((Go))"))
console.print(Mermaid("graph TD\n  A --> B", ascii=True))
```

```text
┌───────┐  ╱────────╲       ╭────╮
│ Start ├─►< Ready? >──yes─►( Go )
└───────┘  ╲────────╱       ╰────╯

+---+
| A |
+-+-+
  |
  v
+---+
| B |
+---+

```

```python
console.print(Mermaid("sequenceDiagram\n  Alice->>Bob: Hi"))
```

```text
Mermaid: sequence diagrams are not drawn as text
                                                  
 sequenceDiagram                                  
   Alice->>Bob: Hi                                
                                                  
```

### The mmdc backend

`backend="mmdc"` renders every diagram type with Mermaid's own CLI (`mmdc`:
Node and headless Chromium, installed separately) and shows the image
through `rs_rich.art`: Sixel where the terminal supports it, else quadrant
blocks. It needs a wheel built with the `mmdc` feature
(`mermaid.MERMAID_HAS_MMDC`); other builds, and any failure, fall back to
text with a note. `MmdcOptions(*, program="mmdc", timeout=20.0,
max_input=65536, max_output=16777216, puppeteer_config=None,
background="white")` says how to run it, and
`mmdc_render_png(source, options=None)` returns the PNG bytes (raising
`MmdcError`, or `NotImplementedError` without the feature).

## Parsing and drawing

`parse_flowchart(source)` returns a `Flowchart` (`direction`, `nodes`,
`edges`, `notes`) and `draw_flowchart(chart, ascii=False)` a
`MermaidDiagram` (`lines`, `width`). Where `Mermaid` shows the source, these
raise:

- `MermaidParseError`, with `kind` `"empty"`, `"unsupported"` (another
  diagram type), `"too_large"` (over `MERMAID_MAX_SOURCE` bytes,
  `MERMAID_MAX_NODES` nodes or `MERMAID_MAX_EDGES` edges) or `"syntax"`
  (with its `line`);
- `MermaidLayoutError` when the chart is too large to lay out.

Both derive from `MermaidError`. The same functions are also available under
the crate's names, `mermaid.parse` and `mermaid.draw`.

```python
chart = mermaid.parse_flowchart("graph TD\n  A[Start] -.->|maybe| B((End))")
print(chart.direction, [node.shape for node in chart.nodes])
print(chart.edges[0])
print(mermaid.draw_flowchart(chart))
try:
    mermaid.parse_flowchart("graph TD\n  A -->")
except mermaid.MermaidParseError as error:
    print(error.kind, error.line, error)
```

```text
TD ['rect', 'circle']
FlowchartEdge(source=0, target=1, label='maybe', stroke='dotted', start=None, end='arrow', length=1)
┌───────┐
│ Start │
└───┬───┘
    ┆
  maybe
    ▼
 ╭─────╮
 ( End )
 ╰─────╯
syntax 2 line 2: an arrow needs a node after it
```

## Markdown fences

`MermaidFences(*, backend="text", ascii=None, mmdc=None)` is the fence
renderer for ```` ```mermaid ```` blocks in Markdown: `accepts(language)`
says whether it draws a fence, and `render_fence(language, code)` returns
the `Mermaid` for it (or `None`, leaving the fence to `Syntax`). It has the
signature of a Python fence renderer, `render_fence(language, code, console,
options)`, so it can be given wherever `rs_rich` takes one (see
[plugins](plugins.md)). The plugin that registers it with a plugin host is
`MermaidPlugin`, from `rs_rich.plugins` (re-exported here).

```python
fences = mermaid.MermaidFences()
print(fences.accepts("mermaid"), fences.accepts("python"))
console.print(fences.render_fence("mermaid", "graph LR\n  x --> y"))
```

```text
True False
┌───┐  ┌───┐
│ x ├─►│ y │
└───┘  └───┘

```
