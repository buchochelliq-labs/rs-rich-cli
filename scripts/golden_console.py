"""Parity cases for the rest of `rich/console.py` (0.0.23 console workstream).

One fixture, `crates/rich/tests/golden/console.tsv`: `name<TAB>json(output)`.
Each case is a function returning the exact string upstream produces;
`crates/rich/tests/golden_console.rs` has a Rust builder per name.

Kept out of `capture_golden.py` so the workstreams do not edit the same lines;
`capture_golden.main` calls `capture_console`. Every console here is truecolor,
the only colour system used in this process (rich memoises a style's escape
codes on the instance). Environment detection goes through upstream's own
injection point, `Console(_environ=…)`, so no case reads the real environment.
"""

from __future__ import annotations

import io
import json
from datetime import datetime
import pathlib
import re
import sys
import tempfile

from rich.console import Console, RenderHook
from rich.control import Control
from rich.terminal_theme import MONOKAI, NIGHT_OWLISH
from rich.panel import Panel
from rich.rule import Rule
from rich.table import Table
from rich.text import Text


def _console(width: int = 20, **overrides) -> Console:
    options = dict(
        force_terminal=True,
        color_system="truecolor",
        width=width,
        highlight=False,
        safe_box=False,
        legacy_windows=False,
        no_color=False,
    )
    options.update(overrides)
    return Console(**options)


def _print(console: Console, *objects, **kwargs) -> str:
    with console.capture() as capture:
        console.print(*objects, **kwargs)
    return capture.get()


def _lines(*values) -> str:
    return "".join(f"{value}\n" for value in values)


def _size(console: Console) -> str:
    return f"{console.size.width}x{console.size.height}"


# --- group 1: quiet, sizes, dumb terminals, soft_wrap, style, detection ----


def _console_quiet() -> str:
    console = _console(quiet=True)
    out = [repr(console.quiet), _print(console, "[bold]hidden[/]")]
    console.quiet = False
    out.append(_print(console, "[bold]shown[/]"))
    return _lines(*out)


def _console_size_setters() -> str:
    console = _console(30, height=10)
    out = [_size(console)]
    console.width = 12
    out.append(_size(console))
    console.height = 4
    out.append(_size(console))
    console.size = (16, 3)
    out.append(_size(console))
    out.append(_print(console, Rule("x")))
    return _lines(*out)


def _console_size_environ() -> str:
    # Every case decides both dimensions, so the real terminal never shows.
    out = []
    for width, height, environ, legacy in [
        (30, 10, {}, True),
        (None, None, {"COLUMNS": "50", "LINES": "7"}, False),
        # `int(columns) - legacy_windows`, then `size` subtracts it again.
        (None, None, {"COLUMNS": "50", "LINES": "7"}, True),
        (None, 9, {"COLUMNS": "50"}, True),
        (33, None, {"COLUMNS": "5 0", "LINES": "7"}, False),
        (None, None, {"TERM": "dumb", "COLUMNS": "50"}, False),
        (None, None, {"TERM": "dumb", "COLUMNS": "50", "LINES": "9"}, False),
        (40, None, {"TERM": "unknown"}, False),
    ]:
        console = Console(
            force_terminal=True,
            width=width,
            height=height,
            legacy_windows=legacy,
            _environ=environ,
        )
        out.append(_size(console))
    return _lines(*out)


def _console_dumb_terminal() -> str:
    out = []
    for terminal, environ in [
        (True, {"TERM": "dumb"}),
        (True, {"TERM": "Dumb"}),
        (True, {"TERM": "unknown"}),
        (True, {"TERM": " dumb"}),
        (True, {"TERM": "xterm"}),
        (True, {}),
        (False, {"TERM": "dumb"}),
    ]:
        console = Console(force_terminal=terminal, _environ=environ)
        out.append(f"{console.is_dumb_terminal} {console.color_system}")
    return _lines(*out)


