"""Shared plumbing for the docs scripts in this directory; not a script itself.

Each `<page>.py` here renders the examples of one page of the Python docs
site (`docs/python/<page>.md`; `ext_tables.py` is `ext/tables.md`) and writes
one SVG per example:

    python crates/rich-py/examples/docs/table.py [DIR]

DIR defaults to `docs/python/media`. `scripts/capture_python_docs.py` runs
every script and, with `--check`, fails when a checked-in picture is stale.

A script imports this module first:

    from _shots import Shots

Then each example is a function taking the console it prints on, with the
code the page shows between `# --8<-- [start:NAME]` and `# --8<-- [end:NAME]`
markers. `Shots.shot(NAME, function, width=...)` renders it on a pinned
console (fixed width and height, truecolor, a clean environment) and exports
`<page>-<NAME>.svg`, using that file stem as the SVG's title and unique id so
the same input gives the same bytes. While it runs, whatever the example
`print()`s goes into the picture too, as the page's reader would see it.

`Shots.finish()` checks that every marked excerpt appears, line for line, in
a code block on the page, and that the page shows every picture, so the code
on the page is the code that made the picture.
"""

import contextlib
import io
import linecache
import os
import pathlib
import re
import sys
import textwrap
import unicodedata

HERE = pathlib.Path(__file__).resolve().parent

# A script named after its page (logging.py, traceback.py) must not shadow
# the standard library module of the same name when rs_rich imports it.
sys.path[:] = [entry for entry in sys.path if pathlib.Path(entry or ".").resolve() != HERE]

# Pictures must not depend on the terminal they were made in.
for _name in list(os.environ):
    if _name in {"COLUMNS", "LINES", "NO_COLOR", "FORCE_COLOR", "CLICOLOR", "CLICOLOR_FORCE",
                 "COLORTERM", "TERM_PROGRAM", "TERM_PROGRAM_VERSION", "TTY_COMPATIBLE",
                 "TTY_INTERACTIVE", "KITTY_WINDOW_ID", "WT_SESSION", "CI", "GITHUB_ACTIONS"} \
            or _name.startswith("RICH_"):
        del os.environ[_name]
os.environ["TERM"] = "xterm-256color"

from rs_rich.console import Console  # noqa: E402
from rs_rich.text import Text  # noqa: E402

ROOT = HERE.parents[3]
DOCS = ROOT / "docs" / "python"
MEDIA = DOCS / "media"
START = re.compile(r"^\s*# --8<-- \[start:([\w-]+)\]\s*$")
END = re.compile(r"^\s*# --8<-- \[end:([\w-]+)\]\s*$")
MIN_WIDTH = 32
FENCE = re.compile(r"^(\s*)```[^\n]*\n(.*?)^\1```[ \t]*$", re.MULTILINE | re.DOTALL)


class Shots:
    """Where to write one page's pictures, and the page they belong to."""

    def __init__(self, page):
        self.page = page
        self.prefix = page.replace("/", "-")
        args = sys.argv[1:]
        self.out = pathlib.Path(args[0]) if args else MEDIA
        self.out.mkdir(parents=True, exist_ok=True)
        self.markdown = (DOCS / f"{page}.md").read_text(encoding="utf-8")
        self.script = pathlib.Path(sys.modules["__main__"].__file__).resolve()
        self.errors = []
        self.names = []

    def shot(self, name, example, *args, width=60, height=25, terminal=True, stdout=True,
             **options):
        """Render `example(console, *args)`, write `<page>-<name>.svg`, and
        return what the example returned (for the examples that follow it).

        `terminal=False` renders on a console that is not a terminal (still in
        colour), where a live display writes only its final frame.
        `stdout=False` leaves what the example prints out of the picture (the
        page shows it as text). Other keyword arguments go to `Console`.
        """
        stem = f"{self.prefix}-{name}"
        console = Console(file=io.StringIO(), record=True, width=width, height=height,
                          force_terminal=terminal, color_system="truecolor", no_color=False,
                          legacy_windows=False, **options)
        echo = _Echo(console if stdout else None)
        with contextlib.redirect_stdout(echo):
            result = example(console, *args)
        echo.close()
        for number, line in enumerate(console.export_text(clear=False).splitlines(), 1):
            if Text(line).cell_len > width:
                self.errors.append(f"{stem}: line {number} is wider than {width} cells "
                                   "and would be cut off in the picture")
        if width < MIN_WIDTH:
            # The window's title bar needs room: show a narrow example, as it
            # rendered, in a wider window.
            recorded = Text.from_ansi(console.export_text(styles=True).removesuffix("\n"))
            console = Console(file=io.StringIO(), record=True, width=MIN_WIDTH,
                              force_terminal=True, color_system="truecolor", no_color=False,
                              legacy_windows=False)
            console.print(recorded, no_wrap=True, overflow="ignore", crop=False)
        svg = console.export_svg(title=stem, unique_id=stem)
        path = self.out / f"{stem}.svg"
        with open(path, "w", encoding="utf-8", newline="\n") as file:
            file.write(svg)
        print(f"wrote {path}", file=sys.stderr)
        self.names.append(name)
        if f"media/{stem}.svg" not in self.markdown:
            self.errors.append(f"{self.page}.md does not show media/{stem}.svg")
        return result

    def finish(self):
        """Check the excerpts against the page; exit non-zero on a mismatch."""
        blocks = [textwrap.dedent(body).splitlines() for _, body in FENCE.findall(self.markdown)]
        for name, lines in excerpts(self.script.read_text(encoding="utf-8")).items():
            if not any(_contains(block, lines) for block in blocks):
                self.errors.append(
                    f"{self.page}.md has no code block containing the excerpt {name!r} of "
                    f"{self.script.relative_to(ROOT).as_posix()}"
                )
        for error in self.errors:
            print(f"error: {error}", file=sys.stderr)
        sys.exit(1 if self.errors else 0)


