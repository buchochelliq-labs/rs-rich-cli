"""Robustness regressions: Live, Progress, logging and prompts.

Each test pins a defect found in the 0.0.12 audit and states Rich 15.0.0's
behaviour. Hang- or crash-prone programs run in a subprocess with a timeout.
"""

from __future__ import annotations

import importlib
import io
import logging
import os
import subprocess
import sys
import textwrap

import pytest

TIMEOUT = 30.0


def run_program(source: str, package: str) -> subprocess.CompletedProcess:
    env = dict(os.environ, TERM="xterm-256color")
    env.pop("NO_COLOR", None)
    return subprocess.run(
        [sys.executable, "-c", textwrap.dedent(source).replace("PKG", package)],
        capture_output=True,
        timeout=TIMEOUT,
        check=False,
        env=env,
    )


# ---------------------------------------------------------------------------
# 1. A console writing to sys.stdout (file=None, the default and what
#    get_console(), track(), Progress() and Live() use) breaks as soon as a
#    Live redirects sys.stdout: rs_rich's console writes into the FileProxy
#    instead of unwrapping `rich_proxied_file` as Rich's `Console.file` does.

DEFAULT_CONSOLE_LIVE = """
    import sys
    from PKG.console import Console
    from PKG.live import Live
    console = Console(force_terminal=True, width=30, color_system=None)
    try:
        with Live("line one\\nline two", console=console, auto_refresh=False) as live:
            print("hello from print")
            live.update("updated\\nframe", refresh=True)
    except Exception as error:
        sys.__stderr__.write(f"{type(error).__name__}: {error}\\n")
"""


def test_live_on_a_default_console_prints_through_the_proxy():
    expected = run_program(DEFAULT_CONSOLE_LIVE, "rich")
    actual = run_program(DEFAULT_CONSOLE_LIVE, "rs_rich")
    assert expected.stderr == b""
    assert actual.stderr.decode() == ""
    assert actual.stdout == expected.stdout


def test_track_on_the_default_console_with_print_inside():
    # In a real terminal (a pty), the global console is a terminal.
    program = """
        from PKG.progress import track
        for value in track(range(3), description="Working", auto_refresh=False):
            print("item", value)
        print("done")
    """
    wrapped = """
        import os, pty, sys
        pid, fd = pty.fork()
        if pid == 0:
            os.execv(sys.executable, [sys.executable, "-c", PROGRAM])
        out = b""
        while True:
            try:
                chunk = os.read(fd, 65536)
            except OSError:
                break
            if not chunk:
                break
            out += chunk
        _, status = os.waitpid(pid, 0)
        print(os.waitstatus_to_exitcode(status))
        print(b"RuntimeError" in out)
    """.replace("PROGRAM", repr(textwrap.dedent(program)))
    for package in ["rich", "rs_rich"]:
        result = run_program(wrapped, package)
        assert result.stdout.decode().split() == ["0", "False"], (package, result.stdout)


# ---------------------------------------------------------------------------
# 2. Leaving a Live / Progress / track / status right before the interpreter
#    exits crashes the process (SIGABRT "FATAL: exception not rethrown" or
#    SIGSEGV): stop() only sets the refresh thread's event and drops it from
#    the atexit registry, so the thread (a pyo3 closure) is still waking up
#    when finalization pthread_exit()s it through Rust frames.

EXIT_PROGRAMS = {
    "live": """
        import io, time
        from PKG.console import Console
        from PKG.live import Live
        with Live("x", console=Console(file=io.StringIO(), force_terminal=True),
                  refresh_per_second=100):
            time.sleep(0.05)
    """,
    "track": """
        import io, time
        from PKG.console import Console
        from PKG.progress import track
        for _ in track(range(5), console=Console(file=io.StringIO(), force_terminal=True)):
            time.sleep(0.01)
    """,
    "status": """
        import io, time
        from PKG.console import Console
        console = Console(file=io.StringIO(), force_terminal=True)
        with console.status("working"):
            time.sleep(0.05)
    """,
}


