"""rs_rich.ext.log_stream: log records, parsers, filters, folding and groups (#724)."""

from __future__ import annotations

import pytest
from conftest import render

from rs_rich import _native
from rs_rich.ext import log_stream
from rs_rich.ext.event import StructuredEvent
from rs_rich.ext.log_stream import (
    Collapser,
    LevelFilter,
    LogAssembler,
    LogFilter,
    LogGroupHeader,
    LogPredicate,
    LogRecord,
    LogSummary,
    format_log_offset,
    log_group_key,
    log_parser_names,
    parse_log_line,
    parse_log_timestamp,
)
from rs_rich.ext.redact import Redactor


def record(level, message, second=0, **fields):
    return LogRecord(
        {"time": f"2026-10-18T10:00:{second:02}Z", "level": level, "msg": message, **fields}
    )


def test_the_rust_names_are_aliases():
    assert log_stream.Record is LogRecord
    assert log_stream.Filter is LogFilter
    assert log_stream.parse_line is parse_log_line
    for name in log_stream.__all__:
        assert getattr(log_stream, name) is not None
    assert LogRecord.__module__ == "rs_rich.ext.log_stream"
    assert ("log.group", "bold") in _native.LOG_STYLES


def test_a_record_reads_its_message_level_and_time():
    rec = record("WARN", "slow", http={"status": 503}, tags=["edge"])
    assert rec.message == "slow"
    assert rec.level == "WARN"
    assert rec.severity == "warn"
    assert rec.time == "2026-10-18T10:00:00Z"
    assert rec.instant == 1_792_317_600_000_000
    assert rec.lookup("message") == "slow"
    assert rec.lookup("http.status") == 503
    assert rec.lookup("tags.0") == "edge"
    assert rec.get("nope") is None
    assert rec.format == "json"
    event = rec.to_event()
    assert isinstance(event, StructuredEvent)
    assert event.severity == "warn"
    assert list(event.fields) == ["http", "tags"]
    assert render(rec, width=80).startswith("2026-10-18T10:00:00Z warn slow")
    text = LogRecord.text("hello")
    assert (text.format, text.message) == ("text", "hello")
    assert record("info", "a", 0).same_as(record("info", "a", 5))
    assert record("info", "a") == record("info", "a")


def test_parsers_read_text_formats():
    assert log_parser_names()[:2] == ["json", "access"]
    logfmt = parse_log_line('level=info msg="hi there" n=1')
    assert (logfmt.format, logfmt.message, logfmt.get("n")) == ("logfmt", "hi there", "1")
    level = parse_log_line("ERROR request failed host=db")
    assert (level.severity, level.message, level.get("host")) == ("error", "request failed", "db")
    access = parse_log_line(
        '127.0.0.1 - - [10/Oct/2000:13:55:36 -0700] "GET /a HTTP/1.0" 404 12', parser="nginx"
    )
    assert (access.get("status"), access.severity) == (404, "warn")
    assert parse_log_line("just words") is None
    assert parse_log_line("level=info msg=hi", parser="text") is None
    with pytest.raises(ValueError, match="unknown parser"):
        parse_log_line("x", parser="xml")


def test_the_assembler_joins_continuation_lines():
    lines = LogAssembler()
    assert lines.push("ERROR failed") is None
    assert lines.push("  at main") is None
    done = lines.push("INFO next")
    assert isinstance(done, LogRecord)
    assert done.continuation == ["  at main"]
    last = lines.flush()
    assert last.message == "next"
    assert lines.push("late") == "late"


def test_filters_by_level_and_field():
    levels = LevelFilter("warn+")
    assert levels.allows("error") and not levels.allows("info") and not levels.allows()
    assert str(LevelFilter("info, error+")) == "info,error+"
    with pytest.raises(ValueError, match="unknown level"):
        LevelFilter("loud")
    predicate = LogPredicate("latency_ms>500")
    assert (predicate.path, predicate.op, predicate.value) == ("latency_ms", ">", "500")
    assert predicate.matches(record("info", "x", latency_ms=812))
    assert not predicate.matches(record("info", "x", latency_ms=12))
    with pytest.raises(ValueError, match="no field name"):
        LogPredicate("=x")
    keep = LogFilter(level="warn+", predicates=["service=api", "msg~time(d )?out"])
    assert keep.matches(record("error", "timed out", service="api"))
    assert not keep.matches(record("error", "timed out", service="web"))
    assert not keep.matches(record("info", "timed out", service="api"))


