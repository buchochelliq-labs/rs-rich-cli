"""Text, Style, Color, Theme, markup and emoji against rich 15.0.0.

Each program is written once against Rich's API and returns a string (reprs,
values and printed output). It runs with ``rich``'s modules and with
``rs_rich``'s, and the two strings must be identical. Exceptions are part of
the output (their type and message).
"""

from __future__ import annotations

import importlib
import io
import re
from types import SimpleNamespace

import pytest

MODULES = ["console", "text", "style", "color", "theme", "markup", "emoji", "segment", "terminal_theme"]


def load(package: str) -> SimpleNamespace:
    return SimpleNamespace(**{name: importlib.import_module(f"{package}.{name}") for name in MODULES})


RICH = load("rich")
RS = load("rs_rich")


def console(m, width=30, color=True):
    return m.console.Console(
        file=io.StringIO(),
        width=width,
        force_terminal=color,
        color_system="truecolor" if color else None,
    )


def printed(m, *objects, width=30, color=True, **options):
    c = console(m, width, color)
    c.print(*objects, **options)
    return c.file.getvalue()


def attempt(fn):
    try:
        return repr(fn())
    except Exception as error:  # noqa: BLE001 - the error is the output
        return f"{type(error).__name__}: {error}"


def segments(items):
    return [(s.text, str(s.style) if s.style is not None else None) for s in items]


# --- Style ------------------------------------------------------------------


def style_construction(m):
    S = m.style.Style
    out = []
    for kwargs in [
        {},
        {"bold": True},
        {"italic": False, "dim": 1},
        {"color": "red", "bgcolor": "#102030"},
        {"blink2": True, "underline2": True, "frame": True, "encircle": True, "overline": True},
        {"reverse": True, "conceal": False, "strike": True},
        {"color": "color(208)", "bgcolor": "rgb(1,2,3)"},
        {"color": "default", "bgcolor": "default"},
        {"link": "https://example.com"},
        {"color": m.color.Color.parse("magenta")},
    ]:
        style = S(**kwargs)
        out.append((repr(style), str(style), bool(style), style.link))
        for name in [
            "bold", "dim", "italic", "underline", "blink", "blink2", "reverse", "conceal",
            "strike", "underline2", "frame", "encircle", "overline",
        ]:
            out.append((name, getattr(style, name)))
        out.append((repr(style.color), repr(style.bgcolor), style.transparent_background))
        out.append((repr(style.background_style), repr(style.without_color), repr(style.copy())))
        out.append((repr(style.clear_meta_and_links()), repr(style.update_link("https://x"))))
    out.append(attempt(lambda: S(color="nope")))
    out.append(attempt(lambda: S(bgcolor="rgb(1,2,300)")))
    return "\n".join(map(str, out))


def style_parsing(m):
    S = m.style.Style
    out = []
    for definition in [
        "", "none", "bold", "b RED", "not bold italic", "bold on blue", "i u s r c o uu",
        "link https://example.com", "#FF0000 on color(3)", "blink2 frame encircle",
        "bold nope", "on", "not", "link", "not BOLD", "bold none",
    ]:
        out.append(attempt(lambda: S.parse(definition)))
        out.append(attempt(lambda: str(S.parse(definition))))
        out.append(S.normalize(definition))
    return "\n".join(out)


def style_arithmetic(m):
    S = m.style.Style
    out = []
    a = S(bold=True, color="red", link="https://a")
    b = S(italic=True, color="blue", bgcolor="white")
    c = S(bold=False)
    out.append(repr(a + b))
    out.append(repr(b + a))
    out.append(repr(a + c))
    out.append(repr(a + None))
    out.append(repr(S() + b))
    out.append(repr(S.combine([a, b, c])))
    out.append(repr(S.chain(c, b)))
    out.append(repr(S.null()))
    out.append(repr(S.pick_first(None, "bold", "red")))
    out.append(attempt(lambda: S.pick_first(None, None)))
    out.append(repr(S.from_color(m.color.Color.parse("red"), m.color.Color.parse("#000001"))))
    out.append(repr(S.from_color()))
    out.append(str(a == S.parse("bold red link https://a")))
    out.append(str(a != b))
    out.append(str(hash(S.parse("bold red")) == hash(S(color="red", bold=True))))
    out.append(str(S(bold=True) == "bold"))
    return "\n".join(out)


