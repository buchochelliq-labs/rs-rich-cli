"""The live area: ``Live``, ``Progress``, ``Status``, ``Screen``, ``Pager``,
prompts and ``RichHandler``, byte-for-byte against Python ``rich`` 15.0.0.

Every compared program runs without a refresh thread (``auto_refresh=False``,
or a refresh rate too low to fire) and with a fake clock, so both libraries
write the same frames. The threaded tests check behaviour instead: frames,
clean stops and no hangs. Everything runs under a timeout.
"""

from __future__ import annotations

import importlib
import io
import logging
import os
import subprocess
import sys
import threading
import time
from types import SimpleNamespace

import pytest

TIMEOUT = 20.0


def within_timeout(fn, *args, timeout: float = TIMEOUT):
    """Run ``fn(*args)`` on a thread; fail if it does not finish in time."""
    result = {}

    def run():
        try:
            result["value"] = fn(*args)
        except BaseException as error:  # noqa: BLE001 - reported below
            result["error"] = error

    worker = threading.Thread(target=run, daemon=True)
    worker.start()
    worker.join(timeout)
    assert not worker.is_alive(), f"{getattr(fn, '__name__', fn)} did not finish in {timeout}s"
    if "error" in result:
        raise result["error"]
    return result.get("value")


@pytest.fixture(autouse=True)
def _a_real_terminal_type(monkeypatch):
    # `is_interactive` and `is_dumb_terminal` read TERM.
    monkeypatch.setenv("TERM", "xterm-256color")


def modules(package: str) -> SimpleNamespace:
    names = [
        "console", "live", "live_render", "progress", "progress_bar", "status", "screen",
        "pager", "prompt", "logging", "text", "table", "panel",
    ]
    return SimpleNamespace(**{name: importlib.import_module(f"{package}.{name}") for name in names})


class Clock:
    """A fake ``get_time``: advance it by hand."""

    def __init__(self) -> None:
        self.now = 0.0

    def __call__(self) -> float:
        return self.now


def console(m, color: bool, clock: Clock, **options):
    return m.console.Console(
        file=io.StringIO(),
        width=options.pop("width", 60),
        height=options.pop("height", 12),
        force_terminal=color,
        color_system="truecolor" if color else None,
        get_time=clock,
        **options,
    )


# ---------------------------------------------------------------------------
# Programs, written once against Rich's API


def live_updates(m, c, clock):
    with m.live.Live("hello", console=c, auto_refresh=False) as live:
        live.update("[bold]world[/]", refresh=True)
        live.update(m.panel.Panel("in a panel"))
        live.refresh()
        c.print("printed above")
        live.refresh()


def live_transient(m, c, clock):
    with m.live.Live("first\nsecond", console=c, auto_refresh=False, transient=True) as live:
        live.update(m.panel.Panel("x"), refresh=True)
        c.print("interleaved")
    c.print("after")


def live_vertical_overflow(m, c, clock):
    tall = "\n".join(f"row {n}" for n in range(30))
    for mode in ["crop", "ellipsis", "visible"]:
        with m.live.Live(tall, console=c, auto_refresh=False, vertical_overflow=mode) as live:
            live.refresh()


def live_nested(m, c, clock):
    with m.live.Live("outer", console=c, auto_refresh=False) as outer:
        with m.live.Live("inner", console=c, auto_refresh=False) as inner:
            inner.refresh()
            outer.refresh()
        outer.update("outer again", refresh=True)


def live_get_renderable(m, c, clock):
    count = [0]

    def get():
        count[0] += 1
        return f"call {count[0]}"

    with m.live.Live(console=c, auto_refresh=False, get_renderable=get) as live:
        live.refresh()
    c.print(live.is_started, live.transient, live.vertical_overflow)


def live_screen(m, c, clock):
    with m.live.Live("full screen", console=c, auto_refresh=False, screen=True) as live:
        live.update(m.panel.Panel("boxed"), refresh=True)
    c.print("back")


