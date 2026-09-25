# Transforms

Module: `rs_rich.ext.transform` (Rust: `rich_ext::transform`). A `Pipeline`
runs named stages in order. A stage is any object with an `apply(value)`
method or any callable; its result feeds the next stage.

```python
from rs_rich.console import Console
from rs_rich.ext.transform import HighlightMatches, KeepLines, Pipeline
from rs_rich.text import Text

console = Console(width=60)
log = Text("ok: started\nerror: disk full\nok: retry\nerror: gave up")
pipeline = (
    Pipeline()
    .then("errors", KeepLines("^error"))
    .then("mark", HighlightMatches("disk", "bold"))
    .then("upper", lambda text: Text(text.plain.upper()))
)
console.print(pipeline.apply(log))
print(KeepLines("^ok", invert=True)(log).plain)
```

```text
ERROR: DISK FULL
ERROR: GAVE UP
error: disk full
error: gave up
```

`KeepLines(pattern, invert=False)` keeps the lines of a `Text` matching a
regular expression; `HighlightMatches(pattern, style)` styles every match.
Other modules provide stages for their own values: `data.Select`,
`data.Filter`, `data.Highlight`, `data.Redact` for a `data.Document`,
`table.TableSort` and `table.TableGroup` for a `TableData`, and
`diff.KeepFiles` for a `Patch`.

A stage that raises `TransformError` stops the pipeline with a
`PipelineError` naming the stage:

```python
from rs_rich.ext.transform import PipelineError, TransformError

def reject(text):
    raise TransformError("empty input")

try:
    Pipeline().then("check", reject).apply(Text(""))
except PipelineError as error:
    print(error, "|", error.stage, "|", error.reason)
```

```text
check: empty input | check | empty input
```

The plugin registry (`ExtensionRegistry.register_transform`) builds
pipelines from registered stages by name; see the plugin guide.