def style_meta(m):
    S = m.style.Style
    out = []
    with_meta = S(bold=True, meta={"a": 1})
    out.append(repr(with_meta))
    out.append(repr(with_meta.meta))
    out.append(str(with_meta == S(bold=True)))
    out.append(str(bool(S(meta={}))))
    out.append(repr(S.from_meta({"x": 1})))
    out.append(repr(S.on(click="go()")))
    out.append(repr(S.on({"k": "v"}, hover="h")))
    out.append(repr(with_meta.clear_meta_and_links()))
    out.append(repr(with_meta + S.from_meta({"b": 2})))
    out.append(repr((with_meta + S.from_meta({"b": 2})).meta))
    out.append(repr(with_meta.without_color))
    out.append(repr(S().meta))
    return "\n".join(out)


def style_render(m):
    S = m.style.Style
    CS = m.color.ColorSystem
    out = []
    # Rich caches a style's codes on its first render (and `Style.parse`
    # caches styles), so each render here uses a new style.
    for kwargs in [
        {"bold": True, "color": "red"},
        {"italic": True, "color": "#102030", "bgcolor": "color(200)"},
        {"dim": True, "underline": True},
        {},
        {"reverse": True, "blink2": True, "overline": True, "color": "rgb(200,10,10)"},
    ]:
        out.append(repr(S(**kwargs).render("text")))
        out.append(repr(S(**kwargs).render("")))
        out.append(repr(S(**kwargs).render("x", color_system=None)))
        for system in [CS.STANDARD, CS.EIGHT_BIT, CS.TRUECOLOR, CS.WINDOWS]:
            out.append(repr(S(**kwargs).render("x", color_system=system)))
    return "\n".join(out)


def style_html(m):
    S = m.style.Style
    tt = m.terminal_theme
    out = []
    for definition in [
        "bold red", "italic #102030 on color(200)", "dim", "dim blue", "reverse green on white",
        "underline strike overline", "none", "default on default",
    ]:
        style = S.parse(definition)
        out.append(style.get_html_style())
        out.append(style.get_html_style(tt.MONOKAI))
    return "\n".join(out)


def style_stack(m):
    stack = m.style.StyleStack(m.style.Style(bold=True))
    out = [repr(stack.current)]
    stack.push(m.style.Style(color="red"))
    out.append(repr(stack.current))
    stack.push(m.style.Style(bold=False))
    out.append(repr(stack))
    out.append(repr(stack.pop()))
    out.append(repr(stack.pop()))
    return "\n".join(out)


# --- Color ------------------------------------------------------------------


COLORS = [
    "red", "bright_blue", "grey93", "default", "DEFAULT", " Red ", "#ff8800", "#FF8800",
    "color(3)", "color(15)", "color(16)", "color(231)", "color(255)", "rgb(10,200,30)",
    "rgb(10, 20 ,30)", "#808080", "#123456", "#fefefe",
]


def color_values(m):
    C = m.color.Color
    theme = m.terminal_theme.MONOKAI
    out = []
    for name in COLORS:
        color = C.parse(name)
        out.append(repr(color))
        out.append(repr((color.system, color.is_system_defined, color.is_default, str(color.system))))
        out.append(repr(color.get_truecolor()))
        out.append(repr(color.get_truecolor(theme)))
        out.append(repr(color.get_truecolor(theme, foreground=False)))
        out.append(repr((color.get_ansi_codes(), color.get_ansi_codes(False))))
        for system in m.color.ColorSystem:
            downgraded = color.downgrade(system)
            out.append(repr(downgraded))
            out.append(repr(downgraded.get_truecolor()))
        out.append(repr(color == tuple(color)))
    return "\n".join(out)


