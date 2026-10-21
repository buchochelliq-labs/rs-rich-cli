"""Parity cases for the rendering core (0.0.22 workstream 3).

One fixture, `crates/rich/tests/golden/parity_core.tsv`: `name<TAB>json(output)`.
Each case is a function returning the exact string upstream produces;
`crates/rich/tests/golden_parity_core.rs` has a Rust builder per name.

Kept out of `capture_golden.py` so the parity workstreams do not edit the same
lines; `capture_golden.main` calls `capture_parity_core`. Every console here is
truecolor, the only colour system used in this process (rich memoises a style's
escape codes on the instance, whatever the colour system). A case that needs
another system renders in a subprocess (`_isolated`).
"""

from __future__ import annotations

import io
import json
import pathlib
import subprocess
import sys

from rich.align import Align
from rich.ansi import AnsiDecoder
from rich.color import Color, ColorParseError, ColorSystem, blend_rgb, parse_rgb_hex
from rich.color_triplet import ColorTriplet
from rich.columns import Columns
from rich.console import Console, Group
from rich.containers import Lines, Renderables
from rich.errors import MarkupError, StyleSyntaxError
from rich.highlighter import ISO8601Highlighter, JSONHighlighter, NullHighlighter, ReprHighlighter
from rich.markdown import Markdown
from rich.measure import Measurement, measure_renderables
from rich.palette import Palette
from rich.panel import Panel
from rich.rule import Rule
from rich.segment import Segment, SegmentLines, Segments
from rich.style import Style, StyleStack
from rich.table import Table
from rich.terminal_theme import (
    DEFAULT_TERMINAL_THEME,
    DIMMED_MONOKAI,
    MONOKAI,
    NIGHT_OWLISH,
    SVG_EXPORT_THEME,
)
from rich.text import Span, Text


def _console(width: int = 40, **overrides) -> Console:
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


def _isolated(source: str) -> str:
    """Run `source` (which prints its result) in a fresh interpreter."""
    return subprocess.run(
        [sys.executable, "-c", source], capture_output=True, text=True, check=True,
        encoding="utf-8",
    ).stdout


# --- measure.py -----------------------------------------------------------------


def _measure_api() -> str:
    m = Measurement(3, 7)
    return _lines(
        m.span,
        tuple(m.with_minimum(5)),
        tuple(m.with_minimum(9)),
        tuple(m.with_maximum(4)),
        tuple(m.clamp(4, 6)),
        tuple(m.clamp(None, 2)),
        tuple(m.clamp(8, None)),
        tuple(Measurement(9, 4).normalize()),
    )


def _measure_default() -> str:
    console = _console(20)
    options = console.options
    out = [
        tuple(Measurement.get(console, options, Markdown("hello world"))),
        tuple(Measurement.get(console, options, Columns(["aa", "bbb"]))),
        tuple(measure_renderables(console, options, [])),
        tuple(measure_renderables(console, options, ["abc defgh", Text("x" * 12)])),
        tuple(measure_renderables(console, options, [Text("abcdef ghi"), "[b]x[/b] yy"])),
    ]
    return _lines(*out)


def _measure_default_in_table() -> str:
    table = Table("h", "k")
    table.add_row(Columns(["aa", "bbb"]), "x")
    table.add_row(Markdown("hello"), "y")
    return _print(_console(30), table)


# --- terminal_theme.py ----------------------------------------------------------


def _terminal_themes() -> str:
    out = []
    for name, theme in [
        ("default", DEFAULT_TERMINAL_THEME),
        ("monokai", MONOKAI),
        ("dimmed_monokai", DIMMED_MONOKAI),
        ("night_owlish", NIGHT_OWLISH),
        ("svg", SVG_EXPORT_THEME),
    ]:
        colors = " ".join(theme.ansi_colors[index].hex for index in range(16))
        out.append(f"{name} {theme.background_color.hex} {theme.foreground_color.hex} {colors}")
    from rich.terminal_theme import TerminalTheme

    custom = TerminalTheme((1, 2, 3), (4, 5, 6), [(n, n, n) for n in range(8)])
    out.append(" ".join(custom.ansi_colors[index].hex for index in range(16)))
    return _lines(*out)


# --- containers.py --------------------------------------------------------------


def _lines_justify() -> str:
    console = _console(20)
    out = []
    for justify in ["left", "center", "right", "full"]:
        for overflow in ["fold", "crop", "ellipsis"]:
            lines = Lines(
                [
                    Text("hello world  ", style="red"),
                    Text.assemble(("a", "bold"), " ", ("b", "bold"), " c"),
                    Text("supercalifragilistic"),
                    Text("last line here"),
                ]
            )
            lines.justify(console, 12, justify=justify, overflow=overflow)
            for line in lines:
                out.append(f"{justify}/{overflow}|" + _print(console, line, end="").rstrip("\n") + "|")
    return _lines(*out)


