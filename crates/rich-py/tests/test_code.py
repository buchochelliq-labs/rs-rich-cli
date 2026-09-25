"""The code area: Markdown, Syntax, JSON, Pretty, inspect, Traceback and the
highlighters, compared byte for byte with rich 15.0.0.

Syntax colours come from the port's code highlighter (syntect, with its own
ANSI themes), not Pygments, so programs that highlight code are compared in
plain output only; see docs/python/syntax.md for the colour differences.
"""

from __future__ import annotations

import collections
import dataclasses
import importlib
import io
import re
import sys
import textwrap
from types import SimpleNamespace

import pytest

from test_compat import console

AREA = ["markdown", "syntax", "json", "pretty", "traceback", "highlighter", "text", "console"]


def modules(package: str) -> SimpleNamespace:
    loaded = {name: importlib.import_module(f"{package}.{name}") for name in AREA}
    return SimpleNamespace(package=importlib.import_module(package), **loaded)


def outputs(program, color: bool, width: int = 60) -> list:
    """Rich's output, then rs_rich's (Rich's random hyperlink ids removed)."""
    results = []
    for package in ["rich", "rs_rich"]:
        m = modules(package)
        c = console(m, color, width=width)
        program(m, c)
        results.append(c.file.getvalue())
    results[0] = re.sub("\x1b]8;id=[0-9]+;", "\x1b]8;;", results[0])
    return results


# --- data used by the programs --------------------------------------------


@dataclasses.dataclass
class Point:
    x: int
    y: float = 1.5
    tags: list = dataclasses.field(default_factory=lambda: ["a", "b"])


Pair = collections.namedtuple("Pair", "left right")


class RichRepr:
    def __rich_repr__(self):
        yield "positional"
        yield "keyword", 10
        yield "default", 5, 5
        yield "changed", 3, 4


class Angular:
    def __rich_repr__(self):
        yield "name", "value"

    __rich_repr__.angular = True  # type: ignore[attr-defined]


class Broken:
    def __repr__(self):
        raise ValueError("no repr")


def sample_data() -> dict:
    data = {
        "numbers": [1, "Hello World!", 100.123, 323.232, 432324.0, {5, 6, 7, (1, 2, 3, 4), 8}],
        "frozen": frozenset({1, 2, 3}),
        "default": collections.defaultdict(list, {"crumble": ["apple", "rhubarb", "butter"]}),
        "counter": collections.Counter(["apple", "pear", "pear"]),
        "atomic": (False, True, None),
        "pair": Pair(1, [2, 3]),
        "point": Point(3),
        "deque": collections.deque([1, 2], maxlen=5),
        "one": (1,),
        "empty": [],
        "empty_set": set(),
        "rich_repr": RichRepr(),
        "angular": Angular(),
        "broken": Broken(),
        "ordered": collections.OrderedDict(a=1),
        "bytes": b"\x00bytes",
    }
    data["self"] = data
    return data


# --- programs compared in colour and plain ----------------------------------


def printed_containers(m, c):
    c.print(sample_data())
    c.print([1, 2, 3], {"a": 1}, "text", Point(1))
    c.print(Point(1), highlight=False)
    c.print({"narrow": list(range(12))}, width=20)


def pretty_options(m, c):
    data = sample_data()
    c.print(m.pretty.Pretty(data, indent_guides=True, max_string=5, max_length=3))
    c.print(m.pretty.Pretty(data, max_depth=1, expand_all=True))
    c.print(m.pretty.Pretty("x" * 100, justify="right"))
    c.print(m.pretty.Pretty([1, 2], insert_line=True))
    c.print(m.pretty.Pretty(list(range(30)), insert_line=True, margin=40, indent_size=2))
    c.print(m.pretty.Pretty(data, highlighter=m.highlighter.NullHighlighter()))
    c.print(m.pretty.pretty_repr(data, max_width=30))
    m.pretty.pprint(data, console=c, max_length=4)
    m.pretty.pprint([1, [2, [3, [4]]]], console=c, max_depth=2, indent_guides=False)