def color_constructors(m):
    C = m.color.Color
    CT = m.color.ColorTriplet
    out = []
    out.append(repr(C.from_ansi(5)))
    out.append(repr(C.from_ansi(100)))
    out.append(repr(C.from_rgb(1.9, 2, 255)))
    out.append(repr(C.from_triplet(CT(1, 2, 3))))
    out.append(repr(C.default()))
    triplet = CT(255, 128, 0)
    out.append(repr((triplet, triplet.hex, triplet.rgb, triplet.normalized, tuple(triplet))))
    out.append(repr(m.color.parse_rgb_hex("0a0B0c")))
    out.append(repr(m.color.blend_rgb((0, 0, 0), (255, 255, 255))))
    out.append(repr(m.color.blend_rgb((10, 20, 30), (200, 100, 0), 0.25)))
    out.append(repr([(s.name, int(s), repr(s), str(s)) for s in m.color.ColorSystem]))
    out.append(repr([(t.name, int(t), repr(t)) for t in m.color.ColorType]))
    for bad in ["nope", "#12345", "color(256)", "color(999)", "rgb(1,2)", "rgb(1,2,256)", "RGB(1,2,999)"]:
        out.append(attempt(lambda: C.parse(bad)))
    out.append(str(issubclass(m.color.ColorParseError, Exception)))
    return "\n".join(out)


def color_downgrades_sweep(m):
    C = m.color.Color
    CS = m.color.ColorSystem
    out = []
    for red in range(0, 256, 51):
        for green in range(0, 256, 85):
            for blue in range(0, 256, 64):
                color = C.from_rgb(red, green, blue)
                out.append(
                    (
                        color.downgrade(CS.EIGHT_BIT).number,
                        color.downgrade(CS.STANDARD).number,
                        color.downgrade(CS.WINDOWS).number,
                    )
                )
    for number in range(0, 256, 7):
        color = C.from_ansi(number)
        out.append((color.downgrade(CS.STANDARD).number, color.downgrade(CS.WINDOWS).number))
    return repr(out)


def color_printed(m):
    return printed(m, m.color.Color.parse("red"), m.color.Color.parse("#123456"), width=60)


# --- Text -------------------------------------------------------------------


def text_construction(m):
    T = m.text.Text
    S = m.style.Style
    Span = m.text.Span
    out = []
    for text in [
        T(),
        T("hello"),
        T("hello", style="bold"),
        T("hello", S(italic=True)),
        T("a\x07b\x08c\rd", justify="center", overflow="ellipsis", no_wrap=True, end="", tab_size=4),
        T("spans", spans=[Span(0, 2, "red"), Span(1, 3, S(bold=True))]),
    ]:
        out.append(repr(text))
        out.append(repr((text.plain, text.style, text.justify, text.overflow, text.no_wrap, text.end, text.tab_size)))
        out.append(repr((len(text), bool(text), str(text), text.cell_len, text.markup)))
        out.append(repr(text.spans))
    span = Span(2, 6, "bold")
    out.append(repr((span, bool(span), bool(Span(3, 3, "x")), span.split(1), span.split(4), span.split(6))))
    out.append(repr((span.move(3), span.right_crop(4), span.right_crop(9), span.extend(2), span.extend(0))))
    return "\n".join(out)


