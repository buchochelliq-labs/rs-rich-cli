"""Byte-for-byte compatibility with Python ``rich`` 15.0.0.

Each program is written once against Rich's API and run twice: with ``rich``'s
modules and with ``rs_rich``'s. The two outputs must be identical, in colour
and without.
"""

from __future__ import annotations

import importlib
import importlib.metadata
import io
import re
from types import SimpleNamespace

import pytest

RICH_VERSION = "15.0.0"


def modules(package: str) -> SimpleNamespace:
    load = lambda name: importlib.import_module(f"{package}.{name}")  # noqa: E731
    return SimpleNamespace(
        console=load("console"),
        text=load("text"),
        style=load("style"),
        table=load("table"),
        panel=load("panel"),
        box=load("box"),
        markup=load("markup"),
        segment=load("segment"),
        measure=load("measure"),
        theme=load("theme"),
        terminal_theme=load("terminal_theme"),
    )


def console(m: SimpleNamespace, color: bool, **options):
    return m.console.Console(
        file=io.StringIO(),
        width=options.pop("width", 60),
        force_terminal=color,
        color_system="truecolor" if color else None,
        **options,
    )


# Every program takes the modules and a console, and prints to it.


def markup_and_highlighting(m, c):
    c.print("[bold red]Error:[/] disk [italic]full[/] at 99% on /dev/sda1")
    c.print("numbers 123 4.5 and True, None, 'strings' and https://example.com")
    c.print("several", "strings", "joined", 42)
    c.print()
    c.print(m.markup.escape("[not markup] stays"))


def text_objects(m, c):
    text = m.text.Text("Hello, World!")
    text.stylize("bold magenta", 0, 6)
    text.stylize(m.style.Style(color="green", underline=True), -6, -1)
    text.append(" appended", style="on blue")
    text.append(m.text.Text.from_markup(" and [i]markup[/i]"))
    c.print(text)
    c.print(m.text.Text("centred", justify="center"))
    c.print(m.text.Text("x" * 90, overflow="ellipsis", no_wrap=True))
    c.print(m.text.Text("wrapped words " * 8, style="yellow"))


def styles(m, c):
    for style in [
        m.style.Style(bold=True, color="red"),
        m.style.Style(italic=False, bgcolor="#102030", underline=True),
        m.style.Style.parse("bold not dim cyan on bright_black"),
        m.style.Style(color="color(208)") + m.style.Style(strike=True),
    ]:
        text = m.text.Text("styled", style=style)
        c.print(text)


def star_wars_table(m, c):
    # The README example of Rich itself.
    table = m.table.Table(title="Star Wars Movies")
    table.add_column("Released", justify="right", style="cyan", no_wrap=True)
    table.add_column("Title", style="magenta")
    table.add_column("Box Office", justify="right", style="green")
    table.add_row("Dec 20, 2019", "Star Wars: The Rise of Skywalker", "$952,110,690")
    table.add_row("May 25, 2018", "Solo: A Star Wars Story", "$393,151,347")
    table.add_row("Dec 15, 2017", "Star Wars Ep. V111: The Last Jedi", "$1,332,539,889")
    table.add_row("Dec 16, 2016", "Rogue One: A Star Wars Story", "$1,332,439,889")
    c.print(table)


def table_options(m, c):
    table = m.table.Table(
        "Name",
        "Score",
        box=m.box.SIMPLE,
        show_lines=True,
        caption="a caption",
        border_style="blue",
    )
    table.add_row("[b]alpha[/]", "1")
    table.add_row(m.text.Text("beta", style="red"), None)
    table.add_row("gamma is a long name that wraps", "3")
    c.print(table)
    grid = m.table.Table(box=None, show_header=False, expand=True)
    grid.add_column(ratio=1)
    grid.add_column(ratio=2, justify="center", header_style="bold")
    grid.add_column(width=6, overflow="fold")
    grid.add_row("left", "middle", "overflowing")
    c.print(grid)
    boxed = m.table.Table(box=m.box.ROUNDED, show_edge=False, title="[i]no edge[/]")
    boxed.add_column("a", min_width=8)
    boxed.add_column("b", max_width=5)
    boxed.add_row("1", "long value here")
    c.print(boxed)


