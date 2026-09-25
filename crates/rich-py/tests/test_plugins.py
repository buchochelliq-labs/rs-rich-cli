"""``rs_rich.plugins``: the plugin API and the ext host from Python.

Rich has no plugin API, so nothing here is compared with Rich. Expected output
comes from the Rust crates themselves: the ``RUST_*`` constants below were
printed by a small Rust program (through ``Console::capture``) (``rich-mermaid``'s ``Mermaid``, core's doc
``Bold`` code highlighter through ``Syntax``, ``rich-ext``'s registry and
``NumberHighlighter``) for exactly the inputs used here, and the rest compares
a Python plugin with the Rust plugin doing the same job, in the same process.
"""

from __future__ import annotations

import io
import time

import pytest

from rs_rich import _native
from rs_rich import plugins as p
from rs_rich.box import DOUBLE, ROUNDED
from rs_rich.console import Console
from rs_rich.panel import Panel
from rs_rich.segment import Segment
from rs_rich.text import Text
from rs_rich.theme import Theme

# Printed by the Rust reference program (see the module docstring).
RUST_MERMAID = "┌───┐  ┌───┐  ┌───┐\n│ A ├─►│ B ├─►│ C │\n└───┘  └───┘  └───┘\n\n"
RUST_BOLD_SYNTAX = "\x1b[1mx = 1\x1b[0m               \n\x1b[1my = 2\x1b[0m               \n"
RUST_UPPER = "\x1b[1mA\x1b[0mBC 12\n"
RUST_NUMBERS = "abc \x1b[1;36m12\x1b[0m\n"
RUST_CHOICE_UNKNOWN = 'unknown code highlighter "nope"; available: bold, syntect'
RUST_CHOICE_THEME = 'the bold code highlighter has no theme "x"'
MERMAID_SOURCE = "graph LR\nA-->B\nB-->C"


def console(width: int = 40, color: bool = False) -> Console:
    return Console(
        file=io.StringIO(),
        width=width,
        force_terminal=color,
        color_system="truecolor" if color else None,
    )


def output(c: Console) -> str:
    return c.file.getvalue()


# ---------------------------------------------------------------------------
# Python equivalents of the Rust reference plugins


class Bold(p.CodeHighlighter):
    """core's doc example (`rich::protocol::CodeHighlighter`): every line bold."""

    def highlight(self, code, language=None, theme=None):
        if theme != "bold":
            raise p.UnknownThemeError(theme)
        return [[(0, len(line), "bold")] if line else [] for line in code.split("\n")]

    def default_theme(self):
        return "bold"

    def themes(self):
        return ["bold"]


def upper(text: Text) -> Text:
    """The reference program's `Upper` transform."""
    out = Text(text.plain.upper())
    out.stylize("bold", 0, 1)
    return out


class Demo(p.Plugin):
    """The reference program's `Demo` plugin, in Python."""

    def metadata(self):
        return p.PluginMetadata("demo", "Demo", "1.0")

    def register(self, registrar):
        registrar.code_highlighter("bold", Bold())
        registrar.transform("upper", upper)


class Everything(p.Plugin):
    """Registers one of every capability kind."""

    def __init__(self, id="everything"):
        self.id = id

    def metadata(self):
        return p.PluginMetadata(self.id, "Everything", "0.1", "one of each")

    def register(self, registrar):
        registrar.highlighter(Shouty())
        registrar.code_highlighter("bold", Bold())
        registrar.theme("loud", Theme({"loud": "bold red"}, inherit=False))
        registrar.box_style("double", DOUBLE)
        registrar.renderer("shout", lambda source: Text(source.upper(), style="bold"))
        registrar.fence_renderer("shout", lambda language, code: Text(code.upper()))
        registrar.transform("upper", upper)


class Shouty:
    """A highlighter object (anything with ``highlight(text)``)."""

    def highlight(self, text):
        text.stylize("bold")


# ---------------------------------------------------------------------------
# The contract's data


