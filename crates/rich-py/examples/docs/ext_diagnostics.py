"""The pictures on docs/python/ext/diagnostics.md: run it, or scripts/capture_python_docs.py."""

from _shots import Shots

from rs_rich.ext.dashboard import DiagnosticsDashboard
from rs_rich.ext.diagnostic import Diagnostic, Location, SourceSnippet, Suggestion
from rs_rich.ext.event import StructuredEvent
from rs_rich.ext.log_handler import EventHandler
from rs_rich.ext.stacktrace import parse


def expanded(console):
    # --8<-- [start:expanded]
    CONFIG = '[server]\nport = invalid\nhost = "localhost"\n'
    start = CONFIG.find("invalid")
    snippet = SourceSnippet("config.toml", CONFIG, start, start + 7, label="not a number")
    snippet.secondary(CONFIG.find("port"), CONFIG.find("port") + 4, "for this key")

    console.print(Diagnostic.error(
        "invalid endpoint",
        code="CFG001",
        location=snippet.location,
        causes=["port must be numeric"],
        snippets=[snippet],
        help=["use a port from 1 to 65535"],
        suggestions=[Suggestion.replace("for example", CONFIG, start, start + 7, "8080")],
        view="expanded",
    ))
    # --8<-- [end:expanded]


def steps(console):
    # --8<-- [start:steps]
    diagnostic = Diagnostic.warning("`timeout` is deprecated", code="W12")
    diagnostic.add_note("it is ignored since 2.0").add_help("use `deadline` instead")
    diagnostic.view = "expanded"
    console.print(diagnostic)
    # --8<-- [end:steps]


def exception(console):
    # --8<-- [start:exception]
    try:
        try:
            {}["port"]
        except KeyError as error:
            raise ValueError("bad config") from error
    except ValueError as error:
        console.print(Diagnostic.from_exception(error))
    # --8<-- [end:exception]


# --8<-- [start:parse]
trace = parse('''Traceback (most recent call last):
  File "/srv/app/app.py", line 6, in main
    load()
  File "/srv/app/app.py", line 3, in load
    return {}["port"]
KeyError: 'port'
''')
# --8<-- [end:parse]


def stacktrace(console):
    # --8<-- [start:stacktrace]
    print(trace.language, trace.kind, [frame.function for frame in trace.frames])
    console.print(trace)
    # --8<-- [end:stacktrace]


def dashboard(console):
    # --8<-- [start:dashboard]
    dashboard = DiagnosticsDashboard(top_codes=3)
    dashboard.push(Diagnostic.error("mismatched types", code="E0308", location=Location("src/main.rs", 12, 5)))
    dashboard.push(Diagnostic.warning("unused variable `x`", code="W1", location=Location("src/main.rs", 3, 9)))
    dashboard.push(Diagnostic.warning("unused import", code="W1", location=Location("src/lib.rs", 1, 5)))
    console.print(dashboard)
    print(dashboard.counts())
    # --8<-- [end:dashboard]


def events(console):
    # --8<-- [start:events]
    event = StructuredEvent("request done", fields={"status": 200, "path": "/api"}, severity="info")
    console.print(event)
    handler = EventHandler(time="[12:00:00]", show_path=False)
    handler.emit(console, StructuredEvent("GET /index", severity="warning", fields={"ms": 12}))
    # --8<-- [end:events]


shots = Shots("ext/diagnostics")
shots.shot("expanded", expanded, width=70)
shots.shot("steps", steps, width=80)
shots.shot("exception", exception, width=80)
shots.shot("stacktrace", stacktrace, width=80)
shots.shot("dashboard", dashboard, width=80)
shots.shot("events", events, width=80)
shots.finish()