def live_redirect(m, c, clock):
    with m.live.Live("frame", console=c, auto_refresh=False):
        print("from print()")
        print("partial", end="")


def live_render_directly(m, c, clock):
    render = m.live_render.LiveRender("a\nbb\nccc")
    c.print(render)
    c.print(render.last_render_height, repr(str(render.position_cursor())))
    c.print(repr(str(render.restore_cursor())))


def progress_default(m, c, clock):
    with m.progress.Progress(console=c, auto_refresh=False, get_time=clock) as progress:
        task = progress.add_task("Downloading", total=200)
        for _ in range(4):
            clock.now += 1.5
            progress.advance(task, 25)
            progress.refresh()
        progress.update(task, completed=200, refresh=True)


def progress_every_column(m, c, clock):
    P = m.progress
    progress = P.Progress(
        P.SpinnerColumn(),
        *P.Progress.get_default_columns(),
        P.MofNCompleteColumn(),
        P.TimeElapsedColumn(),
        P.DownloadColumn(),
        P.DownloadColumn(binary_units=True),
        P.TransferSpeedColumn(),
        P.FileSizeColumn(),
        P.TotalFileSizeColumn(),
        "{task.fields[who]}",
        P.TaskProgressColumn(show_speed=True),
        P.TimeRemainingColumn(compact=True, elapsed_when_finished=True),
        P.RenderableColumn("[red]static"),
        console=c,
        auto_refresh=False,
        get_time=clock,
    )
    with progress:
        a = progress.add_task("one", total=4096, who="me")
        b = progress.add_task("two", total=None, who="you")
        progress.add_task("three", start=False, total=10, who="x")
        for _ in range(5):
            clock.now += 0.75
            progress.advance(a, 700)
            progress.update(b, advance=5)
            progress.refresh()
        progress.update(a, completed=4096)
        progress.stop_task(b)
        progress.refresh()


def progress_custom_columns(m, c, clock):
    P = m.progress

    class Ratio(P.ProgressColumn):
        def render(self, task):
            return m.text.Text(f"{task.completed}/{task.total}", style="magenta")

    class Label(P.TextColumn):
        def __init__(self):
            super().__init__("<{task.description}>", style="cyan", justify="right")

    progress = P.Progress(Label(), Ratio(), P.BarColumn(bar_width=None), console=c,
                          auto_refresh=False, get_time=clock, expand=True)
    with progress:
        task = progress.add_task("custom", total=8)
        progress.update(task, advance=3, description="renamed", refresh=True)


def progress_subclass(m, c, clock):
    class Boxed(m.progress.Progress):
        def get_renderables(self):
            yield m.panel.Panel(self.make_tasks_table(self.tasks), title="tasks")

    with Boxed(console=c, auto_refresh=False, get_time=clock) as progress:
        task = progress.add_task("boxed", total=10)
        progress.advance(task, 4)
        progress.refresh()


def progress_task_management(m, c, clock):
    progress = m.progress.Progress(console=c, auto_refresh=False, get_time=clock)
    with progress:
        a = progress.add_task("a", total=10)
        b = progress.add_task("b", total=10, visible=False)
        progress.update(b, visible=True, advance=10)
        clock.now = 3
        progress.reset(a, total=20, completed=5, description="a again")
        progress.remove_task(b)
        progress.start_task(a)
        progress.stop_task(a)
        progress.refresh()
    c.print(progress.task_ids, progress.finished, [task.description for task in progress.tasks])
    task = progress.tasks[0]
    c.print(task.id, task.total, task.completed, task.remaining, task.elapsed, task.percentage,
            task.started, task.finished, task.speed, task.time_remaining, task.fields)


def progress_printed(m, c, clock):
    progress = m.progress.Progress(console=c, get_time=clock)
    progress.add_task("printed", total=10, completed=3)
    progress.add_task("indeterminate", total=None)
    c.print(progress)
    c.print(m.panel.Panel(progress, title="in a panel"))


