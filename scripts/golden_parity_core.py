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


# --- text.py --------------------------------------------------------------------


def _text_lines(console: Console, lines) -> str:
    return "".join("|" + _print(console, line, end="").rstrip("\n") + "|\n" for line in lines)


def _text_api() -> str:
    console = _console(30)
    text = Text("Hello, World!", style="bold")
    text.stylize("red", 0, 5)
    text.stylize("underline", 3, 9)
    out = _print(console, text[7:12])
    out += _print(console, text[3:9])
    out += _lines(
        "World" in text,
        Text("World") in text,
        "xyz" in text,
        Text("ab") == Text("ab"),
        Text("ab", spans=[Span(0, 1, "red")]) == Text("ab"),
        Text("ab", style="red") == Text("ab"),
    )
    out += _print(console, text + " more")
    out += _print(console, text + Text("x", style="blue"))
    for offset in [0, 3, 6, -1, -4]:
        out += _lines(f"{offset}: {text.get_style_at_offset(console, offset)}")
    return out


def _text_edit() -> str:
    console = _console(30)
    out = ""
    assembled = Text.assemble("a", ("b", "red"), Text("c", style="blue"), style="italic")
    out += _print(console, assembled)
    text = Text("file.py", style="green")
    text.stylize("bold", 4, 7)
    text.remove_suffix(".py")
    text.remove_suffix(".rs")
    out += _print(console, text)
    for length in [3, 10]:
        text = Text("abcdef", style="on blue")
        text.stylize("red", 2, 6)
        text.set_length(length)
        out += _text_lines(console, [text])
    text = Text("ab")
    text.stylize("red", 0, 2)
    text.stylize("blue", 0, 1)
    text.extend_style(3)
    text.extend_style(0)
    out += _text_lines(console, [text])
    text = Text("plain")
    text.extend_style(2)
    out += _text_lines(console, [text])
    for align in ["left", "center", "right"]:
        for width in [3, 9]:
            text = Text("hello", style="red")
            text.align(align, width, character=".")
            out += _text_lines(console, [text])
    text = Text("x")
    text.append_tokens([("a", "red"), ("b", None), ("c\x07", "bold"), ("", "blue")])
    out += _print(console, text)
    target = Text("abcdef")
    source = Text("zz")
    source.stylize("red", 0, 2)
    source.stylize("bold", 1, 4)
    target.copy_styles(source)
    out += _print(console, target)
    return out


WRAP_SOURCE = "The quick brown fox jumps over\tthe lazy dog supercalifragilistic"


def _text_wrap() -> str:
    console = _console(40)
    out = ""
    text = Text(WRAP_SOURCE)
    text.stylize("red", 4, 9)
    text.stylize("bold", 10, 19)
    for justify in ["left", "center", "right", "full", "default"]:
        for overflow in ["fold", "crop", "ellipsis"]:
            out += f"{justify}/{overflow}\n"
            out += _text_lines(console, text.wrap(console, 12, justify=justify, overflow=overflow, tab_size=4))
    out += "no_wrap\n" + _text_lines(console, text.wrap(console, 12, no_wrap=True))
    out += "ignore\n" + _text_lines(console, text.wrap(console, 12, overflow="ignore"))
    own = Text("one two three four", justify="right", overflow="ellipsis", no_wrap=False)
    out += "own\n" + _text_lines(console, own.wrap(console, 7))
    return out


def _text_fit() -> str:
    console = _console(20)
    text = Text("abc\ndefgh\n\nij\n")
    text.stylize("red", 1, 6)
    return _text_lines(console, text.fit(4))


def _text_from_markup() -> str:
    console = _console(30)
    out = _print(console, Text.from_markup(":smile: [b]x[/b] :thumbs_up:"))
    out += _print(console, Text.from_markup("no tags :smile:"))
    out += _print(console, Text.from_markup("[b]:smile:[/b]", emoji=False))
    out += _print(console, Text.from_markup("a [i]b[/i]", style="red"))
    return out


def _span_api() -> str:
    span = Span(2, 5, "red")
    return _lines(
        span.split(3),
        span.split(1),
        span.split(5),
        span.move(2),
        span.right_crop(4),
        span.right_crop(9),
        span.extend(2),
        span.extend(0),
    )


# --- segment.py -----------------------------------------------------------------


def _seg(segments) -> str:
    def one(segment):
        control = "C" if segment.control else ""
        return f"{segment.text!r}{control}:{segment.style}"

    return "[" + ", ".join(one(segment) for segment in segments) + "]"


def _segment_split_cells() -> str:
    out = []
    for text in ["abcdef", "日本語テキスト", "a日b本c", "😀x😀"]:
        segment = Segment(text, Style(bold=True))
        for cut in range(0, segment.cell_length + 2):
            before, after = segment.split_cells(cut)
            out.append(f"{text} {cut}: {_seg([before])} {_seg([after])}")
    return _lines(*out)


def _segment_divide() -> str:
    segments = [Segment("abc"), Segment("日本", Style(color="red")), Segment("xyz")]
    out = []
    for cuts in [[], [0], [0, 0, 2], [1, 4, 6], [3, 5, 9], [2, 20], [4], [8, 8]]:
        out.append(f"{cuts}: " + " ".join(_seg(piece) for piece in Segment.divide(segments, cuts)))
    return _lines(*out)