def text_classmethods(m):
    T = m.text.Text
    out = []
    for markup in [
        "[bold]bold[/] and [i]italic[/i]",
        "[red]red [blue]blue[/blue] red[/red]",
        "[b]a[i][/i]b[/b]",
        "no tags :smiley:",
        "[bold]:thumbs_up: open",
        "\\[escaped] [link=https://x]link[/link]",
    ]:
        text = T.from_markup(markup, style="dim", justify="right", end="")
        out.append(repr(text))
        out.append(repr((text.justify, text.end, text.markup)))
    out.append(repr(T.from_markup(":smiley:", emoji=False)))
    out.append(repr(T.from_markup(":smiley: :x-text:", emoji_variant="emoji").plain))
    out.append(attempt(lambda: T.from_markup("[/bold]")))
    out.append(attempt(lambda: T.from_markup("x [/]")))
    ansi = T.from_ansi("\x1b[1mbold\x1b[0m plain\n\x1b[31mred\x1b[0m\x1b[4m under", style="italic")
    out.append(repr(ansi))
    out.append(repr((ansi.tab_size, ansi.end)))
    out.append(repr(T.styled("styled", "bold red", justify="center")))
    assembled = T.assemble("plain ", ("bold", "bold"), T(" text", style="red"), (" none", None), style="dim", end="!")
    out.append(repr(assembled))
    out.append(repr((assembled.end, assembled.tab_size)))
    return "\n".join(out)


def text_editing(m):
    T = m.text.Text
    S = m.style.Style
    out = []
    text = T("Hello, World!")
    text.stylize("bold", 0, 5)
    text.stylize("red", -6, -1)
    text.stylize("", 0, 3)
    text.stylize("blue", 20)
    text.stylize("green", 5, 2)
    text.stylize_before("underline", 3)
    out.append(repr(text))
    text.plain = "Hello"
    out.append(repr(text))
    text.plain = "Hello there"
    out.append(repr(text))
    text.spans = [m.text.Span(0, 1, "x")]
    out.append(repr(text))
    text.style = "italic"
    text.justify = "full"
    text.overflow = "crop"
    text.no_wrap = True
    text.end = ""
    text.tab_size = 2
    out.append(repr((text.style, text.justify, text.overflow, text.no_wrap, text.end, text.tab_size)))
    copy = text.copy()
    blank = text.blank_copy("blank")
    out.append(repr((copy, copy.end, copy.tab_size, blank, blank.end, blank.justify)))

    text = T("abc", style="bold")
    out.append(repr(text.append("def", "red")))
    out.append(repr(text.append(T("ghi", style="blue"))))
    out.append(repr(text.append("")))
    out.append(repr(text.append("x", style=S())))
    out.append(repr(text.append_text(T("jk", spans=[m.text.Span(0, 1, "u")]))))
    out.append(repr(text.append_tokens([("l", "i"), ("m", None), ("", "s")])))
    out.append(attempt(lambda: text.append(T("x"), "bold")))
    out.append(attempt(lambda: text.append(1)))
    other = T("abcdefghijk")
    other.stylize("dim", 2, 4)
    text.copy_styles(other)
    out.append(repr(text))
    out.append(repr(T("a") + "b"))
    out.append(repr(T("a", style="red") + T("b", style="blue")))
    out.append(attempt(lambda: T("a") + 1).replace("rs_rich.text.", ""))
    return "\n".join(out)


def text_trimming(m):
    T = m.text.Text
    out = []
    text = T("some text   ", spans=[m.text.Span(0, 12, "red"), m.text.Span(5, 9, "bold")])
    text.rstrip()
    out.append(repr(text))
    text = T("a  \t ", spans=[m.text.Span(0, 5, "red")])
    text.rstrip_end(3)
    out.append(repr(text))
    text = T("日本語  ")
    text.rstrip_end(4)
    out.append(repr(text))
    for length in [0, 3, 10]:
        text = T("abcdef", spans=[m.text.Span(1, 6, "b")])
        text.set_length(length)
        out.append(repr(text))
    text = T("filename.py", spans=[m.text.Span(0, 11, "b")])
    text.remove_suffix(".py")
    text.remove_suffix(".rs")
    out.append(repr(text))
    text = T("abcdef", spans=[m.text.Span(1, 6, "b"), m.text.Span(5, 6, "i")])
    text.right_crop(2)
    out.append(repr(text))
    text = T("ab", spans=[m.text.Span(0, 2, "b"), m.text.Span(0, 1, "i")])
    text.extend_style(3)
    out.append(repr(text))
    text = T("plain")
    text.extend_style(2)
    out.append(repr(text))
    for width, overflow, pad in [(3, None, False), (3, "ellipsis", False), (8, None, True), (2, "crop", True), (1, "ignore", False)]:
        text = T("日本語abc", spans=[m.text.Span(0, 6, "red")])
        text.truncate(width, overflow=overflow, pad=pad)
        out.append(repr(text))
    for method, args in [("pad", (2,)), ("pad", (1, "*")), ("pad_left", (3, "-")), ("pad_right", (2,)), ("pad", (0,))]:
        text = T("mid", spans=[m.text.Span(0, 3, "red")])
        getattr(text, method)(*args)
        out.append(repr(text))
    out.append(attempt(lambda: T("x").pad(1, "ab")))
    for align in ["left", "center", "right"]:
        for width in [2, 9]:
            text = T("abcd", style="u")
            text.align(align, width, ".")
            out.append(repr(text))
    return "\n".join(out)