def pretty_python_highlighter(m, c):
    def shout(text):
        text = m.highlighter.ReprHighlighter()(text)
        text.stylize("underline", 0, 1)
        return text

    c.print(m.pretty.Pretty({"key": [1, 2, 3]}, highlighter=shout))


def json_documents(m, c):
    document = '{"name": "apple", "count": 1, "tags": ["a", "b"], "nested": {"x": null, "y": true, "z": 1.5e10}, "esc": "a\\"b"}'
    c.print(m.json.JSON(document))
    c.print(m.json.JSON(document, indent=None))
    c.print(m.json.JSON(document, indent="\t", sort_keys=True))
    c.print(m.json.JSON.from_data({"key": [1, 2.5, False]}, indent=4))
    c.print(m.json.JSON.from_data([1, 2, "é"], ensure_ascii=True, highlight=False))


def highlighters(m, c):
    h = m.highlighter
    c.print(h.ReprHighlighter()("x = [1, 'two', None] at 0x1f /usr/bin/py https://a.b"))
    c.print(h.JSONHighlighter()('{"a": [1, true, null], "b" : "c", "d":\n "e"}'))
    c.print(h.ISO8601Highlighter()("2024-01-02T03:04:05Z"))
    c.print(h.NullHighlighter()("1 2 3"))

    class Vowels(h.RegexHighlighter):
        base_style = "repr."
        highlights = [r"(?P<number>\d+)", r"(?P<str>[aeiou])"]

    c.print(Vowels()("abc 123 def"))
    c.print(Vowels()(m.text.Text("xx 99 ou")))

    class FirstWord(h.Highlighter):
        def highlight(self, text):
            text.stylize("bold red", 0, 5)

    c.print(FirstWord()("hello world"))


MARKDOWN = """# Title

Some *emphasis*, **strong**, ~~struck~~ and `code`.

* one
* two
  * nested

1. first
2. second

> a quote

---

| a | b |
|:--|--:|
| 1 | 2 |

[link](https://example.com)
"""


def markdown_documents(m, c):
    c.print(m.markdown.Markdown(MARKDOWN))
    c.print(m.markdown.Markdown(MARKDOWN, hyperlinks=False, justify="center"))
    c.print(m.markdown.Markdown("plain *text* here", style="bold"))


def inspected(m, c):
    class Thing:
        """A thing.

        With more to say."""

        size = [1, 2, 3]

        def __init__(self):
            self.name = "hello"
            self.count = 3
            self._hidden = {"a": 1}

        def __repr__(self):
            return "Thing()"

        @property
        def broken(self):
            raise ValueError("nope")

    m.package.inspect(Thing(), console=c)
    m.package.inspect(Thing(), console=c, private=True, docs=False, title="[b]Custom")
    m.package.inspect([1, 2, 3], console=c, value=True)
    m.package.inspect(3.5, console=c, value=False, docs=False)


COLOR_PROGRAMS = [
    printed_containers,
    pretty_options,
    pretty_python_highlighter,
    json_documents,
    highlighters,
    markdown_documents,
    inspected,
]


@pytest.mark.parametrize("color", [True, False], ids=["truecolor", "plain"])
@pytest.mark.parametrize("program", COLOR_PROGRAMS, ids=lambda p: p.__name__)
def test_output_matches_rich_byte_for_byte(program, color):
    expected, actual = outputs(program, color)
    assert actual == expected


@pytest.mark.parametrize("width", [20, 35, 100])
def test_pretty_layout_matches_rich_at_any_width(width):
    expected, actual = outputs(pretty_options, False, width=width)
    assert actual == expected


# --- programs compared in plain output (code colours are the engine's) ------

CODE = "def f(x):\n\tif x:\n        return [1, 'a']  # c\n    return None\n"