@pytest.mark.parametrize("name", sorted(EXIT_PROGRAMS))
def test_a_stopped_display_does_not_crash_interpreter_exit(name):
    for package in ["rich", "rs_rich"]:
        codes = [run_program(EXIT_PROGRAMS[name], package).returncode for _ in range(8)]
        assert codes == [0] * 8, (package, codes)


# ---------------------------------------------------------------------------
# 3. Column arithmetic: Python ints are unbounded, rs_rich's columns go
#    through i64 and raise OverflowError past 2**63 (MofNCompleteColumn,
#    DownloadColumn, FileSizeColumn, TotalFileSizeColumn,
#    TransferSpeedColumn, TimeElapsedColumn).


def render_columns(package: str, total, completed) -> str:
    progress = importlib.import_module(f"{package}.progress")
    console = importlib.import_module(f"{package}.console")
    c = console.Console(file=io.StringIO(), width=120, color_system=None)
    p = progress.Progress(
        progress.MofNCompleteColumn(),
        progress.DownloadColumn(),
        progress.FileSizeColumn(),
        progress.TotalFileSizeColumn(),
        console=c,
    )
    p.add_task("big", total=total, completed=completed)
    c.print(p)
    return c.file.getvalue()


@pytest.mark.parametrize("total,completed", [(2**70, 2**69), (10**30, 10**29), (1e300, 1e299)])
def test_columns_render_integers_past_i64(total, completed):
    assert render_columns("rs_rich", total, completed) == render_columns("rich", total, completed)


def test_transfer_speed_past_i64():
    def run(package):
        progress = importlib.import_module(f"{package}.progress")
        console = importlib.import_module(f"{package}.console")
        now = [0.0]

        def get_time():
            return now[0]

        c = console.Console(file=io.StringIO(), width=120, color_system=None)
        p = progress.Progress(progress.TransferSpeedColumn(), console=c, get_time=get_time)
        task = p.add_task("x", total=None)
        now[0] = 1.0
        p.advance(task, 10**40)
        now[0] = 2.0
        p.advance(task, 10**40)
        c.print(p)
        return c.file.getvalue()

    assert run("rs_rich") == run("rich")


# ---------------------------------------------------------------------------
# 4. Task.percentage: Rich clamps with min(100.0, max(0.0, x)), which maps
#    -0.0 and NaN to 0.0; f64::clamp keeps them ("-0%" / "nan%").


@pytest.mark.parametrize("total,completed", [(-1, 0), (100, float("nan"))])
def test_percentage_clamps_like_python(total, completed):
    def run(package):
        progress = importlib.import_module(f"{package}.progress")
        console = importlib.import_module(f"{package}.console")
        c = console.Console(file=io.StringIO(), width=80, color_system=None)
        p = progress.Progress(console=c)
        p.add_task("x", total=total, completed=completed)
        c.print(p)
        return repr(p.tasks[0].percentage), c.file.getvalue()

    assert run("rs_rich") == run("rich")


# ---------------------------------------------------------------------------
# 5. RichHandler's path link: Rich builds it as the style string
#    "link file://<path>", which fails to parse for a path with whitespace,
#    so Rich shows no link; rs_rich writes an OSC 8 link with raw spaces.


def test_rich_handler_path_with_spaces_has_no_link():
    def run(package):
        rich_logging = importlib.import_module(f"{package}.logging")
        console = importlib.import_module(f"{package}.console")
        c = console.Console(file=io.StringIO(), width=80, force_terminal=True,
                            color_system="truecolor")
        handler = rich_logging.RichHandler(console=c, show_time=False)
        record = logging.LogRecord("x", logging.INFO, "/tmp/my dir/some file.py", 42,
                                   "message", (), None)
        handler.handle(record)
        return c.file.getvalue()

    assert "\x1b]8;" not in run("rich")
    assert "\x1b]8;" not in run("rs_rich")