def track_helpers(m, c, clock):
    values = list(m.progress.track(range(6), description="Tracking", console=c,
                                   auto_refresh=False, get_time=clock))
    c.print(values)
    progress = m.progress.Progress(console=c, auto_refresh=False, get_time=clock)
    with progress:
        task = progress.add_task("existing", total=3)
        for _ in progress.track(["a", "b", "c"], task_id=task):
            clock.now += 1


def file_helpers(m, c, clock):
    data = b"0123456789" * 300
    with m.progress.wrap_file(io.BytesIO(data), total=len(data), console=c,
                              auto_refresh=False, get_time=clock) as reader:
        while reader.read(700):
            clock.now += 1
    path = os.path.join(os.path.dirname(os.path.abspath(__file__)), "__live_file_helper.txt")
    with open(path, "w", encoding="utf-8") as handle:
        handle.write("line\n" * 200)
    try:
        with m.progress.open(path, console=c, auto_refresh=False, get_time=clock,
                             description="Text mode") as handle:
            c.print(sum(1 for _ in handle))
        with m.progress.open(path, "rb", total=1000, console=c, auto_refresh=False,
                             get_time=clock) as handle:
            c.print(len(handle.read()))
    finally:
        os.unlink(path)


def progress_bars(m, c, clock):
    bar = m.progress_bar.ProgressBar
    c.print(bar(total=100, completed=33, width=20))
    c.print(bar(total=100, completed=100, width=10))
    c.print(bar(total=None, width=20, animation_time=1.5))
    c.print(bar(completed=12.5, width=15, pulse=True, animation_time=0.2))
    table = m.table.Table("name", "bar")
    table.add_row("half", bar(completed=50))
    c.print(table)
    b = bar(total=10, completed=2)
    b.update(5)
    c.print(repr(b), b.percentage_completed)


def status_display(m, c, clock):
    with c.status("Working [b]hard", refresh_per_second=0.001) as status:
        clock.now += 0.3
        status.update("Still working", spinner="line")
        clock.now += 0.3
        status.update(spinner_style="red", speed=2.0)
        c.print("done step")
    with m.status.Status("plain", console=c, spinner="bouncingBall",
                         refresh_per_second=0.001) as status:
        clock.now += 1
        status.update(status="changed")


def screen_context(m, c, clock):
    with c.screen(style="on blue") as screen:
        screen.update(m.panel.Panel("hi"))
        screen.update("second", style="on red")
    c.print(m.screen.Screen("a", "b"))
    c.print(m.screen.Screen("app", application_mode=True))


def pager_context(m, c, clock):
    shown = []

    class Recorder(m.pager.Pager):
        def show(self, content):
            shown.append(content)

    with c.pager(Recorder()):
        c.print("[bold red]paged[/] text")
    with c.pager(Recorder(), styles=True):
        c.print("[bold red]styled[/] text")
    with pytest.raises(ValueError):
        with c.pager(Recorder()):
            c.print("dropped")
            raise ValueError
    for content in shown:
        c.print(repr(content))


def prompts(m, c, clock):
    P = m.prompt
    c.print(repr(P.Prompt.ask("Name", console=c, stream=io.StringIO("Will\n"))))
    c.print(repr(P.Prompt.ask("Fruit", choices=["apple", "pear"], console=c,
                              stream=io.StringIO("kiwi\npear\n"))))
    c.print(repr(P.IntPrompt.ask("Number", default=5, console=c, stream=io.StringIO("x\n12\n"))))
    c.print(repr(P.FloatPrompt.ask("Float", console=c, stream=io.StringIO("nope\n1.5\n"))))
    c.print(repr(P.Confirm.ask("Continue", default=True, console=c,
                               stream=io.StringIO("maybe\nn\n"))))
    c.print(repr(P.Prompt.ask("Dog", choices=["Border", "Collie"], case_sensitive=False,
                              console=c, stream=io.StringIO("collie\n"))))
    c.print(repr(P.Prompt.ask("Hidden", default="d", show_default=False, show_choices=False,
                              choices=["d"], console=c, stream=io.StringIO("d\n"))))

    class AtLeastThree(P.Prompt):
        def process_response(self, value):
            value = super().process_response(value)
            if len(value) < 3:
                raise P.InvalidResponse("[red]too short")
            return value

    c.print(repr(AtLeastThree.ask("Long", console=c, stream=io.StringIO("ab\nabcd\n"))))