def syntax_layouts(m, c):
    S = m.syntax.Syntax
    for theme in ["monokai", "ansi_dark"]:
        c.print(S(CODE, "python", theme=theme))
        c.print(S(CODE, "python", theme=theme, padding=1))
        c.print(S(CODE.rstrip(), "python", theme=theme, line_numbers=True))
        c.print(S(CODE, "python", theme=theme, line_numbers=True, start_line=98, highlight_lines={99}))
        c.print(S(CODE, "python", theme=theme, line_range=(2, 3)))
        c.print(S(CODE, "python", theme=theme, line_numbers=True, line_range=(2, 3)))
        c.print(S(CODE, "python", theme=theme, word_wrap=True, code_width=12))
        c.print(S(CODE, "python", theme=theme, line_numbers=True, word_wrap=True, code_width=10))
        c.print(S(CODE, "python", theme=theme, indent_guides=True))
        c.print(S(CODE, "python", theme=theme, indent_guides=True, line_numbers=True))
        c.print(S("    x = 1\n    y = 2", "python", theme=theme, dedent=True))
        c.print(S(CODE, "python", theme=theme, padding=(1, 2, 0, 3), line_numbers=True))
        c.print(S(CODE, "python", theme=theme, background_color="red"))
        styled = S(CODE, "python", theme=theme, line_numbers=True)
        styled.stylize_range("bold", (2, 4), (2, 8))
        c.print(styled)
    c.print(S(CODE, "python").highlight(CODE).plain)
    c.print(S(CODE, "python").highlight(CODE, line_range=(2, 3)).plain)


def markdown_code(m, c):
    c.print(m.markdown.Markdown("```python\ndef f(x):\n    return x + 1\n```\n\nand `inline` code"))
    c.print(m.markdown.Markdown("use `x = 1` here", inline_code_lexer="python"))


def raise_and_catch(fn):
    try:
        fn()
    except Exception:
        return sys.exc_info()
    raise AssertionError("expected an exception")


def divide(x):
    data = {"a": [1, 2, 3], "b": "text"}  # noqa: F841 - a local for show_locals
    return 1 / x


def wrap_error(x):
    try:
        return divide(x)
    except ZeroDivisionError as error:
        raise ValueError("bad value") from error


def recurse(n):
    if n == 0:
        return divide(0)
    return recurse(n - 1)


def tracebacks(m, c):
    T = m.traceback.Traceback
    info = raise_and_catch(lambda: wrap_error(0))
    c.print(T.from_exception(*info))
    c.print(T.from_exception(*info, width=60, extra_lines=1, indent_guides=False))
    c.print(T.from_exception(*raise_and_catch(lambda: divide(0)), show_locals=True))
    c.print(T.from_exception(*raise_and_catch(lambda: recurse(30)), max_frames=6))
    c.print(T.from_exception(*raise_and_catch(lambda: compile("x = (1,\n", "<string>", "exec"))))
    c.print(T.from_exception(*raise_and_catch(lambda: {}["missing"]), word_wrap=True))
    try:
        wrap_error(0)
    except Exception:
        c.print(T())
        c.print_exception(extra_lines=0)


def exception_groups_and_notes(m, c):
    T = m.traceback.Traceback

    def grouped():
        raise ExceptionGroup("group", [ValueError("one"), TypeError("two")])

    def noted():
        try:
            1 / 0
        except ZeroDivisionError as error:
            error.add_note("a note")
            raise

    c.print(T.from_exception(*raise_and_catch(noted)))
    if sys.version_info >= (3, 11):
        c.print(T.from_exception(*raise_and_catch(grouped)))


PLAIN_PROGRAMS = [syntax_layouts, markdown_code, tracebacks, exception_groups_and_notes]


@pytest.mark.parametrize("program", PLAIN_PROGRAMS, ids=lambda p: p.__name__)
def test_plain_output_matches_rich(program):
    expected, actual = outputs(program, False, width=100)
    assert actual == expected


def ansi_code_without_whitespace(m, c):
    # No whitespace or comment tokens: those are where syntect's ANSI themes
    # differ from Pygments'.
    c.print(m.syntax.Syntax("x=[1,'a']", "python", theme="ansi_dark", line_numbers=True))