def text_highlighting(m):
    T = m.text.Text
    out = []
    text = T("foo 123 bar 456 foo")
    out.append(repr(text.highlight_regex(r"\d+", "bold")))
    out.append(repr(text.highlight_regex(re.compile(r"(?P<word>[a-z]+) (?P<num>\d+)"), style_prefix="repr.")))
    out.append(repr(text.highlight_regex("o+", lambda s: "red" if len(s) > 1 else None)))
    out.append(repr(text.highlight_regex("x")))
    out.append(repr(text.highlight_words(["foo", "BAR"], "italic")))
    out.append(repr(text.highlight_words(["foo", "BAR"], "underline", case_sensitive=False)))
    out.append(repr(text))
    unicode = T("日本語 text 日本")
    out.append(repr(unicode.highlight_regex("日本", "red")))
    out.append(repr(unicode))
    return "\n".join(out)


def text_splitting(m):
    T = m.text.Text
    out = []
    text = T("one\ntwo\n\nthree\n", style="dim", justify="right", no_wrap=True, end="!")
    text.stylize("bold", 2, 9)
    for kwargs in [{}, {"allow_blank": True}, {"include_separator": True}]:
        lines = text.split(**kwargs)
        out.append(repr(lines))
        out.append(repr([(line.justify, line.no_wrap, line.end) for line in lines]))
    out.append(repr(text.split("o")))
    out.append(repr(text.split("zz")))
    out.append(repr([line.end for line in text.split("zz")]))
    out.append(attempt(lambda: text.split("")))
    out.append(repr(text.divide([2, 5, 20])))
    out.append(repr(text.divide([])))
    out.append(repr(text.divide(iter([4]))))
    lines = text.split()
    out.append(repr((len(lines), lines[0], lines[-1], lines[0:2])))
    popped = lines.pop()
    out.append(repr((popped, len(lines), lines.pop(0))))
    lines.append(T("new"))
    lines.extend([T("more")])
    lines[0] = T("replaced")
    out.append(repr(list(lines)))
    out.append(repr(m.text.Lines([T("a"), T("b")])))
    joined = T(", ", style="dim", end="?").join([T("a", style="red"), T("b"), T("c", spans=[m.text.Span(0, 1, "u")])])
    out.append(repr((joined, joined.end)))
    out.append(repr(T("").join([T("x"), T("y")])))
    return "\n".join(out)


