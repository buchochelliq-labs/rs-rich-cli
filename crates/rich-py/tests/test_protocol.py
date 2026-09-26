"""Rich's render protocol: user classes with ``__rich__``, ``__rich_console__``
and ``__rich_measure__``, printed alone and inside tables and panels.

The programs are compared byte for byte with Rich 15.0.0, as in
``test_compat.py``; the rest checks what only rs_rich can get wrong: errors
raised during a render, re-entrant calls, recursion and threads.
"""

from __future__ import annotations

import io
import threading

import pytest

from test_compat import console, modules


def classes(m):
    """User classes written against Rich's protocol, for modules `m`."""

    class Cast:
        def __rich__(self):
            return "[bold]cast[/] to markup 42"

    class CastToPanel:
        def __rich__(self):
            return m.panel.Panel.fit(Cast())

    class Yields:
        def __init__(self, styled=True):
            self.styled = styled

        def __rich_console__(self, console, options):
            styled = self.styled
            yield "a [i]str[/] with 1 number"
            yield m.text.Text("a Text", style="green") if styled else m.text.Text.from_markup("[green]a Text[/]")
            yield m.segment.Segment("a Segment", m.style.Style(color="red", bold=True))
            yield m.segment.Segment.line()
            yield m.segment.Segment("two\nlines")
            yield m.segment.Segment.line()
            yield m.panel.Panel("a nested panel")
            yield Cast()

    class SegmentsOnly:
        def __rich_console__(self, console, options):
            yield m.segment.Segment("no newline")

    class Options:
        def __rich_console__(self, console, options):
            yield (
                f"max={options.max_width} min={options.min_width} "
                f"justify={options.justify} no_wrap={options.no_wrap} "
                f"highlight={options.highlight} height={options.height}"
            )

    class Measured:
        def __init__(self, text, minimum, maximum):
            self.text, self.minimum, self.maximum = text, minimum, maximum

        def __rich_console__(self, console, options):
            yield m.text.Text(self.text[: options.max_width])

        def __rich_measure__(self, console, options):
            return m.measure.Measurement(self.minimum, self.maximum)

    class Unmeasured:
        def __rich_console__(self, console, options):
            yield "no __rich_measure__, so it takes the width"

    class UsesConsole:
        def __rich_console__(self, console, options):
            width = console.measure("four").maximum
            yield f"measured {width}"
            for line in console.render_lines("rendered [b]lines[/]", options.update_width(8)):
                yield from line
                yield m.segment.Segment.line()
            yield from console.render(m.text.Text("rendered", style="blue"))

    return {
        "Cast": Cast,
        "CastToPanel": CastToPanel,
        "Yields": Yields,
        "SegmentsOnly": SegmentsOnly,
        "Options": Options,
        "Measured": Measured,
        "Unmeasured": Unmeasured,
        "UsesConsole": UsesConsole,
    }


def printed_alone(m, c):
    k = classes(m)
    c.print(k["Cast"]())
    c.print(k["CastToPanel"]())
    c.print(k["Yields"]())
    c.print(k["SegmentsOnly"]())
    c.print(k["Options"]())
    c.print(k["UsesConsole"]())
    c.print("text", k["Cast"](), "joins", k["Yields"](), "then text")
    c.print(k["Options"](), justify="center", no_wrap=True)
    c.print(k["Yields"](), style="on blue")


def inside_panels(m, c):
    k = classes(m)
    c.print(m.panel.Panel(k["Yields"](), title="yields"))
    c.print(m.panel.Panel(k["Options"]()))
    c.print(m.panel.Panel.fit(k["Measured"]("measured", 8, 8)))
    c.print(m.panel.Panel.fit(k["Unmeasured"]()))
    c.print(m.panel.Panel(k["CastToPanel"](), title="cast"))
    c.print(m.panel.Panel(k["UsesConsole"]()))


def inside_tables(m, c):
    k = classes(m)
    table = m.table.Table("protocol", "measured", "text")
    # A whole-Text style would show a core difference: justified padding
    # comes out as a segment of its own (an extra SGR run in a cell).
    table.add_row(k["Yields"](styled=False), k["Measured"]("abcdefghij", 4, 10), "plain")
    table.add_row(k["Cast"](), k["Unmeasured"](), m.text.Text("text"))
    table.add_row(k["Options"](), k["SegmentsOnly"](), None)
    c.print(table)
    grid = m.table.Table(box=None, show_header=False)
    grid.add_column()
    grid.add_column(justify="right")
    grid.add_row(k["Measured"]("wide", 30, 30), k["Measured"]("x", 1, 1))
    c.print(grid)
    c.print(m.panel.Panel(table, title="table in panel"))


PROGRAMS = [printed_alone, inside_panels, inside_tables]


@pytest.mark.parametrize("color", [True, False], ids=["truecolor", "plain"])
@pytest.mark.parametrize("program", PROGRAMS, ids=lambda p: p.__name__)
def test_protocol_output_matches_rich_byte_for_byte(program, color):
    outputs = []
    for package in ["rich", "rs_rich"]:
        m = modules(package)
        c = console(m, color)
        program(m, c)
        outputs.append(c.file.getvalue())
    assert outputs[1] == outputs[0]