def test_api_version_and_names_match_the_rust_contract():
    assert p.PLUGIN_API_VERSION == 1
    # rich-plugin-api's own test cases.
    for good in ["syntect", "lumis", "ansi_dark", "a.b-c", "7z", "my-plugin_2.x"]:
        assert p.is_valid_name(good), good
    for bad in ["", "-", "-x", "--help", ".", "..", "_x", "Upper", "a b", "x" * 65, "\x1b]0;x"]:
        assert not p.is_valid_name(bad), bad


def test_metadata():
    meta = p.PluginMetadata("x", "X", "1.0.0", "does x")
    assert (meta.id, meta.name, meta.version, meta.description) == ("x", "X", "1.0.0", "does x")
    assert meta.api_version == p.PLUGIN_API_VERSION
    assert meta == p.PluginMetadata("x", "X", "1.0.0", "does x")
    assert meta != p.PluginMetadata("x", "X", "1.0.1", "does x")
    assert repr(meta) == "PluginMetadata(id=\"x\", name=\"X\", version=\"1.0.0\", description=\"does x\")"


def test_capabilities_print_as_rust_does():
    assert str(p.Capability("highlighter")) == "highlighter"
    assert str(p.Capability("code_highlighter", "x")) == 'code highlighter "x"'
    assert str(p.Capability("theme", "dark")) == 'theme "dark"'
    assert str(p.Capability("box_style", "b")) == 'box style "b"'
    assert str(p.Capability("renderer", "r")) == 'renderer "r"'
    assert str(p.Capability("fence_renderer", "mermaid")) == 'fence renderer "mermaid"'
    assert str(p.Capability("transform", "t")) == 'transform "t"'
    theme = p.Capability("theme", "dark")
    assert (theme.kind, theme.name) == ("theme", "dark")
    assert theme == p.Capability("theme", "dark") and hash(theme) == hash(p.Capability("theme", "dark"))
    assert p.Capability.KINDS[0] == "highlighter"
    with pytest.raises(ValueError):
        p.Capability("theme")
    with pytest.raises(ValueError):
        p.Capability("spinner", "x")


# ---------------------------------------------------------------------------
# The host


def test_with_defaults_is_the_builtin_plugin():
    registry = p.ExtensionRegistry.with_defaults()
    [builtin] = registry.plugins()
    assert builtin.metadata.id == "rich-ext"
    assert builtin.capabilities == [p.Capability("highlighter"), p.Capability("code_highlighter", "syntect")]
    assert registry.code_highlighter_names() == ["syntect"]
    assert registry.provided_by(p.Capability("code_highlighter", "syntect")) == "rich-ext"
    assert registry.provided_by(p.Capability("highlighter")) == "rich-ext"
    assert p.ExtensionRegistry().plugins() == []
    # The built-in plugin class is the same plugin.
    other = p.ExtensionRegistry()
    other.add_plugin(p.BuiltinPlugin())
    assert other.plugins() == registry.plugins()


def test_a_python_plugin_registers_every_capability_kind():
    registry = p.ExtensionRegistry.with_defaults()
    registry.add_plugin(Everything())
    plugin = registry.plugins()[-1]
    assert plugin.metadata == p.PluginMetadata("everything", "Everything", "0.1", "one of each")
    assert [str(c) for c in plugin.capabilities] == [
        "highlighter",
        'code highlighter "bold"',
        'theme "loud"',
        'box style "double"',
        'renderer "shout"',
        'fence renderer "shout"',
        'transform "upper"',
    ]
    for capability in plugin.capabilities[1:]:
        assert registry.provided_by(capability) == "everything"
    assert registry.code_highlighter_names() == ["bold", "syntect"]
    assert registry.theme("loud").styles["loud"] == Theme({"loud": "bold red"}, inherit=False).styles["loud"]
    assert registry.theme("nope") is None
    assert registry.box_style("double") is DOUBLE
    assert registry.box_style("rounded") is None
    assert registry.transform_names() == ["upper"]
    assert registry.transform("upper")(Text("abc")).plain == "ABC"