def headless_tables_and_header_styles(m, c):
    # Without a header, head-styled boxes draw plain (`get_plain_headed_box`).
    for box in [m.box.HEAVY_HEAD, m.box.SQUARE_DOUBLE_HEAD, m.box.MINIMAL_HEAVY_HEAD, m.box.ASCII_DOUBLE_HEAD]:
        table = m.table.Table("a", "b", box=box, show_header=False)
        table.add_row("1", "2")
        c.print(table)
    # A column's header_style covers the whole header cell.
    table = m.table.Table(box=m.box.SIMPLE)
    table.add_column("styled", header_style="bold magenta on white", style="dim")
    table.add_column("plain")
    table.add_row("x", "y")
    c.print(table)


def panels(m, c):
    c.print(m.panel.Panel("Hello, [bold]World[/]! 42"))
    c.print(
        m.panel.Panel.fit(
            m.text.Text("fits its text", style="italic"),
            title="Title",
            subtitle="[dim]sub[/]",
            subtitle_align="right",
            border_style="green",
        )
    )
    c.print(m.panel.Panel("padded", box=m.box.DOUBLE, padding=(1, 4), width=30))
    table = m.table.Table("k", "v")
    table.add_row("a", "1")
    c.print(m.panel.Panel(m.panel.Panel(table, title="inner"), title="outer", title_align="left"))


def rules(m, c):
    c.rule()
    c.rule("[bold]Section[/]")
    c.rule("Stars", characters="*", style="red")


def justified_prints(m, c):
    c.print("left")
    c.print("centre", justify="center")
    c.print("right [b]bold[/]", justify="right")


def print_arguments(m, c):
    c.print("hello [b]world[/]", 42, style="on blue")
    c.print("a", m.text.Text("b", style="red"), "c", sep="-", end="!\n")
    c.print("no newline", end="")
    c.print(" continues")
    c.print("x" * 70, overflow="ellipsis")
    c.print("word " * 15, no_wrap=True)
    c.print("y" * 70, crop=False)
    c.print("z" * 70, soft_wrap=True)
    c.print("narrow text that wraps", width=10)
    c.print("[b]not markup[/] :thumbs_up: 1", markup=False)
    c.print("[b]markup[/] :thumbs_up: 1", emoji=False, highlight=False)
    c.print(123, None, 4.5, highlight=False)
    c.print("one\ntwo", new_line_start=True)
    c.print("single", new_line_start=True)
    c.print("tall", height=3)


def justified_renderables(m, c):
    c.print(m.panel.Panel.fit("fit"), justify="right")
    c.print(m.panel.Panel.fit("fit"), "and text", justify="center")
    c.print("full justification spreads the words of a long line out", justify="full")
    c.print("left", justify="left")
    c.print("end", justify="center", end="!!")
    c.print(" is inside the aligned block", justify="right")


def out_line_and_rules(m, c):
    c.out("raw", 1, "[b]not markup[/]", style="red")
    c.out("x" * 70)
    c.line()
    c.line(2)
    c.rule("left title", align="left", style="rule.line")
    c.rule("right", align="right", characters="=")
    c.print()


def console_style_and_themes(m, c):
    styled = m.console.Console(
        file=c.file, width=40, style="italic", force_terminal=c.is_terminal,
        color_system=c.color_system, theme=m.theme.Theme({"accent": "bold magenta"}),
    )
    styled.print("[accent]themed[/] and italic", m.panel.Panel.fit("boxed"))
    styled.push_theme(m.theme.Theme({"accent": "green"}))
    styled.print("[accent]pushed[/]")
    styled.pop_theme()
    with styled.use_theme(m.theme.Theme({"accent": "underline"})):
        styled.print("[accent]used[/]")
    styled.print("[accent]restored[/]")
    styled.print("ends", end="E", style="bold")
    styled.print()
    styled.rule("styled rule")
    c.print(str(styled.get_style("accent")))


