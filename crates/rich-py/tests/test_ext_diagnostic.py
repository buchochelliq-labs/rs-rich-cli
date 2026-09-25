"""rs_rich.ext: diagnostics, stack traces, hyperlinks and structured events.

Expected output comes from `rich-ext` itself (see test_ext_expected.py).
"""

from __future__ import annotations

import pytest
from conftest import render
from test_ext_expected import EXPECTED

from rs_rich.ext.dashboard import DiagnosticsDashboard
from rs_rich.ext.diagnostic import (
    Diagnostic,
    DiagnosticSpanError,
    Location,
    SourceSnippet,
    Suggestion,
)
from rs_rich.ext.event import StructuredEvent
from rs_rich.ext.highlighter import NumberHighlighter
from rs_rich.ext.hyperlink import Hyperlinker
from rs_rich.ext.log_handler import EventHandler
from rs_rich.ext.stacktrace import StackTrace, TraceFrame, parse
from rs_rich.text import Text

PYTHON_TRACE = """Traceback (most recent call last):
  File "/srv/app/app.py", line 6, in main
    load()
  File "/srv/app/app.py", line 3, in load
    return {}["port"]
           ~~^^^^^^^^
KeyError: 'port'

The above exception was the direct cause of the following exception:

Traceback (most recent call last):
  File "/srv/app/app.py", line 11, in outer
    main()
  File "/srv/app/app.py", line 8, in main
    raise ValueError("bad config") from e
ValueError: bad config
"""

CONFIG = '[server]\nport = invalid\nhost = "localhost"\n'


def check(name, renderable, width):
    assert render(renderable, width=width) == EXPECTED[name]
    assert render(renderable, width=width, color=True) == EXPECTED[f"{name}/color"]


def test_quickstart():
    diagnostic = Diagnostic.error(
        "mismatched types",
        code="E0308",
        location=Location("src/main.rs", 12, 5),
        causes=["expected `u16`, found `&str`"],
    )
    check("diagnostic/quickstart", diagnostic, 70)


def test_snippet_labels_and_suggestion():
    value, key = CONFIG.find("invalid"), CONFIG.find("port")
    snippet = SourceSnippet("config.toml", CONFIG, value, value + 7, context_lines=1, label="not a number")
    snippet.secondary(key, key + 4, "for this key")
    diagnostic = Diagnostic.error(
        "invalid endpoint",
        code="CFG001",
        code_url="https://acme.dev/errors/CFG001",
        location=snippet.location,
        causes=["port must be numeric"],
        snippets=[snippet],
        notes=["ports below 1024 need privileges"],
        help=["use a port from 1 to 65535"],
        suggestions=[Suggestion.replace("for example", CONFIG, value, value + 7, "8080")],
        hyperlinker=Hyperlinker(),
        view="expanded",
    )
    check("diagnostic/snippet", diagnostic, 70)


def test_builder_methods_match_keywords():
    value, key = CONFIG.find("invalid"), CONFIG.find("port")
    snippet = SourceSnippet("config.toml", CONFIG, value, value + 7, label="not a number").secondary(
        key, key + 4, "for this key"
    )
    diagnostic = Diagnostic.error("invalid endpoint", code="CFG001", code_url="https://acme.dev/errors/CFG001")
    diagnostic.location = snippet.location
    diagnostic.add_cause("port must be numeric").add_snippet(snippet)
    diagnostic.add_note("ports below 1024 need privileges").add_help("use a port from 1 to 65535")
    diagnostic.add_suggestion(Suggestion.replace("for example", CONFIG, value, value + 7, "8080"))
    diagnostic.hyperlinker = Hyperlinker()
    diagnostic.view = "expanded"
    check("diagnostic/snippet", diagnostic, 70)
    assert diagnostic.level == "error"
    assert diagnostic.code == "CFG001"
    assert diagnostic.location == Location("config.toml", 2, 8)
    assert diagnostic.causes == ["port must be numeric"]
    assert diagnostic.notes == ["ports below 1024 need privileges"]
    assert [s.message for s in diagnostic.suggestions] == ["for example"]