def _segment_lines() -> str:
    red = Style(color="red")
    segments = [Segment("ab\ncdef"), Segment("gh\n", red), Segment("\n"), Segment("tail")]
    out = []
    for line, terminated in Segment.split_lines_terminator(segments):
        out.append(f"{_seg(line)} {terminated}")
    out.append(str([_seg(line) for line in Segment.split_lines(segments)]))
    for length, pad, include in [(3, True, True), (5, False, True), (5, True, False)]:
        cropped = Segment.split_and_crop_lines(segments, length, style=red, pad=pad, include_new_lines=include)
        out.append(" ".join(_seg(line) for line in cropped))
    out.append(_seg(Segment.adjust_line_length([Segment("ab")], 4, pad=False)))
    return _lines(*out)


def _segment_shape() -> str:
    blue = Style(bgcolor="blue")
    lines = [[Segment("ab")], [Segment("abcd"), Segment("\x1b[1m", None, [(1,)])], [Segment("a")]]
    out = [
        str(Segment.get_line_length(lines[1])),
        str(Segment.get_shape(lines)),
        str(Segment.get_shape([])),
    ]
    for height, new_lines in [(None, False), (5, False), (2, False), (4, True)]:
        shaped = Segment.set_shape(lines, 3, height, style=blue, new_lines=new_lines)
        out.append(" ".join(_seg(line) for line in shaped))
    for align in [Segment.align_top, Segment.align_middle, Segment.align_bottom]:
        for height, new_lines in [(6, False), (4, True), (3, False), (2, False)]:
            aligned = align(lines, 4, height, blue, new_lines)
            out.append(" ".join(_seg(line) for line in aligned))
    return _lines(*out)


def _segment_misc() -> str:
    link = Style(color="red", link="https://x")
    segments = [
        Segment("a", link),
        Segment("\x07", None, [(1,)]),
        Segment("b"),
        Segment("c", Style()),
        Segment("d", Style(bold=True)),
    ]
    # `strip_links` first: upstream's `update_link` copies a definition string
    # already computed, link included (DIVERGENCES §40).
    out = [
        _seg(Segment.strip_links(segments)),
        _seg(Segment.filter_control(segments)),
        _seg(Segment.filter_control(segments, is_control=True)),
        _seg(Segment.strip_styles(segments)),
        _seg(Segment.apply_style(segments, Style(italic=True))),
        _seg(Segment.apply_style(segments, post_style=Style(color="blue"))),
        _seg(Segment.apply_style(segments, Style(italic=True), Style(underline=True))),
        _seg(Segment.apply_style(segments)),
        str(Segment("abc").is_control),
        str(Segment("abc", None, [(1,)]).is_control),
    ]
    return _lines(*out)


def _segments_render() -> str:
    console = _console(12)
    red = Style(color="red")
    out = _print(console, Panel(Segments([Segment("one", red), Segment("two")], new_lines=True)))
    out += _print(console, Panel(Segments([Segment("one", red), Segment("two")])))
    lines = [[Segment("a", red), Segment("b")], [Segment("cd")]]
    out += _print(console, Panel(SegmentLines(lines, new_lines=True)))
    out += _print(console, Panel(SegmentLines(lines)))
    return out


# --- console.py -----------------------------------------------------------------


def _console_print() -> str:
    console = _console(20)
    out = ""
    out += _print(console, "a", "b", sep="-", end="!\n")
    out += _print(console, "[b]x[/b]", Text("y", style="red"), Panel("p", expand=False), "z", 1)
    out += "<" + _print(console, end="") + ">\n"
    out += "<" + _print(console) + ">\n"
    out += _print(console, "the quick brown fox jumps", width=10)
    out += _print(console, "hello", style="red on blue")
    out += _print(console, "x" * 30)
    out += _print(console, "y" * 30, crop=False, no_wrap=True, overflow="ignore")
    out += _print(console, "soft " * 6, soft_wrap=True)
    out += _print(console, "one\ntwo", new_line_start=True)
    out += _print(console, "one", new_line_start=True)
    out += _print(console, "mid", Text("text"), justify="center")
    out += _print(console, Panel("p", expand=False), justify="right")
    out += _print(console, "[b]no[/b] :smile:", markup=False, emoji=False)
    out += _print(console, "1 2 None 'str'", highlight=True)
    out += _print(console, "a", end="")
    out += _print(console, "b", "c", sep="", end="--\n")
    return out


def _console_methods() -> str:
    console = _console(20)
    with console.capture() as capture:
        console.line(2)
        console.rule("T", characters="=", style="red", align="left")
        console.rule()
        console.out("a", "b [b]c[/b]", sep=",", style="bold")
        console.out("1 2", highlight=True, end="|\n")
        console.print_json('{"a": [1, true, null], "b": "x"}')
    out = capture.get()
    out += _lines(tuple(console.measure(Text("hello world"))), tuple(console.measure(Panel("ab"))))
    return out


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
    ("text_api", _text_api),
    ("text_edit", _text_edit),
    ("text_wrap", _text_wrap),
    ("text_fit", _text_fit),
    ("text_from_markup", _text_from_markup),
    ("span_api", _span_api),
    ("segment_split_cells", _segment_split_cells),
    ("segment_divide", _segment_divide),
    ("segment_lines", _segment_lines),
    ("segment_shape", _segment_shape),
    ("segment_misc", _segment_misc),
    ("segments_render", _segments_render),
    ("console_print", _console_print),
    ("console_methods", _console_methods),
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