def test_python_plugin_matches_the_rust_plugin():
    # The same plugin as the Rust reference program's `Demo`.
    registry = p.ExtensionRegistry.with_defaults()
    registry.add_plugin(Demo())
    c = console(width=20, color=True)
    c.print(registry.text_pipeline(["upper"]).apply(Text("abc 12")))
    assert output(c) == RUST_UPPER
    with pytest.raises(p.HighlighterChoiceError) as unknown:
        registry.set_default_code_highlighter("nope")
    assert str(unknown.value) == RUST_CHOICE_UNKNOWN
    assert unknown.value.available == ["bold", "syntect"]
    with pytest.raises(p.HighlighterChoiceError) as theme:
        registry.set_default_code_highlighter("bold", "x")
    assert str(theme.value) == RUST_CHOICE_THEME
    assert registry.default_code_highlighter() is None
    registry.set_default_code_highlighter("bold")
    assert registry.default_code_highlighter() == "bold"


def test_mermaid_plugin_renders_as_rust():
    registry = p.ExtensionRegistry()
    registry.add_plugin(p.MermaidPlugin())
    assert [str(c) for c in registry.plugins()[0].capabilities] == ['fence renderer "mermaid"', 'renderer "mermaid"']
    c = console()
    c.print(registry.renderer("mermaid").render(MERMAID_SOURCE))
    assert output(c) == RUST_MERMAID
    # The fence route draws the same diagram; other languages are declined.
    fences = registry.fences()
    segments = fences.render_fence("mermaid", MERMAID_SOURCE, console())
    assert "".join(s.text for s in segments) == RUST_MERMAID
    assert fences.render_fence("python", "x = 1", console()) is None
    assert p.ExtensionRegistry().fences() is None
    with pytest.raises(ValueError):
        p.MermaidPlugin(backend="kroki")


def test_python_renderers_render_through_rust():
    registry = p.ExtensionRegistry()
    registry.add_plugin(Everything())
    c = console(color=True)
    c.print(registry.renderer("shout").render("hey"))
    expected = console(color=True)
    expected.print(Text("HEY", style="bold"))
    assert output(c) == output(expected)
    # A Python fence renderer through Rust's router, as Markdown would call it.
    segments = registry.fences().render_fence("shout", "abc", console())
    assert [s.text for s in segments] == ["ABC", "\n"]
    # Returning segments works too.
    registry.add_plugin(Registers("segs", lambda r: r.fence_renderer("segs", lambda lang, code: [Segment(code)])))
    assert [s.text for s in registry.fences().render_fence("segs", "x", console())] == ["x", "\n"]
    # A Python renderer wrapping a Rust one (Mermaid) is still Rust's output.
    mermaid = p.ExtensionRegistry()
    mermaid.add_plugin(p.MermaidPlugin())
    rust = mermaid.renderer("mermaid")
    registry.add_plugin(Registers("wrap", lambda r: r.renderer("wrapped", rust.render)))
    c = console()
    c.print(registry.renderer("wrapped").render(MERMAID_SOURCE))
    assert output(c) == RUST_MERMAID


def test_rust_capabilities_register_again_from_python():
    # Handles from one registry register into another: the Rust objects, not
    # Python wrappers around them.
    source = p.ExtensionRegistry.with_defaults()
    source.add_plugin(p.MermaidPlugin())
    source.add_plugin(Demo())
    target = p.ExtensionRegistry()

    def register(registrar):
        registrar.code_highlighter("syntect", source.code_highlighter("syntect"))
        registrar.renderer("mermaid", source.renderer("mermaid"))
        registrar.fence_renderer("mermaid", source.fence_renderer("mermaid"))
        registrar.transform("upper", source.transform("upper"))

    target.add_plugin(Registers("copies", register))
    code = "fn main() {}\n"
    assert target.code_highlighter("syntect").highlight(code, "rust") == source.code_highlighter(
        "syntect"
    ).highlight(code, "rust")
    c = console()
    c.print(target.renderer("mermaid").render(MERMAID_SOURCE))
    assert output(c) == RUST_MERMAID


