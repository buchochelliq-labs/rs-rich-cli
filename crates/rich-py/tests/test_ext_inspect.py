"""rs_rich.ext: hex, unicode, environment and source inspectors; records;
typed tables and streaming tables.

Expected output comes from `rich-ext` itself (see test_ext_expected.py).
"""

from __future__ import annotations

import pytest
from conftest import render
from test_ext_expected import EXPECTED

from rs_rich.ext import derive, env_inspect, hex, source_view, table, unicode_inspect
from rs_rich.text import Text

DATA = bytes(range(41)) + b"\x00\x00\x00" + b"Hello, world!\x7f\xff\xfe"


def check(name, renderable, width):
    assert render(renderable, width=width) == EXPECTED[name]
    assert render(renderable, width=width, color=True) == EXPECTED[f"{name}/color"]


def test_hex_view():
    check("hex/view", hex.HexView(DATA, highlight=b"Hello"), 80)
    check("hex/view", hex.HexView(DATA, highlight='"Hello"'), 80)  # the CLI's needle syntax
    check("hex/view", hex.HexView(DATA, highlight="48 65 6c 6c 6f"), 80)
    check("hex/narrow", hex.HexView(DATA, bytes_per_line=8, group=4, offset=0x100, ascii_panel=False), 40)
    check("hex/collapse", hex.HexView(bytes(64)), 80)
    assert hex.find_all(DATA, '"l"') == [i for i, b in enumerate(DATA) if b == ord("l")]
    assert hex.find_all(b"\x00\x01\x00\x01", "00 01") == hex.find_all(b"\x00\x01\x00\x01", b"\x00\x01") == [0, 2]
    with pytest.raises(ValueError):
        hex.HexView(DATA, highlight="Hello")
    assert [hex.byte_class(b) for b in (0, 65, 32, 7, 200)] == ["null", "printable", "whitespace", "control", "high"]


def test_unicode_view():
    text = "é 👍🏽 ​x\t‮"
    view = unicode_inspect.UnicodeView(text)
    check("unicode/text", view, 80)
    assert view.summary_text() == EXPECTED["unicode/summary"]
    clusters = view.clusters()
    assert clusters[0].text == "é" and clusters[0].code_points == "U+0065 U+0301"
    assert clusters[0].kind == "combining" and clusters[2].kind == "emoji"
    check("unicode/bytes", unicode_inspect.UnicodeView(b"ok\xff\xfe!"), 80)
    assert unicode_inspect.UnicodeView(b"ok\xff\xfe!").summary()["invalid"] == 2
    assert unicode_inspect.control_picture("\x1b") == "␛"


VARS = {"HOME": "/home/ada", "API_TOKEN": "sk-live-abc", "PATH": "/usr/bin:/bin:/usr/bin", "EDITOR": "vim"}


def test_env_views():
    check("env/view", env_inspect.EnvView(VARS, separator=":"), 70)
    check("env/filter", env_inspect.EnvView(VARS, separator=":", filter="*TOKEN*", redact=False), 70)
    view = env_inspect.EnvView(VARS)
    assert view.is_redacted("API_TOKEN") and not view.is_redacted("HOME")
    assert [name for name, _ in view.vars()] == sorted(VARS)
    assert env_inspect.is_secret_name("DB_PASSWORD") and env_inspect.name_matches("*path*", "PATH")

    kinds = {"/usr/bin": "directory", "/etc/passwd": "file"}
    path = env_inspect.PathView(
        "PATH",
        "/usr/bin:/opt/none::/etc/passwd:/usr/bin/",
        separator=":",
        case_insensitive=False,
        probe=lambda p: kinds.get(p, "missing"),
    )
    check("env/path", path, 70)
    assert [status for _, _, status in path.problems()] == ["missing", "empty", "not a directory", "duplicate of #1"]


def test_source_view():
    code = 'fn main() {\n\tlet greeting = "Hello";\n    println!("{greeting}, world");\n}\n'
    view = source_view.SourceView(code, "rust", search="hello", start_line=10)
    check("source/view", view, 60)
    assert view.matches() == [(11, 1)]
    check("source/plain", source_view.SourceView(code, "text", line_numbers=False, tab_size=2), 60)