def test_measure_and_render_match_rich():
    outputs = []
    for package in ["rich", "rs_rich"]:
        m = modules(package)
        k = classes(m)
        c = console(m, False)
        result = [
            tuple(c.measure(k["Measured"]("x", 3, 7))),
            tuple(c.measure(k["Measured"]("x", 9, 2))),
            tuple(c.measure(k["Unmeasured"]())),
            tuple(c.measure(k["Cast"]())),
            tuple(c.measure("two words")),
            [s.text for s in c.render(k["Yields"]())],
            [[s.text for s in line] for line in c.render_lines(k["Yields"](), c.options.update_width(12))],
        ]
        outputs.append(result)
    assert outputs[1] == outputs[0]


def test_measure_accepts_rich_measurements_and_pairs():
    from rs_rich.console import Console

    class Pair:
        def __rich_console__(self, console, options):
            yield "p"

        def __rich_measure__(self, console, options):
            return (2, 6)

    assert tuple(Console(file=io.StringIO()).measure(Pair())) == (2, 6)


# What only rs_rich can get wrong -------------------------------------------

from rs_rich.console import Console  # noqa: E402
from rs_rich.errors import NotRenderableError  # noqa: E402
from rs_rich.panel import Panel  # noqa: E402
from rs_rich.table import Table  # noqa: E402


class Boom:
    def __rich_console__(self, console, options):
        yield "before the error"
        raise ValueError("boom")


@pytest.mark.parametrize(
    "wrap",
    [lambda b: b, lambda b: Panel(b), lambda b: Panel(Panel(b))],
    ids=["alone", "in a panel", "in nested panels"],
)
def test_an_exception_during_a_render_propagates_and_prints_nothing(wrap):
    out = io.StringIO()
    with pytest.raises(ValueError, match="boom"):
        Console(file=out).print(wrap(Boom()))
    assert out.getvalue() == ""


def test_an_exception_in_a_table_cell_propagates():
    table = Table("a")
    table.add_row(Boom())
    with pytest.raises(ValueError, match="boom"):
        Console(file=io.StringIO()).print(table)


def test_an_exception_in_rich_measure_propagates():
    class BadMeasure:
        def __rich_console__(self, console, options):
            yield "x"

        def __rich_measure__(self, console, options):
            raise KeyError("measure")

    table = Table("a")
    table.add_row(BadMeasure())
    with pytest.raises(KeyError, match="measure"):
        Console(file=io.StringIO()).print(table)


def test_the_console_is_usable_after_a_failed_render():
    out = io.StringIO()
    console = Console(file=out)
    with pytest.raises(ValueError):
        console.print(Panel(Boom()))
    console.print("fine")
    assert out.getvalue() == "fine\n"


def test_not_renderable_objects():
    class NotIterable:
        def __rich_console__(self, console, options):
            return 5

    console = Console(file=io.StringIO())
    with pytest.raises(NotRenderableError, match="is not renderable"):
        console.print(NotIterable())
    with pytest.raises(NotRenderableError, match="Unable to render"):
        console.print(Panel(object()))
    with pytest.raises(NotRenderableError, match="Unable to get render width"):
        console.measure(object())


def test_a_rich_method_returning_itself_prints_its_str():
    class Loop:
        def __rich__(self):
            return self

        def __str__(self):
            return "loop"

    out = io.StringIO()
    Console(file=out).print(Loop())
    assert out.getvalue() == "loop\n"


def test_print_from_inside_rich_console_writes_first():
    # Rich buffers the outer print, so the inner one's output comes first.
    class Reentrant:
        def __rich_console__(self, console, options):
            console.print("inner")
            yield "outer"

    out = io.StringIO()
    Console(file=out).print(Reentrant())
    assert out.getvalue() == "inner\nouter\n"


def test_deep_protocol_recursion_is_a_recursion_error():
    class Deep:
        def __init__(self, depth):
            self.depth = depth

        def __rich_console__(self, console, options):
            yield Deep(self.depth - 1) if self.depth else "bottom"

    console = Console(file=io.StringIO())
    with pytest.raises(RecursionError, match="at most 100 nested"):
        console.print(Deep(1000))
    console.print(Deep(50))
    assert console.file.getvalue() == "bottom\n"


def test_protocol_objects_render_from_many_threads():
    class Slow:
        def __rich_console__(self, console, options):
            for i in range(3):
                # Other threads print and query the console meanwhile.
                assert console.width == 30
                yield f"{threading.current_thread().name} {i}"

    out = io.StringIO()
    console = Console(file=out, width=30)
    workers = [
        threading.Thread(target=lambda: [console.print(Slow()) for _ in range(5)], name=f"w{n}")
        for n in range(4)
    ]
    for worker in workers:
        worker.start()
    for worker in workers:
        worker.join()
    lines = out.getvalue().splitlines()
    assert len(lines) == 4 * 5 * 3
    # Each print's three lines are written together.
    for start in range(0, len(lines), 3):
        names = {line.split()[0] for line in lines[start : start + 3]}
        assert len(names) == 1


def test_objects_are_rendered_when_printed_not_when_added():
    class Counter:
        def __init__(self):
            self.value = 0

        def __rich__(self):
            return f"value {self.value}"

    counter = Counter()
    panel = Panel.fit(counter)
    counter.value = 7
    out = io.StringIO()
    Console(file=out).print(panel)
    assert "value 7" in out.getvalue()