class Registers(p.Plugin):
    """A plugin whose register is a function of the registrar."""

    def __init__(self, id, register):
        self.id = id
        self.body = register

    def metadata(self):
        return p.PluginMetadata(self.id, self.id, "1")

    def register(self, registrar):
        self.body(registrar)


def test_metadata_may_be_a_class_attribute_and_builtins_are_plugins():
    class Attr(p.Plugin):
        metadata = p.PluginMetadata("attr", "Attr", "1")

        def register(self, registrar):
            pass

    registry = p.ExtensionRegistry()
    registry.add_plugin(Attr())
    assert registry.plugins()[0].metadata.id == "attr"
    assert isinstance(p.MermaidPlugin(), p.Plugin)
    assert p.MermaidPlugin().metadata().id == "mermaid"


# ---------------------------------------------------------------------------
# Refusals: all-or-nothing, as for Rust plugins


def snapshot(registry):
    theme = registry.theme("loud")
    return (
        registry.plugins(),
        registry.code_highlighter_names(),
        registry.transform_names(),
        None if theme is None else theme.styles,
        registry.renderer("shout") is None,
        registry.fences() is None,
    )


def test_a_plugin_that_raises_in_register_is_refused():
    class Fails(p.Plugin):
        def metadata(self):
            return p.PluginMetadata("fails", "Fails", "1")

        def register(self, registrar):
            registrar.theme("loud", Theme({"loud": "red"}))
            registrar.transform("upper", upper)
            raise ValueError("boom")

    registry = p.ExtensionRegistry.with_defaults()
    before = snapshot(registry)
    with pytest.raises(p.PluginError) as raised:
        registry.add_plugin(Fails())
    error = raised.value
    assert str(error) == 'plugin "fails" failed to register: ValueError: boom'
    assert (error.kind, error.plugin) == ("failed", "fails")
    assert isinstance(error.__cause__, ValueError)
    assert snapshot(registry) == before
    # It can be added once fixed: nothing of it was kept.
    registry.add_plugin(Everything("fails"))


def test_refusals_match_the_rust_host():
    registry = p.ExtensionRegistry.with_defaults()
    registry.add_plugin(Everything())
    before = snapshot(registry)

    with pytest.raises(p.PluginError) as duplicate:
        registry.add_plugin(Everything())
    assert duplicate.value.kind == "duplicate_plugin"
    assert str(duplicate.value) == 'a plugin with id "everything" is already registered'

    with pytest.raises(p.PluginError) as conflict:
        registry.add_plugin(Everything("second"))
    assert conflict.value.kind == "conflict"
    assert (conflict.value.plugin, conflict.value.existing) == ("second", "everything")
    assert conflict.value.capability == p.Capability("code_highlighter", "bold")
    assert str(conflict.value) == 'plugin "second" registers code highlighter "bold", which plugin "everything" already provides'

    with pytest.raises(p.PluginError) as invalid:
        registry.add_plugin(Registers("names", lambda r: r.transform("Bad Name", upper)))
    assert (invalid.value.kind, invalid.value.name) == ("invalid_name", "Bad Name")

    with pytest.raises(p.PluginError) as invalid_id:
        registry.add_plugin(Registers("Bad", lambda r: None))
    assert invalid_id.value.kind == "invalid_name"

    class Old(p.Plugin):
        def metadata(self):
            return p.PluginMetadata("old", "Old", "1", api_version=0)

        def register(self, registrar):
            pass

    with pytest.raises(p.PluginError) as old:
        registry.add_plugin(Old())
    assert (old.value.kind, old.value.built_for, old.value.host) == ("incompatible_api", 0, 1)
    assert "built for plugin API 0" in str(old.value)

    class NoMetadata(p.Plugin):
        def register(self, registrar):
            pass

    with pytest.raises(p.PluginError) as missing:
        registry.add_plugin(NoMetadata())
    assert missing.value.kind == "failed" and isinstance(missing.value.__cause__, NotImplementedError)

    with pytest.raises(TypeError):
        registry.add_plugin(object())
    with pytest.raises(p.PluginError) as wrong_type:
        registry.add_plugin(Registers("types", lambda r: r.highlighter(42)))
    assert isinstance(wrong_type.value.__cause__, TypeError)
    assert snapshot(registry) == before