def _console_color_detection() -> str:
    out = []
    for environ in [
        {"TERM": "xterm-kitty"},
        {"TERM": "kitty"},
        {"TERM": "xterm-256"},
        {"TERM": "xterm-256color"},
        {"TERM": " XTERM-256COLOR "},
        {"TERM": "screen-16color"},
        {"TERM": "xterm"},
        {"COLORTERM": "truecolor"},
        {"COLORTERM": " TrueColor ", "TERM": "xterm-256color"},
        {"COLORTERM": "24bit"},
        {"COLORTERM": "truecolor-ish"},
        {"COLORTERM": "yes", "TERM": "xterm-256color"},
        {"TERM": "dumb", "COLORTERM": "truecolor"},
        {},
    ]:
        console = Console(force_terminal=True, legacy_windows=False, _environ=environ)
        out.append(str(console.color_system))
    # An explicit system, or None, is taken as given.
    out.append(str(Console(force_terminal=True, color_system="256", _environ={"TERM": "dumb"}).color_system))
    out.append(str(Console(force_terminal=True, color_system=None, _environ={"COLORTERM": "truecolor"}).color_system))
    # Not a terminal: no colour.
    out.append(str(Console(force_terminal=False, _environ={"COLORTERM": "truecolor"}).color_system))
    return _lines(*out)


def _console_terminal_detection() -> str:
    out = []
    for environ in [
        {"TTY_COMPATIBLE": "1"},
        {"TTY_COMPATIBLE": "0", "FORCE_COLOR": "1"},
        {"TTY_COMPATIBLE": "1", "FORCE_COLOR": ""},
        {"FORCE_COLOR": ""},
        {"FORCE_COLOR": "0"},
        {"FORCE_COLOR": "1", "TERM": "xterm-256color"},
    ]:
        console = Console(legacy_windows=False, _environ=environ)
        out.append(f"{console.is_terminal} {console.color_system}")
    # `force_terminal` wins over the environment.
    console = Console(force_terminal=False, legacy_windows=False, _environ={"TTY_COMPATIBLE": "1"})
    out.append(f"{console.is_terminal} {console.color_system}")
    return _lines(*out)


def _console_no_color_environ() -> str:
    out = []
    for environ in [{"NO_COLOR": "1"}, {"NO_COLOR": ""}, {}]:
        console = Console(force_terminal=True, color_system="truecolor", _environ=environ)
        out.append(f"{console.no_color} {_print(console, '[red]r[/] [bold]b[/]')!r}")
    out.append(str(Console(no_color=False, _environ={"NO_COLOR": "1"}).no_color))
    return _lines(*out)


_LONG = "aaaa bbbb cccc dddd eeee ffff gggg"


def _console_soft_wrap() -> str:
    console = _console(12, soft_wrap=True)
    return "".join(
        [
            repr(console.soft_wrap) + "\n",
            _print(console, _LONG),
            _print(console, _LONG, soft_wrap=False),
            _print(console, Panel("hi there", width=20)),
            _print(_console(12), _LONG, soft_wrap=True),
        ]
    )


def _console_style() -> str:
    console = _console(12, style="bold")
    out = [
        _print(console, "hi"),
        _print(console, Panel("x", expand=False)),
        _print(console, "a", "b", sep="-", end="!\n"),
        _print(console, _LONG),
        _print(console, "c", style="italic"),
        _print(console, "mid", justify="center"),
        _print(console, Text("tx", style="red")),
    ]
    named = _console(12, style="warning")
    from rich.theme import Theme

    named.push_theme(Theme({"warning": "red on white"}))
    out.append(_print(named, "w"))
    return "".join(out)


def _console_style_soft_wrap() -> str:
    console = _console(12, style="bold", soft_wrap=True)
    return "".join([_print(console, "hi"), _print(console, _LONG), _print(console, Panel("x", expand=False))])


# --- group 2: begin_capture / end_capture ---------------------------------


def _console_begin_end_capture() -> str:
    console = _console(20)
    console.begin_capture()
    console.print("[bold]a[/]")
    console.rule("r")
    first = console.end_capture()
    # Nested: upstream keeps one buffer per thread, so the inner end takes
    # everything and the outer one finds nothing left.
    console.begin_capture()
    console.print("a")
    console.begin_capture()
    console.print("b")
    inner = console.end_capture()
    outer = console.end_capture()
    # Control codes are captured (a terminal), and `end=""` leaves the line open.
    console.begin_capture()
    console.show_cursor(False)
    console.print("z", end="")
    control = console.end_capture()
    quiet = _console(20, quiet=True)
    quiet.begin_capture()
    quiet.print("q")
    hushed = quiet.end_capture()
    return _lines(repr(first), repr(inner), repr(outer), repr(control), repr(hushed))


# --- group 3: record=, export_* and save_* -------------------------------


def _recording(width: int = 20, **overrides) -> Console:
    """A recording console that writes to a throwaway file, so a capture run
    stays quiet. The Rust side writes to stdout, which the record ignores."""
    return _console(width, record=True, file=io.StringIO(), **overrides)