def capture_and_render_str(m, c):
    with c.capture() as capture:
        c.print("[bold]captured[/] 1")
    c.begin_capture()
    c.print("again")
    again = c.end_capture()
    c.print(repr(capture.get()), repr(again))
    c.print(c.render_str("[b]no highlight[/] 1", highlight=False, justify="right", style="red"))
    c.print(c.render_str("[b]highlighted[/] 1"))
    c.print(c.render_str("[b]plain[/]", markup=False))


def json_printing(m, c):
    c.print_json('{"name": "rs_rich", "tags": [1, 2.5, null, true, false], "nested": {"a": []}}')
    c.print_json(data={"z": 1, "a": "a long string that does not wrap even past the width " * 2}, sort_keys=True)


def render_measure_and_options(m, c):
    options = c.options
    c.print(options.max_width, options.min_width, options.is_terminal, options.encoding, options.justify)
    updated = options.update(width=10, justify="center", no_wrap=True, height=2)
    c.print(updated.max_width, updated.min_width, updated.justify, updated.no_wrap, updated.height, updated.max_height)
    c.print(c.size.width, c.size.height, c.encoding, c.is_dumb_terminal)
    c.print(repr(c.measure("a few words")), repr(c.measure(m.panel.Panel.fit("abc"))))
    segments = c.render(m.text.Text("hi", style="bold"))
    c.print(repr([(s.text, bool(s.style), bool(s.control)) for s in segments]))
    lines = c.render_lines("a\nbb [b]c[/]", c.options.update_width(6))
    c.print(repr([[s.text for s in line] for line in lines]))
    lines = c.render_lines(m.panel.Panel("p"), c.options.update_width(7), pad=False, new_lines=True)
    c.print(repr(["".join(s.text for s in line) for line in lines]))


PROGRAMS = [
    markup_and_highlighting,
    text_objects,
    styles,
    star_wars_table,
    table_options,
    headless_tables_and_header_styles,
    panels,
    rules,
    justified_prints,
    print_arguments,
    justified_renderables,
    out_line_and_rules,
    console_style_and_themes,
    capture_and_render_str,
    json_printing,
    render_measure_and_options,
]


def test_the_reference_is_rich_15():
    assert importlib.metadata.version("rich") == RICH_VERSION


@pytest.mark.parametrize("color", [True, False], ids=["truecolor", "plain"])
@pytest.mark.parametrize("program", PROGRAMS, ids=lambda p: p.__name__)
def test_output_matches_rich_byte_for_byte(program, color):
    outputs = []
    for package in ["rich", "rs_rich"]:
        m = modules(package)
        c = console(m, color)
        program(m, c)
        outputs.append(c.file.getvalue())
    expected, actual = outputs
    assert actual == expected


@pytest.mark.parametrize("styles", [False, True])
def test_export_text_matches_rich(styles):
    outputs = []
    for package in ["rich", "rs_rich"]:
        m = modules(package)
        c = console(m, True, record=True)
        star_wars_table(m, c)
        c.rule("done")
        outputs.append(c.export_text(styles=styles))
        assert c.export_text() == ""  # cleared
    assert outputs[1] == outputs[0]


@pytest.mark.parametrize("inline_styles", [False, True])
def test_export_html_matches_rich(inline_styles):
    outputs = []
    for package in ["rich", "rs_rich"]:
        m = modules(package)
        c = console(m, True, record=True)
        star_wars_table(m, c)
        # Core's HTML export leaves links out (Rich wraps them in <a href>).
        c.print("[bold red on white]styled[/] [italic]text[/]")
        outputs.append(c.export_html(inline_styles=inline_styles, clear=False))
        outputs.append(c.export_html(theme=m.terminal_theme.MONOKAI, inline_styles=inline_styles))
        assert c.export_text() == ""
    assert outputs[2:] == outputs[:2]