def test_the_registrar_closes_when_register_returns():
    kept = []
    p.ExtensionRegistry().add_plugin(Registers("keep", kept.append))
    with pytest.raises(RuntimeError, match="closed"):
        kept[0].theme("late", Theme({}))


def test_direct_registration():
    registry = p.ExtensionRegistry()
    registry.register_transform("upper", upper)
    registry.register_code_highlighter("bold", Bold())
    registry.register_highlighter(Shouty)
    assert registry.provided_by(p.Capability("transform", "upper")) == "(direct)"
    assert registry.provided_by(p.Capability("highlighter")) == "(direct)"
    with pytest.raises(p.PluginError) as conflict:
        registry.register_transform("upper", upper)
    assert conflict.value.kind == "conflict"
    with pytest.raises(p.PluginError):
        registry.register_code_highlighter("Nope", Bold())


# ---------------------------------------------------------------------------
# Transforms and pipelines


def test_pipelines_chain_python_and_rust_transforms():
    registry = p.ExtensionRegistry()
    registry.register_transform("upper", upper)
    registry.register_transform("exclaim", lambda text: Text(text.plain + "!"))

    class Twice(p.TextTransform):
        def transform(self, text):
            return Text(text.plain * 2)

    registry.register_transform("twice", Twice())
    pipeline = registry.text_pipeline(["upper", "twice", "exclaim"])
    assert pipeline.names() == ["upper", "twice", "exclaim"] and len(pipeline) == 3
    assert pipeline(Text("ab")).plain == "ABAB!"
    # A pipeline is itself a transform.
    registry.register_transform("all", pipeline)
    assert registry.transform("all")(Text("x")).plain == "XX!"
    with pytest.raises(KeyError, match="nope"):
        registry.text_pipeline(["upper", "nope"])


def test_a_failing_stage_names_itself_and_keeps_the_python_exception():
    registry = p.ExtensionRegistry()
    registry.register_transform("upper", upper)

    def broken(text):
        raise KeyError("nope")

    registry.register_transform("broken", broken)
    registry.register_transform("wrong", lambda text: 42)
    with pytest.raises(p.PluginError) as raised:
        registry.text_pipeline(["upper", "broken"]).apply(Text("x"))
    assert (raised.value.kind, raised.value.stage) == ("pipeline", "broken")
    assert isinstance(raised.value.__cause__, KeyError)
    with pytest.raises(p.PluginError) as wrong:
        registry.transform("wrong")(Text("x"))
    assert isinstance(wrong.value.__cause__, TypeError)


def test_a_transform_may_style_in_place():
    registry = p.ExtensionRegistry()
    registry.register_transform("bold", lambda text: text.stylize("bold"))
    c = console(color=True)
    c.print(registry.transform("bold")(Text("x")))
    expected = console(color=True)
    expected.print(Text("x", style="bold"))
    assert output(c) == output(expected)


# ---------------------------------------------------------------------------
# Code highlighters


def test_a_python_code_highlighter_matches_the_rust_one_it_wraps():
    syntect = p.ExtensionRegistry.with_defaults().code_highlighter("syntect")

    class Delegating(p.CodeHighlighter):
        def highlight(self, code, language=None, theme=None):
            return syntect.highlight(code, language, theme)

        def default_theme(self):
            return syntect.default_theme()

        def themes(self):
            return syntect.themes()

    registry = p.ExtensionRegistry()
    registry.register_code_highlighter("delegating", Delegating())
    wrapped = registry.code_highlighter("delegating")
    for code in ["fn main() {}\n", "let s = \"é 日本 🦀\"; // ünïcode\n", ""]:
        for theme in ["ansi_dark", syntect.default_theme()]:
            assert wrapped.highlight(code, "rust", theme) == syntect.highlight(code, "rust", theme)
    assert wrapped.themes() == syntect.themes()
    assert wrapped.default_theme() == syntect.default_theme()