FIELDS = [("name", "'web'"), ("port", "8080"), ("tags", "['a', 'b']")]


def test_records():
    for presentation in ("fields", "panel", "table"):
        check(f"derive/{presentation}", derive.Record(FIELDS, title="Server", presentation=presentation), 40)
    record = derive.Record(FIELDS, title="Server")
    check("derive/table-many", derive.records_table([record, record]), 40)

    import dataclasses

    @dataclasses.dataclass
    class Server:
        name: str
        port: int
        tags: list

    from_object = derive.Record.from_object(Server("web", 8080, ["a", "b"]))
    check("derive/fields", from_object, 40)
    check("derive/table-many", derive.records_table([Server("web", 8080, ["a", "b"])] * 2), 40)


def latency(value):
    return Text("-", style="dim") if value is None else f"{value:.0f} ms"


def services():
    return table.TableData(
        [
            "service",
            "region",
            table.Column("errors", justify="right"),
            table.Column("p99", justify="right", format=latency),
        ],
        [
            ["api", "eu", 3, 120.0],
            ["web", "us", 0, 80.0],
            ["db", "eu", 7, None],
            ["cache", "us", 1, 4.0],
            ["worker-10", "eu", 3, 95.0],
            ["worker-9", None, 0, 60.0],
        ],
    )


def test_table_data():
    check("table/sort", services().sort_by([table.SortKey(1), table.SortKey(2, descending=True)]), 60)
    check("table/natural", services().sort_by(0), 60)
    grouped = table.TableData(
        services().headers[:2]
        + [table.Column("errors", justify="right"), table.Column("p99", justify="right", format=latency)],
        services().rows,
        sort=[1, 0],
        group_by=table.GroupBy(1, aggregates=[table.Aggregate("sum", 2), table.Aggregate("max", 3)]),
        totals=[table.Aggregate("count", 0), table.Aggregate("sum", 2), table.Aggregate("mean", 3)],
        totals_label="all",
    )
    check("table/group", grouped, 60)
    from rs_rich import box

    framed = table.TableData(
        grouped.headers[:2]
        + [table.Column("errors", justify="right"), table.Column("p99", justify="right", format=latency)],
        services().rows,
        title="Services",
        caption="6 rows",
        box=box.ROUNDED,
        expand=True,
    )
    check("table/framed", framed, 60)
    # api, cache, db, web, worker-9, worker-10: natural order.
    assert services().sort_by(0).order() == [0, 3, 2, 1, 5, 4]
    sorted_by_transform = table.Sort([0]).apply(services())
    assert sorted_by_transform.order() == [0, 3, 2, 1, 5, 4]
    assert table.Aggregate("mean", 0).compute([1, 2, None, 3]) == 2.0
    assert table.Aggregate.custom(0, lambda values: len(values)).compute(["a", "b"]) == 2


def test_streaming_table():
    jobs = table.StreamingTable(
        [
            "job",
            "state",
            table.Column("done", justify="right", format=lambda v: f"{v}%" if isinstance(v, int) else ""),
        ],
        title="pipeline",
        window=("tail", 4),
    )
    for job in ["fetch", "build", "test", "lint", "docs"]:
        assert jobs.upsert(job, [job, "queued", 0])
    jobs.update_cell("build", 1, "running")
    jobs.update_cell("build", 2, 45)
    jobs.upsert("fetch", ["fetch", "done", 100])
    check("table/stream", jobs, 60)
    assert jobs["build"] == ["build", "running", 45] and "docs" in jobs and len(jobs) == 5
    assert not jobs.upsert("docs", ["docs", "queued", 0])  # unchanged
    assert jobs.stats()["frames"] >= 2

    capped = table.StreamingTable(["k", "v"], capacity=2)
    for key, value in [("a", 1), ("b", 2), ("c", 3)]:
        capped.upsert(key, [key, value])
    assert render(capped, width=30) == EXPECTED["table/capped"]
    assert str(capped.evicted) == EXPECTED["table/capped-evicted"]
    assert "a" not in capped and [k for k, _ in capped.rows()] == ["b", "c"]
    assert capped.remove("b") == ["b", 2] and capped.remove("zz") is None