@pytest.mark.parametrize(
    "name, diagnostic",
    [
        ("warning", lambda: Diagnostic.warning("`timeout` is deprecated")),
        ("info", lambda: Diagnostic("using 4 worker threads", level="info")),
        ("note", lambda: Diagnostic("defaults came from /etc/acme", level="note")),
        ("help", lambda: Diagnostic("run `acme check` to validate", level="help")),
        ("nolevel", lambda: Diagnostic("config rejected", code="CFG")),
    ],
)
def test_levels(name, diagnostic):
    check(f"diagnostic/level/{name}", diagnostic(), 70)


def test_offsets_are_characters():
    # "é" is two bytes: a byte-offset API would split it.
    source = "naïve = 1\n"
    snippet = SourceSnippet("x.toml", source, 0, 5, label="here")
    assert snippet.location == Location("x.toml", 1, 1)
    out = render(Diagnostic("bad", snippets=[snippet], view="expanded"), width=40)
    assert "^^^^^ here" in out


def test_invalid_span_raises():
    with pytest.raises(ValueError):
        Diagnostic("x", level="fatal")
    with pytest.raises(DiagnosticSpanError):
        SourceSnippet("a", "abc", 2, 1)


def test_from_exception_chain():
    try:
        try:
            raise KeyError("port")
        except KeyError as error:
            raise ValueError("bad config") from error
    except ValueError as error:
        diagnostic = Diagnostic.from_exception(error)
    assert diagnostic.message == "bad config"
    assert diagnostic.causes == ["'port'"]
    assert render(diagnostic, width=60) == "error: bad config\ncaused by: 'port'\n"

    one = Diagnostic.from_exception(RuntimeError("a"), max_depth=0, level=None)
    assert one.level is None and one.causes == []


def test_stacktrace_parse_and_render():
    trace = parse(PYTHON_TRACE)
    assert trace.language == "python"
    assert (trace.kind, trace.message) == ("ValueError", "bad config")
    assert trace.cause.kind == "KeyError"
    assert trace.cause_kind == "caused_by"
    assert len(trace.chain()) == 2
    assert trace.origin.function == "main"
    assert [f.line for f in trace.frames] == [11, 8]
    check("stacktrace/python", trace, 70)
    assert StackTrace.parse("not a trace") is None


def test_stacktrace_custom_parser_and_building():
    class Tiny:
        def detect(self, text):
            return text.startswith("tiny error: ")

        def parse(self, text):
            first, *rest = text.splitlines()
            frames = []
            for line in reversed(rest):
                function, place = line.strip()[3:].split(" (")
                path, number = place.rstrip(")").rsplit(":", 1)
                frames.append(TraceFrame(function=function, path=path, line=int(number)))
            return StackTrace("tiny", kind="tiny error", message=first[12:], frames=frames)

    text = "tiny error: out of cheese\n  at brew (src/pot.tiny:9)\n  at main (src/main.tiny:2)"
    trace = StackTrace.parse(text, parsers=[Tiny()])
    assert trace.language == "tiny"
    assert [f.function for f in trace.frames] == ["main", "brew"]
    out = render(Diagnostic.error("worker crashed", trace=trace, view="expanded"), width=60)
    assert "out of cheese" in out and "src/pot.tiny:9" in out


def test_stacktrace_from_python_exception():
    def fail():
        raise LookupError("missing")

    try:
        fail()
    except LookupError as error:
        trace = StackTrace.from_exception(error)
    assert trace.kind == "LookupError"
    assert trace.frames[-1].function == "fail"