def text_wrapping(m):
    T = m.text.Text
    c = console(m, 40)
    out = []
    source = T("The quick brown fox jumps over the lazy dog. " * 2, spans=[m.text.Span(4, 9, "bold"), m.text.Span(10, 30, "red")])
    for justify in [None, "default", "left", "center", "right", "full"]:
        for overflow in [None, "fold", "crop", "ellipsis", "ignore"]:
            lines = source.wrap(c, 12, justify=justify, overflow=overflow)
            out.append(repr(lines))
    out.append(repr(T("abcdefghijklmnop qr").wrap(c, 5)))
    out.append(repr(T("abcdefghijklmnop qr").wrap(c, 5, no_wrap=True)))
    out.append(repr(T("a\tb\tc").wrap(c, 20, tab_size=4)))
    out.append(repr(T("日本語の文章です").wrap(c, 5)))
    full = T("word " * 9, style="on blue")
    full.stylize("red", 0, 10)
    out.append(repr(full.wrap(c, 14, justify="full")))
    out.append(repr(T("line one\nline two").fit(5)))
    out.append(repr(T("abc\ndefghij\n").fit(4)))
    lines = T("one two three four five six").wrap(c, 9)
    lines.justify(c, 12, "right")
    out.append(repr(lines))
    lines.justify(c, 12, "center")
    out.append(repr(lines))
    lines.justify(c, 14, "full")
    out.append(repr(lines))
    lines.justify(c, 14, "left", "ellipsis")
    out.append(repr(lines))
    return "\n".join(out)


def text_tabs_and_indents(m):
    T = m.text.Text
    out = []
    for tab_size in [None, 4, 1]:
        text = T("a\tbc\t\tdef\n\tx", spans=[m.text.Span(0, 4, "red"), m.text.Span(5, 9, "blue")])
        text.expand_tabs(tab_size)
        out.append(repr(text))
    text = T("x\ty", tab_size=2)
    text.expand_tabs()
    out.append(repr(text))
    code = "def f():\n    if x:\n        return 1\n\n    return 2\n"
    out.append(repr(T(code).detect_indentation()))
    out.append(repr(T("  a\n    b\n      c").detect_indentation()))
    out.append(repr(T("a\n b\n   c").detect_indentation()))
    out.append(repr(T("").detect_indentation()))
    out.append(repr(T(code).with_indent_guides()))
    out.append(repr(T(code, style="bold").with_indent_guides(2, character="|", style="red")))
    out.append(repr(T("\tif x:\n\t\ty\n").with_indent_guides()))
    return "\n".join(out)


def text_dunders(m):
    T = m.text.Text
    out = []
    text = T("héllo wörld", spans=[m.text.Span(0, 5, "bold"), m.text.Span(3, 9, "red")])
    for index in [0, 4, -1, -7]:
        out.append(repr(text[index]))
        out.append(repr(text[index].end))
    out.append(attempt(lambda: text[20]))
    for piece in [slice(1, 4), slice(None, 3), slice(-5, None), slice(4, 2), slice(0, 100)]:
        out.append(repr(text[piece]))
    out.append(attempt(lambda: text[::2]))
    out.append(repr(("llo" in text, T("wör") in text, "xyz" in text, 5 in text)))
    out.append(repr((text == T("héllo wörld"), text == text.copy(), text == "héllo wörld")))
    out.append(repr((T("a") == T("a"), T("a", style="b") == T("a"))))
    return "\n".join(out)


def text_styles_and_rendering(m):
    T = m.text.Text
    S = m.style.Style
    c = console(m, 40)
    out = []
    text = T("styled text here", style="italic")
    text.stylize("bold red", 0, 6)
    text.stylize(S(underline=True, color="blue"), 4, 11)
    text.stylize("no.such.style", 12)
    for offset in [0, 5, 10, -1]:
        out.append(repr(text.get_style_at_offset(c, offset)))
    out.append(repr(segments(text.render(c))))
    out.append(repr(segments(text.render(c, end="\n"))))
    out.append(repr(segments(T("plain").render(c, end="!"))))
    out.append(repr(segments(c.render(text))))
    out.append(repr(c.measure(T("a longer line\nshort words")).minimum))
    out.append(repr(tuple(c.measure(T("a longer line\nshort words")))))
    return "\n".join(out)