def stable_file(function, name="/docs/example.py"):
    """`function`, with its code (nested functions too) claiming to come from
    `name`, a file that does not exist, so a traceback through it is the same
    on every machine. `linecache` holds the script's source under that name,
    so the traceback still shows the code."""
    source = pathlib.Path(function.__code__.co_filename).read_text(encoding="utf-8")
    linecache.cache[name] = (len(source), None, source.splitlines(keepends=True), name)

    def renamed(code):
        consts = tuple(renamed(c) if hasattr(c, "co_filename") else c for c in code.co_consts)
        return code.replace(co_filename=name, co_consts=consts)

    function.__code__ = renamed(function.__code__)
    return function


def excerpts(source):
    """{name: dedented lines} for every marked section of a script."""
    found, open_ = {}, {}
    for line in source.splitlines():
        if match := START.match(line):
            open_[match[1]] = []
        elif match := END.match(line):
            found[match[1]] = textwrap.dedent("\n".join(open_.pop(match[1]))).splitlines()
        else:
            for lines in open_.values():
                lines.append(line)
    if open_:
        raise SystemExit(f"unclosed excerpt(s): {', '.join(open_)}")
    return found


def _contains(block, lines):
    """Whether `lines` run, in order and unbroken, through `block`.

    Blank lines do not count: a script may space its code as PEP 8 asks.
    """
    block = [line.rstrip() for line in block if line.strip()]
    lines = [line.rstrip() for line in lines if line.strip()]
    return any(block[i:i + len(lines)] == lines for i in range(len(block) - len(lines) + 1))


class _Echo(io.TextIOBase):
    """A stdout that prints each line onto the recording console, as is."""

    def __init__(self, console):
        self.console = console
        self.pending = ""

    def writable(self):
        return True

    def write(self, text):
        self.pending += text
        *lines, self.pending = self.pending.split("\n")
        for line in lines:
            if self.console is not None:
                self.console.out(line, highlight=False)
        return len(text)

    def close(self):
        if self.pending and self.console is not None:
            self.console.out(self.pending, highlight=False)
            self.pending = ""


CSI = re.compile(r"\x1b\[([?0-9;]*)([@-~])")
OSC = re.compile(r"\x1b\][^\x07\x1b]*(?:\x07|\x1b\\)")


def painted(record, frame=-1, width=80):
    """Frame `frame` of a headless run (`record.frames[frame]`), in colour.

    A run's frames are plain text; this replays the bytes it wrote up to that
    frame through `screen`, so the picture has the component's styles.
    """
    if frame < 0:
        frame += len(record.frames)
    return screen("".join(record.writes[:frame + 1]), width)


def screen(output, width):
    """What a terminal `width` columns wide shows after `output`, as a `Text`.

    Enough of a VT emulator for the paints of `rs_rich.interact` and
    `rs_rich.tui`: text, carriage return, line feed, cursor moves, erasing and
    SGR styles. Anything else is ignored.
    """
    rows, row, col, sgr = [[]], 0, 0, ""

    def put(char, cells):
        nonlocal col
        line = rows[row]
        while len(line) < col + cells:
            line.append((" ", ""))
        line[col] = (char, sgr)
        for extra in range(1, cells):
            line[col + extra] = ("", sgr)
        col += cells

    position = 0
    while position < len(output):
        if osc := OSC.match(output, position):
            position = osc.end()
            continue
        if csi := CSI.match(output, position):
            position = csi.end()
            params, final = csi[1], csi[2]
            numbers = [int(part) if part.isdigit() else 0 for part in params.split(";")]
            count = max(numbers[0], 1) if numbers else 1
            if final == "m":
                sgr = "" if params in ("", "0") else (f"{sgr};{params}" if sgr else params)
                if params.startswith("0;"):
                    sgr = params[2:]
            elif final == "A":
                row = max(row - count, 0)
            elif final == "B":
                row += count
            elif final == "C":
                col += count
            elif final == "D":
                col = max(col - count, 0)
            elif final == "G":
                col = count - 1
            elif final == "H":
                row = max(numbers[0], 1) - 1 if numbers else 0
                col = max(numbers[1], 1) - 1 if len(numbers) > 1 else 0
            elif final == "K":
                rows[row] = rows[row][:col] if numbers[0] == 0 else []
            elif final == "J":
                if numbers[0] == 2:
                    rows, row, col = [[]], row, col
                else:
                    rows[row] = rows[row][:col]
                    del rows[row + 1:]
            while len(rows) <= row:
                rows.append([])
            continue
        char = output[position]
        position += 1
        if char == "\r":
            col = 0
        elif char == "\n":
            row += 1
            while len(rows) <= row:
                rows.append([])
        elif char >= " ":
            cells = 2 if unicodedata.east_asian_width(char) in "WF" else 1
            if unicodedata.combining(char) or char in "‍️":
                cells = 0
            if cells:
                put(char, cells)
            elif col:
                previous, style = rows[row][col - 1]
                rows[row][col - 1] = (previous + char, style)
    while rows and not rows[-1]:
        rows.pop()
    ansi = []
    for line in rows:
        current, parts = "", []
        for char, style in line[:width]:
            if style != current:
                parts.append("\x1b[0m" + (f"\x1b[{style}m" if style else ""))
                current = style
            parts.append(char)
        ansi.append("".join(parts).rstrip() + "\x1b[0m")
    return Text.from_ansi("\n".join(ansi))