def _record_session(console: Console, control: bool = True) -> None:
    console.print("[bold red]hi[/] there", "[link=https://x.io/?a=1&b=2]<&>[/]")
    console.print(Panel("boxed", expand=False))
    # Upstream's `Segment.simplify` merges a control code into the text after
    # it, so its HTML shows the raw escape; the port keeps them apart
    # (DIVERGENCES §44). The HTML sessions leave control codes out.
    if control:
        console.show_cursor(False)
    console.print("open", end="")
    console.print()
    # Neither a capture nor a quiet console is recorded.
    with console.capture():
        console.print("captured")
    console.quiet = True
    console.print("hushed")
    console.quiet = False
    console.print("[on blue]last[/]", style="italic")


def _no_link_id(text: str) -> str:
    """Drop the random `id=` upstream puts in hyperlinks (DIVERGENCES §20)."""
    return re.sub(r"\x1b\]8;id=\d+;", "\x1b]8;;", text)


def _console_record_text() -> str:
    console = _recording()
    _record_session(console)
    out = [
        repr(console.export_text(clear=False)),
        repr(_no_link_id(console.export_text(clear=False, styles=True))),
        repr(console.export_text()),
        repr(console.export_text()),
    ]
    # Off a terminal, control codes are still recorded: kept with styles.
    plain = _recording(force_terminal=False)
    plain.control(Control.bell())
    plain.print("x")
    out.append(repr(plain.export_text(clear=False)))
    out.append(repr(plain.export_text(styles=True)))
    try:
        Console(file=io.StringIO()).export_text()
    except AssertionError as error:
        out.append(str(error))
    return _lines(*out)


def _console_record_html() -> str:
    console = _recording()
    _record_session(console, control=False)
    return "\n".join(
        [
            console.export_html(clear=False),
            console.export_html(theme=MONOKAI, inline_styles=True, clear=False),
            console.export_html(code_format="{foreground}|{background}|{stylesheet}|{code}"),
            console.export_html(),
        ]
    )


_SVG_FORMAT = (
    "{unique_id}|{char_width}|{char_height}|{line_height}|{terminal_width}|{terminal_height}|"
    "{width}|{height}|{terminal_x}|{terminal_y}|{{lit}}\n{styles}\n{matrix}\n{backgrounds}\n{lines}\n{chrome}"
)


def _console_record_svg() -> str:
    console = _recording()
    _record_session(console)
    return "\n".join(
        [
            console.export_svg(unique_id="u", clear=False),
            console.export_svg(title="T", theme=NIGHT_OWLISH, unique_id="v", clear=False),
            console.export_svg(code_format=_SVG_FORMAT, font_aspect_ratio=0.5, unique_id="w"),
            console.export_svg(unique_id="x"),
        ]
    )


def _console_record_save() -> str:
    console = _recording()
    _record_session(console, control=False)
    with tempfile.TemporaryDirectory() as directory:
        root = pathlib.Path(directory)
        console.save_text(root / "a.txt", clear=False)
        console.save_text(root / "b.txt", styles=True, clear=False)
        console.save_html(root / "c.html", inline_styles=True, clear=False)
        console.save_svg(root / "d.svg", title="S", unique_id="s")
        console.save_text(root / "e.txt")
        return "\n".join(
            _no_link_id((root / name).read_text(encoding="utf-8"))
            for name in ["a.txt", "b.txt", "c.html", "d.svg", "e.txt"]
        )


# --- group 4: log ----------------------------------------------------------


def _logging(width: int = 40, caller=("/src/app.py", 7), times=None, **overrides) -> Console:
    """A console whose `log` reads a fixed clock (`get_datetime`) and names a
    fixed caller (upstream's `_caller_frame_info`)."""
    times = iter(times or [datetime(2024, 1, 2, 3, 4, 5, 678901)] * 20)
    console = _console(width, get_datetime=lambda: next(times), **overrides)
    console._caller_frame_info = lambda offset, currentframe=None: (*caller, {})
    return console


def _log(console: Console, *objects, **kwargs) -> str:
    with console.capture() as capture:
        console.log(*objects, **kwargs)
    return _no_link_id(capture.get())


def _log_table() -> Table:
    # A renderable in a log shares the message column, as a table cell does;
    # the port's cells need `Send + Sync` renderables, which a `Table` is
    # and a `Panel` (holding any child) is not.
    table = Table("a", "b")
    table.add_row("1", "2")
    return table