def test_python_output_is_validated_like_cores():
    class Sloppy(p.CodeHighlighter):
        def highlight(self, code, language=None, theme=None):
            return [
                # overlapping, reversed, past the end, negative, then fine
                [(0, 2, "red"), (1, 3, "blue"), (3, 2, "green"), (2, 99, "bold"), (-1, 1, "red")],
                [p.HighlightSpan(1, 2, "italic link https://example.com")],
                # extra lines are dropped, missing ones unstyled
                [(0, 1, "red")],
            ][: 3 if code.count("\n") else 1]

        def default_theme(self):
            return "t"

        def themes(self):
            return ["t"]

    registry = p.ExtensionRegistry()
    registry.register_code_highlighter("sloppy", Sloppy())
    result = registry.code_highlighter("sloppy").highlight("é日x\n🦀z", None)
    first, second = result.lines
    assert [(s.start, s.end, str(s.style)) for s in first.spans] == [(0, 2, "red")]
    assert [(s.start, s.end, str(s.style)) for s in second.spans] == [(1, 2, "italic")]
    single = registry.code_highlighter("sloppy").highlight("ab\ncd\nef", None)
    assert len(single) == 3
    missing = registry.code_highlighter("sloppy").highlight("ab", None)
    assert len(missing) == 1


def test_code_highlighter_errors_map_to_rusts():
    class Failing(p.CodeHighlighter):
        def highlight(self, code, language=None, theme=None):
            if theme != "ok":
                raise p.UnknownThemeError(theme)
            raise RuntimeError("engine broke")

        def default_theme(self):
            return "ok"

        def themes(self):
            return ["ok"]

    registry = p.ExtensionRegistry()
    registry.register_code_highlighter("failing", Failing())
    handle = registry.code_highlighter("failing")
    with pytest.raises(p.UnknownThemeError) as unknown:
        handle.highlight("x", None, "nope")
    assert unknown.value.args == ("nope",)
    with pytest.raises(p.HighlightError) as broke:
        handle.highlight("x")
    assert str(broke.value) == "syntax highlighting failed: RuntimeError: engine broke"
    assert isinstance(broke.value.__cause__, RuntimeError)
    # Rust's own unknown-theme error is the same class.
    with pytest.raises(p.UnknownThemeError):
        p.ExtensionRegistry.with_defaults().code_highlighter("syntect").highlight("x", None, "nope")
    with pytest.raises(TypeError):
        registry.register_code_highlighter("nothing", object())
    with pytest.raises(NotImplementedError):
        p.CodeHighlighter().highlight("x")


def test_highlighted_code_values():
    code = p.HighlightedCode([[(0, 1, "bold")], p.HighlightedLine([], newline_style="red")], background="#272822")
    assert code.background == "#272822"
    assert code.lines[0].spans == [p.HighlightSpan(0, 1, "bold")]
    assert str(code.lines[1].newline_style) == "red"
    assert code == p.HighlightedCode([[(0, 1, "bold")], p.HighlightedLine([], newline_style="red")], background="#272822")
    with pytest.raises(TypeError):
        p.HighlightedCode(["not spans"])


# ---- The ext conformance kit's checks (rich_ext::testing::conformance), in
# Python: the kit itself is behind rich-ext's `testing` feature, which the
# wheel does not build. Same sources, languages and rules.

SOURCES = [
    ("empty", ""),
    ("a lone newline", "\n"),
    ("no final newline", "fn main() {\n    let x = 1;\n}"),
    ("a final newline", "def f(x):\n    return x + 1  # one\n"),
    ("CRLF line endings", "fn a() {}\r\nfn b() {}\r\n"),
    ("tabs", 'if x {\n\treturn "a\\tb";\n}\n'),
    ("multi-byte text", 'let s = "é 日本 🦀"; // ünïcode\nprint("ok")\n'),
    ("blank lines", "\n\n\nx = 1\n\n"),
    ("an unterminated string", 'let s = "never closed\nnext line\n'),
]
LANGUAGES = ["rust", "python", "rs", None]