def text_printed(m):
    T = m.text.Text
    out = []
    text = T.from_markup("[bold]Bold[/] [italic red]italic red[/] :thumbs_up: plain words that wrap around")
    text.highlight_words(["words"], "reverse")
    text.stylize_before("on #202020", 0, 4)
    out.append(printed(m, text))
    out.append(printed(m, text, justify="full"))
    out.append(printed(m, T("centred", justify="center", style="on blue")))
    out.append(printed(m, T("tab\tstop\tend")))
    lines = T("one two three four five six seven").wrap(console(m), 10, justify="center")
    out.append(printed(m, lines))
    out.append(printed(m, T("x" * 50, overflow="ellipsis", no_wrap=True)))
    out.append(printed(m, T.from_ansi("\x1b[32mgreen\x1b[0m and \x1b[1;4mmore\x1b[0m")))
    out.append(printed(m, T("with guides\n    indented\n        more").with_indent_guides(4)))
    return "".join(out)


# --- Theme, markup, emoji ------------------------------------------------------


def themes(m):
    Th = m.theme.Theme
    S = m.style.Style
    out = []
    theme = Th({"warning": "bold red", "repr.number": S(color="blue"), "info": "dim cyan"})
    styles = theme.styles
    out.append(repr((len(styles), list(styles)[:3], list(styles)[-4:], styles["warning"], styles["repr.number"])))
    theme = Th({"b": "red", "a": "bold"}, inherit=False)
    out.append(repr(theme.styles))
    out.append(theme.config)
    out.append(attempt(lambda: Th({"x": "not-a-colour"})))
    config = "[styles]\nWarning = bold red\ninfo : dim cyan\n# comment\nlink = underline blue\n"
    loaded = Th.from_file(io.StringIO(config), inherit=False)
    out.append(repr(loaded.styles))
    out.append(repr(len(Th.from_file(io.StringIO(config)).styles)))
    out.append(attempt(lambda: Th.from_file(io.StringIO("[other]\nx = red\n"))))
    out.append(attempt(lambda: Th.from_file(io.StringIO("[styles]\nx = nope\n"))))
    stack = m.theme.ThemeStack(Th({"a": "red"}, inherit=False))
    out.append(repr((stack.get("a"), stack.get("b"), stack.get("b", "dflt"))))
    stack.push_theme(Th({"b": "blue"}, inherit=False))
    out.append(repr((stack.get("a"), stack.get("b"))))
    stack.push_theme(Th({"c": "green"}, inherit=False), inherit=False)
    out.append(repr((stack.get("a"), stack.get("c"))))
    stack.pop_theme()
    stack.pop_theme()
    out.append(attempt(stack.pop_theme))
    return "\n".join(out)


def theme_printing(m):
    theme = m.theme.Theme({"warning": "bold magenta", "repr.number": "underline"})
    c = m.console.Console(file=io.StringIO(), width=40, force_terminal=True, color_system="truecolor", theme=theme)
    c.print("[warning]careful[/] 123")
    return c.file.getvalue()


def markup_module(m):
    out = []
    for markup in ["[bold]x[/]", "plain :smiley:", "[red on blue]a[/] [link=https://x]b[/]", "[b]open"]:
        out.append(repr(m.markup.render(markup)))
        out.append(repr(m.markup.render(markup, style="dim", emoji=False)))
    out.append(attempt(lambda: m.markup.render("[/x]")))
    out.append(attempt(lambda: m.markup.render("a[/]")))
    tag = m.markup.Tag("bold", None)
    link = m.markup.Tag("link", "https://x")
    out.append(repr((tag, str(tag), tag.markup, link, str(link), link.markup, tuple(link))))
    out.append(m.markup.escape("[bold]not[/bold] \\"))
    out.append(repr(issubclass(m.markup.MarkupError, Exception)))
    return "\n".join(out)


def text_meta(m):
    T, S = m.text.Text, m.style.Style
    out = []
    t = T("hello world")
    t.apply_meta({"a": 1, "b": [1, "x"], "c": None, "d": 1.5, "e": True}, 2, 5)
    t.apply_meta({}, 0, 3)
    meta = {"x": 1}
    returned = t.on(meta, click="app.bell")
    out.append(repr((returned is t, meta)))
    out.append(repr([(s.start, s.end, s.style.meta) for s in t.spans]))
    a = T.assemble("a", ("b", "bold"), meta={"k": "v"})
    out.append(repr([(s.start, s.end, str(s.style), getattr(s.style, "meta", None)) for s in a.spans]))
    out.append(repr(S(bold=True, meta={"q": 2}).without_color.meta))
    out.append(repr(str(S(bold=True, meta={"q": 2}, link="x").clear_meta_and_links())))
    c = console(m)
    c.print(t, a)
    out.append(repr(c.file.getvalue()))
    return "\n".join(out)


