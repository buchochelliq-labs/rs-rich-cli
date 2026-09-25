# Layout and live output

Modules: `rs_rich.ext.layout` and `rs_rich.ext.live` (Rust:
`rich_ext::layout`, `rich_ext::live`).

## Bounded layouts

`LayoutNode` is a renderable with size constraints: a fixed size, a
`Constraint(min=, max=, flex=)`, or `"content"` to size to what it holds.
`LayoutNode.split("vertical" | "horizontal", children)` divides the space;
`align` places content inside its box and `overflow` says what happens when
it does not fit. `validate()` checks the tree and raises `ConstraintError`.

```python
from rs_rich.console import Console
from rs_rich.ext import layout
from rs_rich.panel import Panel
from rs_rich.text import Text

header = layout.LayoutNode(Text("acme deploy"), height=1)
sidebar = layout.LayoutNode(Text("api\nworker\nweb"), width=layout.Constraint(min=6, max=12), content_width=True)
body = layout.LayoutNode(Panel("v2.4.1 on 3 of 4 services", title="status"))
footer = layout.LayoutNode(Text("q quit"), height=1, align=("end", "start"))
screen = layout.LayoutNode.split("vertical", [header, layout.LayoutNode.split("horizontal", [sidebar, body]), footer])
screen.validate()
Console(width=40, height=7).print(screen)
```

```text
acme deploy                             
api   ╭──────────── status ────────────╮
worker│ v2.4.1 on 3 of 4 services      │
web   │                                │
      │                                │
      ╰────────────────────────────────╯
                                  q quit
```

`allocate(total, constraints)` is the size solver on its own: it returns the
sizes, the padding left over, and whether minimums had to be relaxed.

```python
constraints = [layout.Constraint.fixed(20), layout.Constraint(min=10, max=30), layout.Constraint(min=5, flex=2)]
print(layout.allocate(60, constraints))
print(layout.allocate(30, constraints))
```

```text
([20, 19, 21], 0, [])
([15, 10, 5], 0, [0])
```

`Overflowing(renderable, policy)` renders with one overflow policy (`"fold"`,
`"crop"`, `"ellipsis"`, `"wrap"`), and `fit_segments(segments, width)` fits
segments to lines.

```python
code = Text("let answer = compute_the_answer(universe, everything, 42);")
Console(width=30).print(layout.Overflowing(code, "ellipsis"))
```

```text
let answer = compute_the_answ…
```

## The live coordinator

`LiveCoordinator(file, target=...)` shares one output between several live
regions and ordinary printed lines. On a terminal it redraws the regions in
place; on a plain stream (a pipe, CI) it prints each region once when it
settles, so logs stay readable. Use it as a context manager, or call
`finish()`.

```python
import io
from rs_rich.ext import live, target

out = io.StringIO()
stream = target.RenderTarget("plain_stream", width=40, color_system=None)
with live.LiveCoordinator(out, target=stream) as coordinator:
    build = coordinator.add(Text("build   running"))
    coordinator.add("tests   queued")
    coordinator.refresh()
    coordinator.print(Text("compiled 12 crates"))
    coordinator.update(build, "build   done")
print(out.getvalue(), end="")
```

```text
compiled 12 crates
build   done
tests   queued
```

Regions are added, updated and removed by the `RegionId` that `add`
returns. `countdown(seconds, message)` shows a waiting countdown, and
`present(notifications, now)` shows a `Notifications` stack.