def _console_log() -> str:
    second = datetime(2024, 1, 2, 3, 4, 6)
    first = datetime(2024, 1, 2, 3, 4, 5, 678901)
    console = _logging(times=[first, first, second, second, second, second, second, second, second])
    return "".join(
        [
            _log(console, "hello [bold]world[/]"),
            _log(console, "same time", _log_table(), "after"),
            _log(console, "a new second, a long message that folds over several lines"),
            _log(console),
            _log(console, "a", "b", sep="-", end="!"),
            _log(console, "two", end="\n\n"),
            _log(console, "x", end="x\ny"),
            _log(console, "mid", justify="center"),
            _log(console, "st", style="bold"),
        ]
    )


def _console_log_options() -> str:
    out = [
        _log(_logging(log_time=False), "no time"),
        _log(_logging(log_path=False), "no path"),
        _log(_logging(log_time=False, log_path=False), "neither"),
        _log(_logging(caller=("<stdin>", 3)), "pseudo file"),
        _log(_logging(caller=("/a/b/deep.py", 1234)), "deep"),
        _log(_logging(style="italic"), "console style", _log_table()),
        _log(_logging(highlight=True), "x = 1, y = [True, None]"),
        _log(_logging(highlight=True), "plain 1", highlight=False),
        _log(_logging(20), "a narrow console wraps the message"),
    ]
    return "".join(out)


def _console_log_time_format() -> str:
    out = [
        _log(
            _logging(60, log_time_format="%Y-%m-%d %H:%M:%S.%f %j %a %A %b %B", log_path=False),
            "x",
        ),
        _log(_logging(60, log_time_format="%p %I %y %c|%x %X|%D %T %F", log_path=False), "x"),
        _log(_logging(60, log_time_format="%w %u %U %W %e %C %R %r %h %% %z%Z %Q", log_path=False), "x"),
        _log(
            _logging(
                60,
                log_time_format=lambda dt: Text(dt.strftime("%H:%M:%S.%f"), style="red"),
                caller=("<stdin>", 3),
            ),
            "callable",
        ),
    ]
    for when in [
        datetime(1999, 12, 31, 23, 59, 59),
        datetime(2000, 2, 29, 12, 0, 0),
        datetime(2024, 12, 30, 0, 0, 1),
        datetime(1969, 7, 20, 20, 17, 40),
    ]:
        out.append(_log(_logging(60, times=[when], log_time_format="%a %d %b %Y %H:%M:%S %j %U %W %w %p", log_path=False), "d"))
    return "".join(out)


# --- group 5: render hooks, live stack, screen(), status() ----------------


class _Around(RenderHook):
    """Adds a text before and after what is printed."""

    def __init__(self, before: str, after: str) -> None:
        self.before, self.after = before, after

    def process_renderables(self, renderables):
        return [Text(self.before), *renderables, Text(self.after)]


class _Drop(RenderHook):
    def process_renderables(self, renderables):
        return []


class _Bell(RenderHook):
    """Adds a control code, as upstream's live display adds a cursor move."""

    def process_renderables(self, renderables):
        return [Control.bell(), *renderables]


def _console_render_hooks() -> str:
    console = _logging(16)
    out = [_print(console, "plain")]
    console.push_render_hook(_Around("<<", ">>"))
    out.append(_print(console, "hooked"))
    out.append(_print(console, "a", "b", Panel("p", expand=False), "c"))
    out.append(_print(console, "styled", style="bold"))
    out.append(_print(console, "open", end=""))
    out.append(_print(console))
    with console.capture() as capture:
        console.out("out", "put")
    out.append(capture.get())
    out.append(_log(console, "logged"))
    console.push_render_hook(_Bell())
    out.append(_print(console, "two hooks"))
    console.pop_render_hook()
    console.pop_render_hook()
    out.append(_print(console, "popped"))
    console.push_render_hook(_Drop())
    out.append(_print(console, "dropped"))
    console.pop_render_hook()
    soft = _console(16, soft_wrap=True, style="italic")
    soft.push_render_hook(_Around("[", "]"))
    out.append(_print(soft, "soft wrapped and styled"))
    return "".join(repr(piece) + "\n" for piece in out)


def _console_live_stack() -> str:
    console = _console()
    out = [console.set_live(object()), console.set_live(object())]
    console.clear_live()
    out.append(console.set_live(object()))
    console.clear_live()
    console.clear_live()
    out.append(console.set_live(object()))
    return _lines(*out)