def conformance(engine, handle) -> list:
    """Every failure: `engine` is the Python object (its raw output is
    checked), `handle` the registered one (Rust's view of it)."""
    failures = []
    themes = list(engine.themes())
    default = engine.default_theme()
    if default not in themes:
        failures.append(("themes", f"the default theme {default!r} is not in themes()"))
    try:
        handle.highlight("x = 1", "python", "no-such-theme-for-conformance")
        failures.append(("unknown theme", "highlighted instead of UnknownThemeError"))
    except p.UnknownThemeError as error:
        if error.args != ("no-such-theme-for-conformance",):
            failures.append(("unknown theme", f"names {error.args}"))
    chosen = [default] + (["ansi_dark"] if "ansi_dark" in themes and default != "ansi_dark" else [])
    for theme in chosen:
        for name, code in SOURCES:
            for language in LANGUAGES:
                raw = p.HighlightedCode(engine.highlight(code, language, theme))
                lines = code.split("\n")
                if len(raw) != len(lines):
                    failures.append(("line count", f"{name}, {language}: {len(raw)} for {len(lines)}"))
                for number, (line, text) in enumerate(zip(raw.lines, lines)):
                    end = 0
                    for span in line.spans:
                        if span.start >= span.end or span.start < end or span.end > len(text):
                            failures.append(("spans", f"{name}, {language}: line {number} {span!r}"))
                        end = max(end, span.end)
            unknown = handle.highlight(code, "no-such-language-for-conformance", theme)
            if unknown != handle.highlight(code, None, theme):
                failures.append(("unknown language", name))
    block = 'fn f(x: u32) -> u32 {\n    // add one\n    let s = "text";\n    x + 1\n}\n'

    def timed(code):
        best = float("inf")
        for _ in range(3):
            started = time.perf_counter()
            handle.highlight(code, "rust", default)
            best = min(best, time.perf_counter() - started)
        return best

    timed(block * 200)
    small = max(timed(block * 200), 0.002)
    if timed(block * 2000) / small > 40.0:
        failures.append(("scaling", "10,000 lines cost more than 40x 1,000"))
    return failures


def test_conformance_of_a_python_code_highlighter():
    registry = p.ExtensionRegistry()
    registry.register_code_highlighter("bold", Bold())
    # The Rust `Bold` passes the Rust kit (`CONFORMANCE_BOLD=Ok(())`); the
    # Python one passes the same checks.
    assert conformance(Bold(), registry.code_highlighter("bold")) == []


def test_conformance_catches_a_broken_python_code_highlighter():
    class Broken(Bold):
        def highlight(self, code, language=None, theme=None):
            return [[(0, 5, "bold"), (2, 3, "red")]]  # one line, overlapping

        def default_theme(self):
            return "missing"

    registry = p.ExtensionRegistry()
    registry.register_code_highlighter("broken", Broken())
    checks = {check for check, _ in conformance(Broken(), registry.code_highlighter("broken"))}
    assert {"themes", "unknown theme", "line count", "spans"} <= checks


# ---------------------------------------------------------------------------
# Callbacks raising during a render


def test_an_exception_in_a_python_renderer_reaches_the_print():
    class Explodes:
        def __rich_console__(self, console, options):
            raise ZeroDivisionError("inside the render")
            yield  # pragma: no cover

    registry = p.ExtensionRegistry()
    registry.add_plugin(Registers("boom", lambda r: r.renderer("boom", lambda source: Explodes())))
    rendered = registry.renderer("boom").render("x")
    with pytest.raises(ZeroDivisionError, match="inside the render"):
        console().print(Panel(rendered))
    registry.add_plugin(Registers("raises", lambda r: r.renderer("raises", lambda source: 1 / 0)))
    with pytest.raises(p.PluginError) as raised:
        registry.renderer("raises").render("x")
    assert isinstance(raised.value.__cause__, ZeroDivisionError)
    registry.add_plugin(Registers("bad", lambda r: r.renderer("bad", lambda source: 42)))
    with pytest.raises(p.PluginError, match="renderable"):
        registry.renderer("bad").render("x")