def test_ansi_theme_colours_match_rich_where_the_tokens_do():
    expected, actual = outputs(ansi_code_without_whitespace, True)
    assert actual == expected


SIGNED_LINES = [
    {"line_numbers": True, "start_line": 0},
    {"line_numbers": True, "start_line": -3, "highlight_lines": {-2, 0}},
    {"line_numbers": True, "start_line": -12},
    {"line_numbers": True, "line_range": (0, 2)},
    {"line_numbers": True, "line_range": (-2, 3)},
    {"line_numbers": True, "line_range": (2, -1)},
    {"line_numbers": True, "line_range": (None, -1)},
    {"line_numbers": True, "line_range": (3, 0)},
    {"line_numbers": True, "line_range": (None, None)},
    {"line_range": (1, -2)},
    {"line_range": (-5, None), "word_wrap": True},
    {"line_numbers": True, "start_line": -1, "line_range": (2, 4), "word_wrap": True},
    {"line_numbers": True, "start_line": -2, "padding": 1, "background_color": "red"},
]


def signed_line_numbers(m, c):
    # Plain text in `ansi_dark`: its colours are Rich's in either engine.
    for options in SIGNED_LINES:
        c.print(m.syntax.Syntax(CODE, "text", theme="ansi_dark", **options))
    styled = m.syntax.Syntax(CODE, "text", theme="ansi_dark", line_numbers=True)
    styled.stylize_range("bold", (-1, 2), (2, 3))
    styled.stylize_range("reverse", (2, -4), (3, 2))
    c.print(styled)
    for line_range in [(-1, -2), (2, -1), (0, 0), (None, -1), (3, None)]:
        text = m.syntax.Syntax(CODE, "text", theme="ansi_dark").highlight(CODE, line_range=line_range)
        c.print(repr(text.plain), text.no_wrap, text.tab_size, markup=False)


def test_signed_line_numbers_and_ranges_match_rich():
    expected, actual = outputs(signed_line_numbers, True, width=40)
    assert actual == expected


# --- API ---------------------------------------------------------------------


def test_builtin_highlight_patterns_are_rich_s():
    import rich.highlighter as upstream

    from rs_rich import highlighter

    for name in ["ReprHighlighter", "JSONHighlighter", "ISO8601Highlighter"]:
        ours, theirs = getattr(highlighter, name), getattr(upstream, name)
        assert list(ours.highlights) == list(theirs.highlights), name
        assert ours.base_style == theirs.base_style
    assert highlighter.JSONHighlighter.JSON_STR == upstream.JSONHighlighter.JSON_STR


def test_highlighter_needs_text_and_an_implementation():
    from rs_rich.highlighter import Highlighter, ReprHighlighter

    with pytest.raises(TypeError, match="str or Text instance required"):
        ReprHighlighter()(42)
    with pytest.raises(NotImplementedError):
        Highlighter()("x")


def test_a_failing_python_highlighter_raises_from_print():
    from rs_rich.pretty import Pretty

    def broken(text):
        raise RuntimeError("highlighter failed")

    with pytest.raises(RuntimeError, match="highlighter failed"):
        console(modules("rs_rich"), False).print(Pretty([1], highlighter=broken))


def test_node_and_traverse():
    from rs_rich.pretty import Node, pretty_repr, traverse

    node = traverse({"a": [1, 2]})
    assert str(node) == "{'a': [1, 2]}"
    assert node.render(max_width=8) == "{\n    'a': [\n        1,\n        2\n    ]\n}"
    assert pretty_repr(node, max_width=10) == node.render(max_width=10)
    assert list(Node(value_repr="x").iter_tokens()) == ["x"]
    custom = Node(open_brace="(", close_brace=")", children=[Node(value_repr="1", last=True)], is_tuple=True)
    assert str(custom) == "(1,)"


def test_pretty_install_sets_the_display_hook(monkeypatch):
    import builtins

    from rs_rich.console import Console
    from rs_rich.pretty import install

    monkeypatch.setattr(sys, "displayhook", sys.displayhook)
    out = io.StringIO()
    install(console=Console(file=out, width=40, color_system=None), max_length=2)
    sys.displayhook([1, 2, 3])
    sys.displayhook(None)
    assert out.getvalue() == "[1, 2, ... +1]\n"
    assert builtins._ == [1, 2, 3]