def test_the_collapser_folds_repeats():
    collapser = Collapser()
    assert collapser.push(record("warn", "again", 0)) == (None, True)
    assert collapser.push(record("warn", "again", 1)) == (None, False)
    assert collapser.current().count == 2
    ended, show = collapser.push(record("info", "other", 2))
    assert show and ended.count == 2 and ended.last_time == "2026-10-18T10:00:01Z"
    assert collapser.finish() is None
    assert collapser.folded == 1


def test_groups_and_the_summary():
    failing = record("error", "failed", 1, trace_id="t1")
    assert log_group_key(failing, "trace") == "t1"
    assert log_group_key(record("info", "x", request_id="r9"), "correlation") == "r9"
    with pytest.raises(ValueError, match="unknown grouping"):
        log_group_key(failing, "host")
    header = LogGroupHeader("trace", "t1", [record("info", "a", 0, trace_id="t1"), failing])
    assert render(header, width=80) == "▸ trace t1 · 2 records · 1 error · 1.000s\n"
    assert render(LogGroupHeader("span", None), width=80) == "▸ no span id\n"
    summary = LogSummary("trace")
    summary.read(3)
    summary.shown(record("info", "a", trace_id="t1"))
    summary.shown(failing)
    assert (summary.records_read, summary.records_shown) == (3, 2)
    assert summary.levels == {"error": 1, "info": 1}
    assert summary.failing == [("t1", 1)]
    assert render(summary, width=80) == (
        "Summary: 2 of 3 records shown\n  error 1 · info 1\n  1 trace with errors: t1 (1 error)\n"
    )


def test_times_and_redaction():
    assert parse_log_timestamp("1970-01-01T00:00:01Z") == 1_000_000
    assert parse_log_timestamp("10/Oct/2000:13:55:36 -0700") == 971_211_336_000_000
    assert parse_log_timestamp("soon") is None
    assert format_log_offset(62_500_000) == "+1m02.500s"
    secret = LogRecord({"password": "hunter 2", "msg": "user ann", "pin": 1234})
    masked = secret.redacted(Redactor.secrets())
    assert masked.get("password") == "********"
    assert masked.message == "user ann"
    assert secret.get("password") == "hunter 2"
    assert LogRecord.text("a\x1b[2Jb").sanitized().message != "a\x1b[2Jb"


def test_the_log_view_folds_filters_and_expands():
    from rs_rich.interact import LogView, Script, degrade

    failed = LogRecord(
        {"time": "2026-10-18T10:00:01Z", "level": "error", "msg": "refused", "trace_id": "t1"},
        continuation=["Traceback (most recent call last):"],
    )
    view = LogView([record("info", "started", 0, service="api"), failed, failed, "WARN disk low"])
    assert (len(view), view.collapse, view.time, view.fields) == (4, True, "absolute", False)
    assert view.records[1] == failed and view.records[3].message == "disk low"
    frame = view.headless("q", width=80, height=8).last_frame
    assert "ERROR ×2 refused trace_id=t1 (+1 line)" in frame
    assert "service=api" in frame and "disk low" in frame
    run = view.headless(Script().keys("/").text("trace_id=t1").keys("enter enter q"), height=10)
    assert run.outcome.done and run.value is None
    assert "trace_id: t1" in run.last_frame
    assert "│ Traceback (most recent call last):" in run.last_frame
    assert "started" not in run.last_frame
    flat = LogView([failed, failed], collapse=False, time="none", filter="refused")
    assert flat.filter == "refused"
    assert degrade(flat).prompts == "ERROR refused trace_id=t1 (+1 line)\n" * 2
    with pytest.raises(ValueError):
        LogView(time="soon")
    with pytest.raises(TypeError, match="LogRecord"):
        LogView([1])