def rich_handler(m, c, clock):
    logger = logging.getLogger(f"rs_rich.tests.{m.logging.__name__}.{id(c)}")
    logger.propagate = False
    logger.setLevel(logging.DEBUG)
    handler = m.logging.RichHandler(console=c, enable_link_path=False)
    logger.addHandler(handler)
    records = [
        (logging.INFO, "GET /index.html 200 1298"),
        (logging.WARNING, "values {'x': 1} 3.5 True None"),
        (logging.ERROR, "error [bold]not markup[/]"),
        (logging.DEBUG, "a long message " * 8),
    ]
    for level, message in records:
        record = logger.makeRecord(logger.name, level, "/tmp/where/module.py", 42, message, (), None)
        record.created = 1_700_000_000.0
        logger.handle(record)
    quiet = m.logging.RichHandler(console=c, markup=True, show_path=False, show_time=False,
                                  keywords=["foo"])
    record = logger.makeRecord(logger.name, logging.CRITICAL, "/x.py", 7,
                               "[bold]markup[/] foo bar", (), None)
    quiet.handle(record)
    logger.removeHandler(handler)


PROGRAMS = [
    live_updates,
    live_transient,
    live_vertical_overflow,
    live_nested,
    live_get_renderable,
    live_screen,
    live_redirect,
    live_render_directly,
    progress_default,
    progress_every_column,
    progress_custom_columns,
    progress_subclass,
    progress_task_management,
    progress_printed,
    track_helpers,
    file_helpers,
    progress_bars,
    status_display,
    screen_context,
    pager_context,
    prompts,
    rich_handler,
]


def run_program(program, package: str, color: bool) -> str:
    m = modules(package)
    clock = Clock()
    c = console(m, color, clock)
    stdout, stderr = sys.stdout, sys.stderr
    try:
        program(m, c, clock)
    finally:
        sys.stdout, sys.stderr = stdout, stderr
    return c.file.getvalue()


@pytest.mark.parametrize("color", [True, False], ids=["truecolor", "plain"])
@pytest.mark.parametrize("program", PROGRAMS, ids=lambda p: p.__name__)
def test_output_matches_rich_byte_for_byte(program, color):
    expected = within_timeout(run_program, program, "rich", color)
    actual = within_timeout(run_program, program, "rs_rich", color)
    assert actual == expected


def test_the_final_frame_of_a_non_terminal_display():
    for package in ["rich", "rs_rich"]:
        m = modules(package)
        c = console(m, False, Clock())
        with m.live.Live("first", console=c, auto_refresh=False) as live:
            live.update("last")
        assert c.file.getvalue() == "last"


# ---------------------------------------------------------------------------
# The API


def test_module_paths():
    from rs_rich import _native

    for module, names in {
        "rs_rich.live": ["Live"],
        "rs_rich.live_render": ["LiveRender"],
        "rs_rich.progress": ["Progress", "Task", "TaskID", "track", "wrap_file", "open"],
        "rs_rich.progress_bar": ["ProgressBar"],
        "rs_rich.status": ["Status"],
        "rs_rich.screen": ["Screen"],
        "rs_rich.pager": ["Pager", "SystemPager"],
        "rs_rich.prompt": ["Prompt", "Confirm", "IntPrompt", "FloatPrompt", "InvalidResponse"],
        "rs_rich.logging": ["RichHandler"],
    }.items():
        loaded = importlib.import_module(module)
        for name in names:
            assert name in loaded.__all__
            assert getattr(loaded, name) is getattr(_native, name)