def test_traceback_install_sets_the_excepthook(monkeypatch):
    from rs_rich.console import Console
    from rs_rich.traceback import install

    monkeypatch.setattr(sys, "excepthook", sys.excepthook)
    out = io.StringIO()
    previous = install(console=Console(file=out, width=80, color_system=None))
    assert previous is not sys.excepthook
    sys.excepthook(*raise_and_catch(lambda: divide(0)))
    assert "ZeroDivisionError: division by zero" in out.getvalue()
    assert "Traceback (most recent call last)" in out.getvalue()


def test_traceback_outside_except_is_a_value_error():
    from rs_rich.traceback import Traceback

    with pytest.raises(ValueError, match="Value for 'trace' required"):
        Traceback()


def test_extract_builds_rich_s_data_classes():
    from rs_rich.traceback import Frame, Stack, Trace, Traceback

    trace = Traceback.extract(*raise_and_catch(lambda: wrap_error(0)), show_locals=True)
    assert isinstance(trace, Trace)
    assert [stack.exc_type for stack in trace.stacks] == ["ValueError", "ZeroDivisionError"]
    assert trace.stacks[0].is_cause is False and trace.stacks[1].is_cause is True
    frame = trace.stacks[1].frames[-1]
    assert isinstance(frame, Frame) and frame.name == "divide"
    assert str(frame.locals["data"]) == "{'a': [1, 2, 3], 'b': 'text'}"
    made = Trace(stacks=[Stack(exc_type="E", exc_value="v")])
    out = io.StringIO()
    modules("rs_rich").console.Console(file=out, width=40, color_system=None).print(Traceback(made))
    assert out.getvalue() == "E: v\n"


def test_code_highlighter_choice():
    from rs_rich.markdown import Markdown
    from rs_rich.syntax import Syntax, code_highlighters, code_themes

    assert "syntect" in code_highlighters()
    assert {"ansi_dark", "ansi_light", "base16-ocean.dark"} <= set(code_themes())
    code = "x = 1\n"
    default = console(modules("rs_rich"), True)
    chosen = console(modules("rs_rich"), True)
    default.print(Syntax(code, "python"))
    chosen.print(Syntax(code, "python", highlighter="syntect"))
    assert chosen.file.getvalue() == default.file.getvalue()
    assert Syntax(code, "python", highlighter="syntect").highlighter == "syntect"
    with pytest.raises(ValueError, match="unknown code highlighter 'nope'"):
        Syntax(code, "python", highlighter="nope")
    with pytest.raises(ValueError, match="unknown code highlighter"):
        Markdown("x", highlighter="nope")
    if "lumis" in code_highlighters():
        lumis = console(modules("rs_rich"), True)
        lumis.print(Syntax(code, "python", highlighter="lumis"))
        assert "x" in lumis.file.getvalue()


def test_syntax_helpers(tmp_path):
    from rs_rich.syntax import Syntax

    path = tmp_path / "example.py"
    path.write_text("print('hi')\n")
    syntax = Syntax.from_path(str(path), line_numbers=True)
    assert syntax.lexer == "python"
    assert syntax.code == "print('hi')\n"
    assert Syntax.guess_lexer("x.rs") == "rust"
    assert Syntax.guess_lexer("x.unknown-extension") == "default"
    assert Syntax.get_theme("ansi_dark") == "ansi_dark"
    text = Syntax("a = 1", "python").highlight("a = 1")
    assert text.plain == "a = 1\n"


def test_json_text_is_a_copy():
    from rs_rich.json import JSON

    document = JSON('{"a": 1}')
    assert document.text.plain == '{\n  "a": 1\n}'
    document.text.append("changed")
    assert document.text.plain == '{\n  "a": 1\n}'