def _console_screen() -> str:
    console = _console(6, height=3)
    out = []
    with console.capture() as capture:
        with console.screen(style="on blue") as screen:
            screen.update(Text("hi"))
            screen.update(style="on red")
            screen.update("a", "b")
    out.append(capture.get())
    with console.capture() as capture:
        with console.screen(hide_cursor=False) as screen:
            screen.update("x")
    out.append(capture.get())
    plain = _console(6, height=2, force_terminal=False, color_system=None)
    with plain.capture() as capture:
        with plain.screen() as screen:
            screen.update("not a terminal")
    out.append(capture.get())
    return "".join(repr(piece) + "\n" for piece in out)


def _console_status() -> str:
    now = [0.0]
    buffer = io.StringIO()
    console = _console(30, file=buffer, get_time=lambda: now[0])
    status = console.status("Working", spinner="line", spinner_style="green", speed=2.0)
    status._live.auto_refresh = False
    status.start()
    status._live.refresh()
    now[0] = 0.13
    status._live.refresh()
    status.update("Next", spinner="dots")
    status._live.refresh()
    status.stop()
    return buffer.getvalue()


# --- group 6: Progress.log (0.0.23) ---------------------------------------


def _progress_log() -> str:
    """`Progress.log` is `console.log`: through the live display's render
    hook, above the tasks, with the time repeated only when it changes."""
    from rich.progress import BarColumn, Progress, TextColumn

    buffer = io.StringIO()
    second = datetime(2024, 1, 2, 3, 4, 6)
    console = _logging(40, times=[datetime(2024, 1, 2, 3, 4, 5)] + [second] * 4, file=buffer)
    progress = Progress(
        TextColumn("{task.description}"),
        BarColumn(bar_width=10),
        console=console,
        auto_refresh=False,
        get_time=lambda: 0.0,
    )
    progress.start()
    task = progress.add_task("Copying", total=4)
    progress.log("starting [bold]copy[/]")
    progress.advance(task, 2)
    progress.refresh()
    progress.log("halfway", _log_table())
    progress.log("same second", style="italic")
    progress.advance(task, 2)
    progress.stop()
    # A disabled progress logs straight to the console.
    off = Progress(console=console, disable=True)
    off.start()
    off.log("disabled")
    off.stop()
    return _no_link_id(buffer.getvalue())


def _progress_log_file() -> str:
    """Not a terminal: the log is written as it is, the tasks at the end."""
    from rich.progress import BarColumn, Progress, TextColumn

    buffer = io.StringIO()
    console = _logging(40, file=buffer, force_terminal=False, color_system=None)
    progress = Progress(
        TextColumn("{task.description}"),
        BarColumn(bar_width=10),
        console=console,
        auto_refresh=False,
        get_time=lambda: 0.0,
    )
    progress.start()
    progress.add_task("Copying", total=4, completed=1)
    progress.log("into a file")
    progress.stop()
    return _no_link_id(buffer.getvalue())


CONSOLE_CASES = [
    ("console_quiet", _console_quiet),
    ("console_size_setters", _console_size_setters),
    ("console_size_environ", _console_size_environ),
    ("console_dumb_terminal", _console_dumb_terminal),
    ("console_color_detection", _console_color_detection),
    ("console_terminal_detection", _console_terminal_detection),
    ("console_no_color_environ", _console_no_color_environ),
    ("console_soft_wrap", _console_soft_wrap),
    ("console_style", _console_style),
    ("console_style_soft_wrap", _console_style_soft_wrap),
    ("console_begin_end_capture", _console_begin_end_capture),
    ("console_record_text", _console_record_text),
    ("console_record_html", _console_record_html),
    ("console_record_svg", _console_record_svg),
    ("console_record_save", _console_record_save),
    ("console_log", _console_log),
    ("console_log_options", _console_log_options),
    ("console_log_time_format", _console_log_time_format),
    ("console_render_hooks", _console_render_hooks),
    ("console_live_stack", _console_live_stack),
    ("console_screen", _console_screen),
    ("console_status", _console_status),
    ("progress_log", _progress_log),
    ("progress_log_file", _progress_log_file),
]


def capture_console(golden_dir: pathlib.Path) -> None:
    path = golden_dir / "console.tsv"
    lines = ["# name\tjson(expected output) — see scripts/golden_console.py"]
    for name, build in CONSOLE_CASES:
        lines.append(f"{name}\t{json.dumps(build(), ensure_ascii=False)}")
    path.write_text("\n".join(lines) + "\n", encoding="utf-8", newline="\n")
    print(f"wrote {len(CONSOLE_CASES)} console cases to {path}")


if __name__ == "__main__":
    sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
    from capture_golden import golden_dir, verify_upstream_version

    verify_upstream_version()
    capture_console(golden_dir())