def _lines_ops() -> str:
    lines = Lines([Text("a"), Text("b")])
    lines.append(Text("c"))
    lines.extend([Text("d"), Text("e")])
    popped = lines.pop()
    lines[0] = Text("z")
    first = lines.pop(0)
    return _lines(len(lines), popped.plain, first.plain, " ".join(line.plain for line in lines))


def _lines_render() -> str:
    return _print(_console(10), Lines([Text("one", style="bold"), Text("two")]))


def _renderables_render() -> str:
    console = _console(10)
    renderables = Renderables(["[b]a[/b]", Text("bb")])
    renderables.append(Rule())
    out = _print(console, renderables)
    out += _print(console, Panel(Renderables(["x", "yyy"]), expand=False))
    out += _lines(tuple(Measurement.get(console, console.options, Renderables())))
    return out


# --- ansi.py --------------------------------------------------------------------


def _ansi_decode() -> str:
    console = _console(30)
    out = ""
    for source in [
        "a\x1b7b\x1b8c",
        "x\x1b=y\x1b>z",
        "\x1b[99999999999999999999mbig\x1b[0m",
        "\x1b[38;5;99999mq\x1b[0m",
        "\x1b[1;;3mbi\x1b[0m",
        "\x1b(Bok\x1bMup",
        "\x1b[2Jcl\x1b[?25lear",
    ]:
        decoder = AnsiDecoder()
        for text in decoder.decode(source):
            out += _print(console, text)
    return out


# --- highlighter.py -------------------------------------------------------------


def _highlighters() -> str:
    console = _console(60)
    out = ""
    json_highlighter = JSONHighlighter()
    for source in [
        '{"key": "value", "n": -1.5e3, "t": true, "f": false, "z": null}',
        '{"a" :\n  "b", "c"\t: [1, 2]}',
        '"lonely" "string":x',
    ]:
        out += _print(console, json_highlighter(source))
    out += _print(console, NullHighlighter()("plain 123 True"))
    text = Text("2024-01-02", style="italic")
    highlighted = ISO8601Highlighter()(text)
    out += _print(console, text)
    out += _print(console, highlighted)
    out += _print(console, ReprHighlighter()(Text("x=1 [None]")))
    return out


# --- color.py, color_triplet.py, palette.py ---------------------------------------


def _color_api() -> str:
    out = []
    for name in ["red", "bright_red", "color(9)", "color(100)", "#ff0080", "rgb(1,2,3)", "default", "grey50"]:
        color = Color.parse(name)
        out.append(
            f"{name}: system={int(color.system)} sysdef={color.is_system_defined} "
            f"default={color.is_default} true={color.get_truecolor().hex} "
            f"bg={color.get_truecolor(foreground=False).hex} "
            f"mono={color.get_truecolor(MONOKAI).hex}"
        )
    triplet = ColorTriplet(10, 20, 255)
    out.append(f"{triplet.hex} {triplet.rgb} {triplet.normalized}")
    out.append(str(Color.from_triplet(triplet)))
    out.append(str(Color.from_rgb(1.9, 2.5, 255.0)))
    out.append(str(parse_rgb_hex("a0b1c2")))
    out.append(str(blend_rgb(ColorTriplet(0, 0, 0), ColorTriplet(255, 100, 51), 0.3)))
    out.append(str(blend_rgb(ColorTriplet(0, 0, 0), ColorTriplet(255, 100, 51))))
    for palette_color in [(250, 0, 0), (128, 128, 128), (0, 0, 1)]:
        out.append(str(DEFAULT_TERMINAL_THEME.ansi_colors.match(palette_color)))
    return _lines(*out)


def _color_parse_errors() -> str:
    out = []
    for name in [
        "color(256)",
        "color( 5)",
        "color(0005)",
        "rgb(1,2)",
        "rgb(1,2,300)",
        "rgb( 1 , 2 , 3 )",
        "#ABCDEF",
        "  Red  ",
        "#abc",
        "nope",
        "rgb(1,2,3,4)",
    ]:
        try:
            color = Color.parse(name)
            out.append(f"{name!r}: ok {color.name} {color.type.name} {color.number} {color.triplet}")
        except ColorParseError as error:
            out.append(f"{name!r}: {error}")
    return _lines(*out)


def _color_rich() -> str:
    console = _console(40)
    out = ""
    for name in ["red", "color(100)", "#ff0080", "default"]:
        out += _print(console, Color.parse(name))
    return out


def _palette_rich() -> str:
    return _print(_console(50), Palette([(255, 0, 0), (0, 128, 255), (12, 34, 56)]))


# --- style.py -------------------------------------------------------------------