def test_export_svg_matches_rich():
    # Rich derives the default unique_id from its segments' Python reprs;
    # with an explicit one the documents are identical. (Panel and rule
    # titles are left out: core splits a title from the border line beside
    # it into two segments where Rich has one, which only SVG shows.)
    outputs = []
    for package in ["rich", "rs_rich"]:
        m = modules(package)
        c = console(m, True, record=True)
        star_wars_table(m, c)
        text_objects(m, c)
        styles(m, c)
        outputs.append(c.export_svg(title="Test", unique_id="compat"))
    assert outputs[1] == outputs[0]


def test_saving_matches_rich(tmp_path):
    for package in ["rich", "rs_rich"]:
        m = modules(package)
        c = console(m, True, record=True)
        table_options(m, c)
        c.save_text(tmp_path / f"{package}.txt", clear=False)
        c.save_html(tmp_path / f"{package}.html", clear=False)
        c.save_svg(tmp_path / f"{package}.svg", unique_id="x")
    for suffix in ["txt", "html", "svg"]:
        rich_bytes = (tmp_path / f"rich.{suffix}").read_bytes()
        assert (tmp_path / f"rs_rich.{suffix}").read_bytes() == rich_bytes


def log_program(m, c):
    import datetime

    logging_console = m.console.Console(
        file=c.file, width=60, force_terminal=c.is_terminal, color_system=c.color_system,
        get_datetime=lambda: datetime.datetime(2026, 9, 25, 12, 34, 56),
    )
    logging_console.log("first [b]record[/]", 1)
    logging_console.log("same second, so the time is blank")
    logging_console.log("a long message " * 6, justify="right")
    m.console.Console(
        file=c.file, width=60, force_terminal=c.is_terminal, color_system=c.color_system,
        log_time=False, log_path=False,
    ).log("no time", "no path", style="italic")
    m.console.Console(
        file=c.file, width=60, force_terminal=c.is_terminal, color_system=c.color_system,
        log_path=False, log_time_format="%Y-%m-%d",
        get_datetime=lambda: datetime.datetime(2026, 9, 25),
    ).log("custom format")


@pytest.mark.parametrize("color", [True, False], ids=["truecolor", "plain"])
def test_log_matches_rich_except_for_link_ids(color):
    outputs = []
    for package in ["rich", "rs_rich"]:
        m = modules(package)
        c = console(m, color)
        log_program(m, c)
        outputs.append(c.file.getvalue())
    expected = re.sub(r"\x1b\]8;id=[^;]*;", "\x1b]8;;", outputs[0])
    assert outputs[1] == expected


def test_input_matches_rich():
    outputs = []
    for package in ["rich", "rs_rich"]:
        m = modules(package)
        c = console(m, True)
        answer = c.input("[bold]name?[/] ", stream=io.StringIO("Ada\n"))
        c.print(repr(answer))
        outputs.append(c.file.getvalue())
    assert outputs[1] == outputs[0]


def test_links_match_except_for_upstreams_random_ids():
    # Upstream tags each OSC 8 link with a random `id=`; the port leaves it
    # out so output is reproducible (docs/DIVERGENCES.md #20).
    outputs = []
    for package in ["rich", "rs_rich"]:
        m = modules(package)
        c = console(m, True)
        c.print(m.text.Text("site", style=m.style.Style(link="https://example.com")))
        c.print("[link=https://example.org]markup link[/link]")
        outputs.append(c.file.getvalue())
    expected = re.sub(r"\x1b\]8;id=[^;]*;", "\x1b]8;;", outputs[0])
    assert outputs[1] == expected