def emoji_module(m):
    E = m.emoji.Emoji
    out = []
    for args in [("smiley",), ("thumbs_up", "bold"), ("heart", "red", "emoji"), ("heart", None, "text")]:
        emoji = E(*args)
        out.append(repr((repr(emoji), str(emoji), emoji.name, emoji.variant)))
    for name in ["no_such_emoji", "Smiley", "smiley-text", ""]:
        out.append(attempt(lambda: E(name)))
    out.append(repr(E.replace("hi :smiley: :x: :nope: :heart-text:")))
    out.append(repr(E.VARIANTS))
    out.append(repr(issubclass(m.emoji.NoEmoji, Exception)))
    # Rich renders an emoji as one segment with no newline, so consecutive
    # prints share a line.
    c = console(m)
    for emoji in [E("rocket", style="on blue"), E("smiley", style="repr.number")]:
        out.append(repr(segments(c.render(emoji))))
        c.print(emoji)
    c.print(E("heart"), "after", "end")
    out.append(repr(c.file.getvalue()))
    return "\n".join(out)


PROGRAMS = [
    text_meta,
    style_construction,
    style_parsing,
    style_arithmetic,
    style_meta,
    style_render,
    style_html,
    style_stack,
    color_values,
    color_constructors,
    color_downgrades_sweep,
    color_printed,
    text_construction,
    text_classmethods,
    text_editing,
    text_trimming,
    text_highlighting,
    text_splitting,
    text_wrapping,
    text_tabs_and_indents,
    text_dunders,
    text_styles_and_rendering,
    text_printed,
    themes,
    theme_printing,
    markup_module,
    emoji_module,
]


@pytest.mark.parametrize("program", PROGRAMS, ids=lambda p: p.__name__)
def test_matches_rich_15(program):
    assert program(RS) == program(RICH)


# --- rs_rich-only behaviour ------------------------------------------------------


def test_style_render_link_differs_only_in_its_id():
    style = RS.style.Style(bold=True, link="https://example.com")
    rendered = style.render("x")
    assert re.sub(r"id=[^;]*;", "", rendered) == "\x1b]8;https://example.com\x1b\\\x1b[1mx\x1b[0m\x1b]8;;\x1b\\"
    assert style.link_id


def test_meta_values_core_cannot_hold_are_refused_on_spans():
    text = RS.text.Text("click")
    with pytest.raises(TypeError, match="meta values"):
        text.apply_meta({"a": {"nested": 1}})
    # A Style keeps any marshal-able meta; only a span needs core's kinds.
    assert RS.style.Style(meta={"a": {"nested": 1}}).meta == {"a": {"nested": 1}}


def test_theme_read(tmp_path):
    path = tmp_path / "theme.ini"
    path.write_text("[styles]\nalert = bold red\n", encoding="utf-8")
    for m in [RICH, RS]:
        theme = m.theme.Theme.read(str(path), inherit=False)
        assert str(theme.styles["alert"]) == "bold red"


def test_color_rich_display():
    assert printed(RS, RS.color.Color.parse("red"), color=False) == printed(RICH, RICH.color.Color.parse("red"), color=False)


def test_value_types_are_tuples_and_int_enums():
    assert isinstance(RS.color.Color.parse("red"), tuple)
    assert RS.color.ColorSystem.TRUECOLOR == 3
    assert RS.text.Span(0, 1, "b") == (0, 1, "b")
    assert RS.markup.Tag("b", None) == ("b", None)


def test_text_is_unhashable_like_richs():
    with pytest.raises(TypeError):
        hash(RS.text.Text("x"))