def _style_api() -> str:
    style = Style(bold=True, italic=False, color="red", bgcolor="blue", link="https://x")
    out = [
        f"{style.bold} {style.italic} {style.underline} {style.dim}",
        str(style),
        str(style.without_color),
        str(style.background_style),
        f"{style.transparent_background} {Style(bgcolor='default').transparent_background} {Style().transparent_background}",
        str(Style.null()),
        bool(Style.null()),
        str(Style.chain(Style(bold=True), Style(color="red"), Style(bold=False))),
        str(Style.combine([Style(italic=True), Style(underline=True)])),
        str(Style.pick_first(None, "bold", "red")),
        str(style.copy()),
        # A fresh style: upstream's copy keeps a definition string already
        # computed, link included (DIVERGENCES §40).
        str(Style(bold=True, color="red", link="https://x").clear_meta_and_links()),
        str(Style(blink=True, blink2=True, conceal=True, strike=True, underline2=True,
                  frame=True, encircle=True, overline=True, reverse=True, dim=True)),
        Style(meta={"a": 1}).without_color == Style(),
        Style(bold=True) + Style(meta={}) == Style(bold=True),
    ]
    stack = StyleStack(Style(color="red"))
    stack.push(Style(bold=True))
    stack.push(Style(color="blue"))
    out.append(str(stack.current))
    out.append(str(stack.pop()))
    return _lines(*out)


def _style_parse_errors() -> str:
    out = []
    for definition in [
        "on",
        "bold on nope",
        "not",
        "not boldx",
        "not BOLD",
        "link",
        "nope",
        "bold color(300)",
        "red on rgb(1,2)",
    ]:
        try:
            out.append(f"{definition!r}: ok {Style.parse(definition)}")
        except StyleSyntaxError as error:
            out.append(f"{definition!r}: {error}")
    return _lines(*out)


def _style_render() -> str:
    out = [
        repr(Style(bold=True, color="red").render("hi")),
        repr(Style(bold=True, color="#ff8800").render("hi", color_system=None)),
        repr(Style(color="red").render("")),
        repr(Style().render("plain")),
    ]
    return _lines(*out)


# --- markup.py ------------------------------------------------------------------


def _markup_meta() -> str:
    from rich.markup import render

    out = []
    for source in [
        "[@click]a[/]",
        "[@click=foo]a[/]",
        "[@click=foo(1, 'x')]a[/]",
        "[@click='hi']a[/]",
        "[@click=5]a[/]",
        "[@click=[1, 2.5, None, True]]a[/]",
        "[@click=(1,)]a[/]",
        "[@click=foo(]a[/]",
        # Not `foo(bar)`: its error names an AST node at a memory address.
        "[bold][@enter=go(1)]x[/][/bold]",
        "[@click=-3]a[/]",
    ]:
        try:
            text = render(source)
            metas = [span.style.meta if isinstance(span.style, Style) else span.style for span in text.spans]
            out.append(f"{source} -> {metas!r} | {text.markup}")
        except MarkupError as error:
            out.append(f"{source} -> error: {error}")
    return _lines(*out)


def _markup_errors() -> str:
    from rich.markup import render

    out = []
    for source in ["[/  bold ]", "[/bold=x]", "a[/]"]:
        try:
            render(source)
            out.append("ok")
        except MarkupError as error:
            out.append(str(error))
    return _lines(*out)


def _markup_tag_and_style() -> str:
    from rich.markup import Tag, render

    out = [
        str(Tag("bold", None)),
        str(Tag("link", "https://x")),
        Tag("bold", None).markup,
        Tag("link", "https://x").markup,
    ]
    console = _console(20)
    text = render("a [i]b[/i] c", style="red")
    return _lines(*out) + _print(console, text)


PARITY_CORE_CASES = [
    ("measure_api", _measure_api),
    ("measure_default", _measure_default),
    ("measure_default_in_table", _measure_default_in_table),
    ("terminal_themes", _terminal_themes),
    ("lines_justify", _lines_justify),
    ("lines_ops", _lines_ops),
    ("lines_render", _lines_render),
    ("renderables_render", _renderables_render),
    ("ansi_decode", _ansi_decode),
    ("highlighters", _highlighters),
    ("color_api", _color_api),
    ("color_parse_errors", _color_parse_errors),
    ("color_rich", _color_rich),
    ("palette_rich", _palette_rich),
    ("style_api", _style_api),
    ("style_parse_errors", _style_parse_errors),
    ("style_render", _style_render),
    ("markup_meta", _markup_meta),
    ("markup_errors", _markup_errors),
    ("markup_tag_and_style", _markup_tag_and_style),
]


def capture_parity_core(golden_dir: pathlib.Path) -> None:
    path = golden_dir / "parity_core.tsv"
    lines = ["# name\tjson(expected output) — see scripts/golden_parity_core.py"]
    for name, build in PARITY_CORE_CASES:
        lines.append(f"{name}\t{json.dumps(build(), ensure_ascii=False)}")
    path.write_text("\n".join(lines) + "\n", encoding="utf-8", newline="\n")
    print(f"wrote {len(PARITY_CORE_CASES)} parity core cases to {path}")


if __name__ == "__main__":
    sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
    from capture_golden import golden_dir, verify_upstream_version

    verify_upstream_version()
    capture_parity_core(golden_dir())