def test_dashboard():
    at = lambda path, line, column: Location(path, line, column)  # noqa: E731
    dashboard = DiagnosticsDashboard(top_codes=3)
    dashboard.push(Diagnostic.error("mismatched types", code="E0308", location=at("src/main.rs", 12, 5)))
    dashboard.push(Diagnostic.warning("unused variable `x`", code="W1", location=at("src/main.rs", 3, 9)))
    dashboard.push(Diagnostic.warning("unused import", code="W1", location=at("src/lib.rs", 1, 5)))
    dashboard.push(Diagnostic("see the migration guide", level="note"))
    check("dashboard", dashboard, 70)
    assert list(dashboard.counts().items()) == [("error", 1), ("warning", 2), ("note", 1)]
    dashboard.min_level = "warning"
    assert dashboard.counts() == {"error": 1, "warning": 2}


def test_hyperlinker():
    linker = Hyperlinker(repository="https://github.com/o/r")
    text = "see café src/a.rs:3:4 and #12 at https://x.dev/p."
    links = linker.find(text)
    spans = [(text.index(part), text.index(part) + len(part)) for part in ("src/a.rs:3:4", "#12", "https://x.dev/p")]
    assert [(link.start, link.end) for link in links] == spans
    assert links[0].url.startswith("file://") and links[0].url.endswith("src/a.rs#3")
    assert links[1].url == "https://github.com/o/r/issues/12"
    assert linker.reference_url(7, repo="a/b") == "https://github.com/a/b/issues/7"
    editor = Hyperlinker(editor="vscode://file/{path}:{line}:{column}")
    assert editor.file_url("/x/y.py", 3, 2) == "vscode://file//x/y.py:3:2"
    assert Hyperlinker.disabled().file_url("/x") is None
    text = Hyperlinker()("go to https://example.com now")
    assert isinstance(text, Text)
    check("hyperlink/text", text, 40)
    check("hyperlink/location", Hyperlinker().location("/src/lib.rs", 3, 1, "magenta"), 40)


def test_number_highlighter():
    check("highlighter/number", NumberHighlighter()("count=7 of 12.5"), 40)


def test_structured_event():
    event = StructuredEvent(
        "request done",
        fields={"status": 200, "path": "/api", "ok": True, "tags": ["a", "b"]},
        severity="info",
    )
    check("event/compact", event, 60)
    event.field("status", 500)
    assert event.fields["status"] == 500
    expanded = StructuredEvent("cfg", fields={"nested": {"a": [1, 2]}}, view="expanded")
    check("event/expanded", expanded, 60)


def test_event_handler():
    handler = EventHandler(time="[12:00:00]")
    event = StructuredEvent(
        "GET /index 200",
        severity="warning",
        path="/srv/app/web.py",
        line=42,
        fields={"ms": 12},
        spans=[("request", {"id": 7}), "render"],
    )
    check("log_handler/event", handler.render(event), 80)
    assert EventHandler.level_text("error").plain == "ERROR   "


def test_event_handler_blanks_repeated_times_and_draws_spans():
    handler = EventHandler(time="[09:30:00]")
    first = handler.render(StructuredEvent("first"))
    second = handler.render(StructuredEvent("second", severity="error"))
    assert render(first, width=40) == EXPECTED["log_handler/repeat-1"]
    assert render(second, width=40) == EXPECTED["log_handler/repeat-2"]
    tree = EventHandler(span_view="tree", show_time=False, show_path=False)
    opened = StructuredEvent("request", fields={"id": 7}, span_event="open")
    inner = StructuredEvent("[b]step[/b] done", markup=True, spans=["request"])
    closed = StructuredEvent("request", span_event="close", elapsed=1.2)
    assert render(tree.render(opened), width=50) == EXPECTED["log_handler/tree-open"]
    assert render(tree.render(inner), width=50) == EXPECTED["log_handler/tree-inner"]
    assert render(tree.render(closed), width=50) == EXPECTED["log_handler/tree-close"]


def test_event_handler_emits_to_a_console():
    import io

    from rs_rich.console import Console

    out = io.StringIO()
    EventHandler(time="[09:30:00]").emit(Console(file=out, width=40), StructuredEvent("first"))
    assert out.getvalue() == EXPECTED["log_handler/repeat-1"]
