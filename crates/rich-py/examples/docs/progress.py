"""The pictures on docs/python/progress.md: run it, or scripts/capture_python_docs.py."""

from _shots import Shots

from rs_rich.progress import BarColumn, Progress, ProgressColumn, TextColumn, track
from rs_rich.progress_bar import ProgressBar
from rs_rich.text import Text


def tasks(console):
    # --8<-- [start:tasks]
    now = [0.0]
    with Progress(console=console, auto_refresh=False, get_time=lambda: now[0]) as progress:
        download = progress.add_task("Download", total=200)
        unpack = progress.add_task("Unpack", total=None)
        for _ in range(4):
            now[0] += 1
            progress.advance(download, 40)
    # --8<-- [end:tasks]


def fields(console):
    # --8<-- [start:fields]
    progress = Progress(
        TextColumn("{task.description}: {task.completed}/{task.total} {task.fields[unit]}"),
        console=console,
    )
    task = progress.add_task("Files", total=12, unit="files")
    progress.update(task, advance=5)
    progress.console.print(progress)
    # --8<-- [end:fields]


# --8<-- [start:left]
class Left(ProgressColumn):
    def render(self, task):
        return Text(f"{task.remaining} left", style="magenta")
# --8<-- [end:left]


def column(console):
    # --8<-- [start:column]
    progress = Progress(BarColumn(bar_width=20), Left(), console=console)
    task = progress.add_task("", total=10, completed=7)
    progress.console.print(progress)
    # --8<-- [end:column]


def tracked(console):
    # --8<-- [start:track]
    now = [0.0]
    for value in track(range(3), description="Counting", console=console,
                       auto_refresh=False, get_time=lambda: now[0]):
        now[0] += 1
    # --8<-- [end:track]


def bar(console):
    # --8<-- [start:bar]
    console.print(ProgressBar(total=100, completed=40, width=20))
    print()
    # --8<-- [end:bar]


# A display on a terminal redraws itself; off one it writes only its last
# frame, which is the still these pictures want.
shots = Shots("progress")
shots.shot("tasks", tasks, width=60, terminal=False)
shots.shot("fields", fields, width=40)
shots.shot("column", column, width=40)
shots.shot("track", tracked, width=60, terminal=False)
shots.shot("bar", bar, width=40)
shots.finish()