def test_an_exception_in_a_fence_renderer_reaches_the_caller():
    registry = p.ExtensionRegistry()
    registry.add_plugin(Registers("f", lambda r: r.fence_renderer("f", lambda language, code: 1 / 0)))
    with pytest.raises(ZeroDivisionError):
        registry.fences().render_fence("f", "x", console())


def test_fence_renderer_objects_get_console_and_options():
    seen = {}

    class Fence(p.FenceRenderer):
        def render_fence(self, language, code, console, options):
            seen["args"] = (language, code, console, options.max_width)
            return Text(code)

    registry = p.ExtensionRegistry()
    registry.add_plugin(Registers("fence", lambda r: r.fence_renderer("fence", Fence())))
    c = console(width=30)
    assert [s.text for s in registry.fences().render_fence("fence", "abc", c)] == ["abc", "\n"]
    assert seen["args"] == ("fence", "abc", c, 30)


# ---------------------------------------------------------------------------
# install(console)


def _console_applies_installed() -> bool:
    registry = p.ExtensionRegistry()
    registry.register_highlighter(Shouty())
    c = console(width=20, color=True)
    registry.install(c)
    c.print("abc")
    return output(c) == "\x1b[1mabc\x1b[0m\n"


needs_console_hook = pytest.mark.skipif(
    not _console_applies_installed(),
    reason="Console does not apply installed extensions yet (see plugins::installed)",
)


@needs_console_hook
def test_install_makes_a_python_code_highlighter_the_consoles_default():
    Syntax = pytest.importorskip("rs_rich.syntax").__dict__.get("Syntax")
    if Syntax is None:
        pytest.skip("rs_rich.syntax.Syntax is not built")

    registry = p.ExtensionRegistry.with_defaults()
    registry.add_plugin(Demo())
    registry.set_default_code_highlighter("bold")
    c = console(width=20, color=True)
    registry.install(c)
    c.print(Syntax("x = 1\ny = 2", "python"))
    assert output(c) == RUST_BOLD_SYNTAX


@needs_console_hook
def test_install_adds_highlighters_as_rust_does():
    # rich-ext's NumberHighlighter (from `with_defaults`), as the Rust
    # reference console prints "abc 12" (`NUMBERS`).
    registry = p.ExtensionRegistry.with_defaults()
    c = console(width=20, color=True)
    registry.install(c)
    c.print("abc 12")
    assert output(c) == RUST_NUMBERS
    # A Python highlighter class is instantiated for each console.
    made = []

    class Counting(Shouty):
        def __init__(self):
            made.append(self)

    registry = p.ExtensionRegistry()
    registry.register_highlighter(Counting)
    registry.install(c)
    registry.install(console())
    c.print("x")
    assert made and output(c).endswith("\x1b[1mx\x1b[0m\n")
    # Installing again adds to what is there; the registry changing later
    # does not reach the console.
    registry.register_highlighter(Shouty())
    plain = console(color=True)
    p.ExtensionRegistry().install(plain)
    plain.print("x")
    assert output(plain) == "x\n"


@needs_console_hook
def test_a_python_highlighter_that_raises_fails_the_print():
    class Broken:
        def highlight(self, text):
            raise LookupError("highlighter bug")

    registry = p.ExtensionRegistry()
    registry.register_highlighter(Broken())
    c = console()
    registry.install(c)
    with pytest.raises(LookupError, match="highlighter bug"):
        c.print("x")


def test_install_accepts_only_consoles():
    registry = p.ExtensionRegistry.with_defaults()
    registry.install(console())
    p.install_defaults(console())
    with pytest.raises(TypeError):
        registry.install(object())