def test_markdown_attributes():
    from rs_rich.markdown import Markdown

    document = Markdown("# hi", code_theme="ansi_dark")
    assert document.markup == "# hi"
    assert document.inline_code_theme == "ansi_dark"
    assert document.hyperlinks is True


def test_inspect_class_and_repr():
    from rs_rich._native import Inspect

    out = io.StringIO()
    modules("rs_rich").console.Console(file=out, width=60, color_system=None).print(
        Inspect(textwrap, methods=False, dunder=False, private=False, all=False, docs=False)
    )
    assert out.getvalue().startswith("╭─ <module 'textwrap'")


# Rust's own output for this document: core `Markdown` with rich-mermaid's
# `MermaidFences` (default options) as its fence renderer, 60 cells wide, no
# colour (a small Rust program over the two crates; Rich has no fences).
MERMAID_DOCUMENT = "# Flow\n\n```mermaid\nflowchart LR\n    A[Start] --> B{Check}\n    B --> C[Done]\n```\n\nAfter.\n"
MERMAID_EXPECTED = (
    "                            Flow                            \n"
    "\n"
    "┌───────┐  ╱───────╲  ┌──────┐\n"
    "│ Start ├─►< Check >─►│ Done │\n"
    "└───────┘  ╲───────╱  └──────┘\n"
    "\n"
    "After.                                                      \n"
)


def test_markdown_fences_draw_mermaid_as_rust_does():
    from rs_rich.markdown import Markdown
    from rs_rich.mermaid import MermaidFences

    c = console(modules("rs_rich"), False)
    c.print(Markdown(MERMAID_DOCUMENT, fences=[MermaidFences()]))
    assert c.file.getvalue() == MERMAID_EXPECTED
    plain = console(modules("rs_rich"), False)
    plain.print(Markdown(MERMAID_DOCUMENT))
    assert "flowchart LR" in plain.file.getvalue()


def test_markdown_fences_take_python_renderers():
    from rs_rich.markdown import Markdown

    def shout(language, code):
        return code.upper() if language == "shout" else None

    c = console(modules("rs_rich"), False, width=20)
    document = Markdown("```shout\nhello\n```\n\n```python\nx\n```", fences=[shout])
    c.print(document)
    assert c.file.getvalue().startswith("HELLO")
    assert document.fences == [shout]
    with pytest.raises(TypeError, match="fence renderer"):
        Markdown("x", fences=[42])


def test_a_python_code_highlighter_object():
    from rs_rich.syntax import Syntax

    class Upper:
        def highlight(self, code, language, theme):
            return []  # no spans: plain text

        def default_theme(self):
            return "plain"

        def themes(self):
            return ["plain"]

    engine = Upper()
    syntax = Syntax("x = 1", "python", highlighter=engine)
    assert syntax.highlighter is engine
    c = console(modules("rs_rich"), False, width=10)
    c.print(syntax)
    assert c.file.getvalue() == "x = 1\n"


def test_deep_nesting_pretty_prints_as_rich_does_instead_of_crashing():
    # The walk recurses: past a few hundred levels it once overflowed the
    # native stack. Rich prints a list this deep, with a repr-error where its
    # recursive walk runs out of Python frames; so does rs_rich, at about the
    # same depth (it counts the caller's frames; C calls on the stack can
    # count in Python's limit too, so the depth can differ by a level or two).
    import rich.pretty
    from rs_rich.pretty import pretty_repr, traverse

    deep = []
    inner = deep
    for _ in range(1500):
        inner.append([])
        inner = inner[0]
    for pretty in [rich.pretty.pretty_repr, pretty_repr]:
        text = pretty(deep)
        assert "<repr-error 'maximum recursion depth exceeded" in text
    depth = pretty_repr(deep).count("[")
    assert abs(depth - rich.pretty.pretty_repr(deep).count("[")) <= 3
    node = traverse(deep)
    assert node == traverse(deep) and node.children[0].children is not None
    c = console(modules("rs_rich"), False, width=80)
    c.print(deep[0][0][0][0][0][0][0][0][0][0])
    assert c.file.getvalue().startswith("[\n    [\n")
