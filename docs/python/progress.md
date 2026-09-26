# Progress

```python
from rs_rich.progress import Progress, track, wrap_file, open
from rs_rich.progress_bar import ProgressBar
```

`rs_rich.progress` corresponds to `rich.progress`: a `Progress` display of
tasks, its columns, and the `track`, `wrap_file` and `open` helpers.
`ProgressBar` is `rich.progress_bar.ProgressBar`. Output is byte-for-byte
Rich 15.0.0's (see `tests/test_live.py`).

## Progress

```text
Progress(*columns, console=None, auto_refresh=True, refresh_per_second=10,
         speed_estimate_period=30.0, transient=False, redirect_stdout=True,
         redirect_stderr=True, get_time=None, disable=False, expand=False)
```

A `Progress` is a [`Live`](live.md) display of a grid: one row per task, one
cell per column. Without columns it uses `Progress.get_default_columns()`: the
description, a bar, the percentage and the time remaining. A `str` column is a
format string, expanded with `str.format(task=task)` and printed as markup.
`get_time` replaces the clock (the console's `get_time` by default), which
makes elapsed times, speeds and spinners reproducible.

| Method | Does |
|---|---|
| `add_task(description, start=True, total=100.0, completed=0, visible=True, **fields)` | Add a task; returns its id (a `TaskID`, an `int`). |
| `update(task_id, *, total, completed, advance, description, visible, refresh=False, **fields)` | Change a task. |
| `advance(task_id, advance=1)` | Add to `completed`. |
| `reset(task_id, *, start=True, total, completed=0, visible, description, **fields)` | Start a task over. |
| `start_task(task_id)`, `stop_task(task_id)` | Start or freeze a task's clock. |
| `remove_task(task_id)` | Remove a task. |
| `track(sequence, total=None, completed=0, task_id=None, description="Working...", update_period=0.1)` | Iterate, advancing a task. |
| `wrap_file(file, total=None, *, task_id=None, description="Reading...")` | A binary reader that advances a task. |
| `open(file, mode="r", ..., *, total=None, task_id=None, description="Reading...")` | Open a file for reading, advancing a task. |
| `start()`, `stop()`, `refresh()` | Control the display; a `Progress` is also a context manager. |
| `get_renderables()`, `make_tasks_table(tasks)` | Override `get_renderables` to change what the display shows. |

Properties: `tasks`, `task_ids`, `finished`, `console`, `live`, `columns`,
`print` and `log` (the console's).

```python
from rs_rich.console import Console
from rs_rich.progress import Progress

now = [0.0]
console = Console(width=60)
with Progress(console=console, auto_refresh=False, get_time=lambda: now[0]) as progress:
    download = progress.add_task("Download", total=200)
    unpack = progress.add_task("Unpack", total=None)
    for _ in range(4):
        now[0] += 1
        progress.advance(download, 40)
```

```text
Download ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━          80% 0:00:01
Unpack   ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━             
```

Task values keep their Python types, and every attribute of a `Task` (`id`,
`description`, `total`, `completed`, `fields`, `percentage`, `elapsed`,
`speed`, `time_remaining`, `finished`, ...) is available to format strings:

```python
from rs_rich.console import Console
from rs_rich.progress import Progress, TextColumn

progress = Progress(
    TextColumn("{task.description}: {task.completed}/{task.total} {task.fields[unit]}"),
    console=Console(width=40),
)
task = progress.add_task("Files", total=12, unit="files")
progress.update(task, advance=5)
progress.console.print(progress)
```

```text
Files: 5/12 files
```

## Columns

`TextColumn`, `BarColumn`, `SpinnerColumn`, `TimeElapsedColumn`,
`TimeRemainingColumn`, `MofNCompleteColumn`, `FileSizeColumn`,
`TotalFileSizeColumn`, `DownloadColumn`, `TransferSpeedColumn`,
`TaskProgressColumn` and `RenderableColumn` take Rich's arguments, including
`table_column=` (any object with `rich.table.Column`'s attributes: `justify`,
`width`, `ratio`, `no_wrap`, `style`, ...).

A column of your own subclasses `ProgressColumn` and implements
`render(task)`, returning any renderable. `max_refresh` (seconds) reuses a
render while the task has completed nothing, as `TimeRemainingColumn` does.

```python
from rs_rich.console import Console
from rs_rich.progress import BarColumn, Progress, ProgressColumn
from rs_rich.text import Text


class Left(ProgressColumn):
    def render(self, task):
        return Text(f"{task.remaining} left", style="magenta")


progress = Progress(BarColumn(bar_width=20), Left(), console=Console(width=40))
task = progress.add_task("", total=10, completed=7)
progress.console.print(progress)
```

```text
━━━━━━━━━━━━━━       3 left
```

## track, wrap_file and open

`track(sequence, description="Working...", total=None, completed=0,
auto_refresh=True, console=None, transient=False, get_time=None,
refresh_per_second=10, style=..., complete_style=..., finished_style=...,
pulse_style=..., update_period=0.1, disable=False, show_speed=True)` iterates
over `sequence` inside its own display. `wrap_file(file, total, *, ...)` and
`open(file, mode="r", ..., *, total=None, ...)` return context managers
yielding a reader that advances the display as it is read.

```python
from rs_rich.console import Console
from rs_rich.progress import track

now = [0.0]
for value in track(range(3), description="Counting", console=Console(width=60),
                   auto_refresh=False, get_time=lambda: now[0]):
    now[0] += 1
```

```text
Counting ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━ 100% 0:00:03
```

With `auto_refresh=True` (the default) a background thread advances the task
every `update_period` seconds, as in Rich; leaving the loop early (or calling
`close()` on the iterator) stops it.

## ProgressBar

```text
ProgressBar(total=100.0, completed=0, width=None, pulse=False,
            style="bar.back", complete_style="bar.complete",
            finished_style="bar.finished", pulse_style="bar.pulse",
            animation_time=None)
```

The bar `BarColumn` draws; `total=None` pulses. `update(completed,
total=None)` changes it, `percentage_completed` reads it.

```python
from rs_rich.console import Console
from rs_rich.progress_bar import ProgressBar

Console(width=40).print(ProgressBar(total=100, completed=40, width=20))
print()
```

```text
━━━━━━━━
```

## Differences from Rich

- `make_tasks_table()` returns a renderable grid, not an
  `rs_rich.table.Table` (it prints, and nests in panels and tables, the same).
- Without `rs_rich.table.Column`, `ProgressColumn.get_table_column()`
  returns `None` for a column created without `table_column=`.
- `SpinnerColumn` has no `spinner` attribute; use `set_spinner()`.
- Jupyter is not supported.