def test_rich_handler_is_a_logging_handler():
    from rs_rich.logging import RichHandler

    assert issubclass(RichHandler, logging.Handler)
    assert RichHandler.__module__ == "rs_rich.logging"


def test_invalid_response_carries_its_message():
    from rs_rich.prompt import InvalidResponse, PromptError

    error = InvalidResponse("[red]no")
    assert isinstance(error, PromptError)
    assert error.message == "[red]no"
    assert error.__rich__() == "[red]no"


def test_task_id_and_default_columns():
    from rs_rich import progress as P

    assert P.TaskID(3) == 3
    kinds = [type(column).__name__ for column in P.Progress.get_default_columns()]
    assert kinds == ["TextColumn", "BarColumn", "TaskProgressColumn", "TimeRemainingColumn"]
    assert P.TimeRemainingColumn.max_refresh == 0.5
    assert P.ProgressColumn.max_refresh is None


def test_task_values_keep_their_python_types():
    from rs_rich.console import Console
    from rs_rich.progress import Progress

    clock = Clock()
    progress = Progress(console=Console(file=io.StringIO()), get_time=clock, auto_refresh=False)
    task_id = progress.add_task("t", total=200, extra=1)
    progress.advance(task_id, 50)
    task = progress.tasks[0]
    assert (task.total, task.completed, task.remaining) == (200, 50, 150)
    assert type(task.completed) is int
    assert task.fields == {"extra": 1}
    assert "{task.completed}/{task.total}".format(task=task) == "50/200"
    with pytest.raises(KeyError):
        progress.update(99, advance=1)


def test_abstract_bases_raise():
    from rs_rich.pager import Pager
    from rs_rich.progress import ProgressColumn

    with pytest.raises(NotImplementedError):
        ProgressColumn().render(None)
    with pytest.raises(NotImplementedError):
        Pager().show("x")


def test_refresh_per_second_must_be_positive():
    from rs_rich.live import Live
    from rs_rich.progress import Progress

    with pytest.raises(AssertionError):
        Live(refresh_per_second=0)
    with pytest.raises(AssertionError):
        Progress(refresh_per_second=0)


def test_console_file_is_restored_after_a_display():
    from rs_rich.console import Console
    from rs_rich.live import Live

    out = io.StringIO()
    c = Console(file=out, force_terminal=True, width=20)
    stdout = sys.stdout
    with Live("x", console=c, auto_refresh=False):
        assert c.file.getvalue() is not None  # the wrapper passes attributes through
    assert c.file is out
    assert sys.stdout is stdout


# ---------------------------------------------------------------------------
# Threads


def test_auto_refresh_draws_from_a_thread_and_stops():
    from rs_rich.console import Console
    from rs_rich.live import Live

    def run():
        c = Console(file=io.StringIO(), force_terminal=True, color_system=None, width=30)
        before = threading.active_count()
        with Live("tick", console=c, refresh_per_second=100) as live:
            time.sleep(0.2)
            live.update("tock")
            time.sleep(0.1)
        deadline = time.monotonic() + 5
        while threading.active_count() > before and time.monotonic() < deadline:
            time.sleep(0.01)
        return c.file.getvalue(), threading.active_count() - before

    output, extra_threads = within_timeout(run)
    assert output.startswith("\x1b[?25ltick")
    assert output.count("\r\x1b[2Ktick") >= 3, output  # redrawn by the thread
    assert output.endswith("\r\x1b[2Ktock\n\x1b[?25h")
    assert extra_threads == 0


