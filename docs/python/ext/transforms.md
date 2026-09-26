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

## `Pipeline` or `TextPipeline`?

Both exist because Rust has both, and they answer different questions.
`Pipeline` (this page, `rich_ext::transform`) is built in code from stage
objects: any value, not only `Text`, with `TransformError` and
`PipelineError` naming the stage. `TextPipeline` (from
`ExtensionRegistry.text_pipeline(names)`, `rich_plugin_api`) is assembled
from transforms *registered by name* by plugins, works on `Text` only, and
fails with `PluginError(kind="pipeline")`. A `KeepLines` or
`HighlightMatches` stage can be registered as a plugin transform, so the
same stages serve both; see [Using what is registered](../plugins.md#using-what-is-registered).
