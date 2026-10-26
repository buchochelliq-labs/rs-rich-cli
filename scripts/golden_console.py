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

import json
import pathlib
import sys

from rich.console import Console
from rich.panel import Panel
from rich.rule import Rule
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