def test_threads_printing_during_a_progress_never_deadlock():
    from rs_rich.console import Console
    from rs_rich.progress import Progress

    def run():
        c = Console(file=io.StringIO(), force_terminal=True, color_system=None, width=60)
        with Progress(console=c, refresh_per_second=200) as progress:
            ids = [progress.add_task(f"worker {n}", total=40) for n in range(4)]

            def work(task_id):
                for step in range(40):
                    progress.advance(task_id)
                    if step % 10 == 0:
                        progress.console.print(f"worker {task_id} at {step}")
                    time.sleep(0.001)

            workers = [threading.Thread(target=work, args=(task_id,)) for task_id in ids]
            for worker in workers:
                worker.start()
            for worker in workers:
                worker.join()
        return progress.finished, c.file.getvalue()

    finished, output = within_timeout(run)
    assert finished
    assert output.count("worker 3 at 30") == 1
    assert output.endswith("\n\x1b[?25h")


def test_an_exception_inside_stops_the_display_and_restores_the_streams():
    from rs_rich.console import Console
    from rs_rich.live import Live

    def run():
        c = Console(file=io.StringIO(), force_terminal=True, color_system=None, width=30)
        stdout, stderr = sys.stdout, sys.stderr
        with pytest.raises(ValueError):
            with Live("x", console=c, refresh_per_second=100) as live:
                time.sleep(0.05)
                raise ValueError("boom")
        assert not live.is_started
        assert (sys.stdout, sys.stderr) == (stdout, stderr)
        return c.file.getvalue()

    assert within_timeout(run).endswith("\x1b[?25h")


@pytest.mark.filterwarnings("ignore::pytest.PytestUnhandledThreadExceptionWarning")
def test_a_failing_render_on_the_thread_still_lets_stop_finish():
    from rs_rich.console import Console
    from rs_rich.live import Live

    class FailsLater:
        renders = 0

        def __rich_console__(self, console, options):
            FailsLater.renders += 1
            if FailsLater.renders > 2:
                raise RuntimeError("render failed")
            yield "ok"

    def run():
        c = Console(file=io.StringIO(), force_terminal=True, color_system=None, width=30)
        stderr = sys.stderr
        live = Live(FailsLater(), console=c, refresh_per_second=100, redirect_stderr=False)
        sys.stderr = io.StringIO()  # the thread's traceback
        try:
            live.start(refresh=True)
            time.sleep(0.1)
            with pytest.raises(RuntimeError):
                live.stop()
        finally:
            sys.stderr = stderr
        assert not live.is_started
        return c.file.getvalue()

    assert within_timeout(run).endswith("\x1b[?25h")


def test_track_with_a_thread_counts_every_value():
    from rs_rich.console import Console
    from rs_rich.progress import Progress

    def run():
        c = Console(file=io.StringIO(), force_terminal=True, color_system=None, width=60)
        with Progress(console=c, refresh_per_second=100) as progress:
            seen = [value for value in progress.track(range(50), update_period=0.005)]
            time.sleep(0.02)
        return seen, progress.tasks[0].completed

    seen, completed = within_timeout(run)
    assert seen == list(range(50))
    assert completed == 50


def test_leaving_a_track_loop_early_stops_its_display():
    from rs_rich.console import Console
    from rs_rich.progress import track

    def run():
        c = Console(file=io.StringIO(), force_terminal=True, color_system=None, width=60)
        iterator = track(range(100), console=c, refresh_per_second=100, update_period=0.005)
        for value in iterator:
            if value == 10:
                break
        iterator.close()
        return c.file.getvalue()

    assert within_timeout(run).endswith("\x1b[?25h")


def test_a_running_display_does_not_hold_up_interpreter_exit():
    program = (
        "import io, time\n"
        "from rs_rich.console import Console\n"
        "from rs_rich.progress import Progress\n"
        "progress = Progress(console=Console(file=io.StringIO(), force_terminal=True))\n"
        "progress.start()\n"
        "progress.add_task('never stopped', total=None)\n"
        "time.sleep(0.2)\n"
    )
    result = subprocess.run(
        [sys.executable, "-c", program], capture_output=True, timeout=TIMEOUT, check=False
    )
    assert result.returncode == 0, result.stderr.decode()
