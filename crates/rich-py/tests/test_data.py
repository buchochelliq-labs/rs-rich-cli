"""``rs_rich.data``: ``rs-rich-data`` and the 0.0.16 views from Python
(0.0.16 workstream 7).

Rich has no counterpart, so the expected values are what the Rust crates
give for the same input: most cases are ``rich_data``'s and ``rich_ext``'s
own doc examples, which their doc tests assert.
"""

from __future__ import annotations

import io
import json
import os
import sys

import pytest

from rs_rich import _native, data
from rs_rich.console import Console
from rs_rich.ext import data as ext_data
from rs_rich.ext import diff as ext_diff
from rs_rich.ext.table import TableData

SERVICES = "service,p99,up\nweb,120,true\napi,35.5,false\ndb,,true\n"

DDL = """
CREATE TABLE users (id INT PRIMARY KEY, email TEXT UNIQUE);
CREATE TABLE posts (id INT PRIMARY KEY,
                    author INT NOT NULL REFERENCES users (id));
"""

OLD = {"title": "User", "type": "object", "properties": {"id": {"type": "integer"}}}
NEW = {
    "title": "User",
    "type": "object",
    "required": ["id"],
    "properties": {"id": {"type": "string"}, "name": {"type": "string"}},
}

CONFLICT = "a\n<<<<<<< HEAD\nb\n=======\nc\n>>>>>>> topic\nd\n"


def drawn(renderable, width: int = 60) -> str:
    out = io.StringIO()
    Console(file=out, width=width, color_system=None).print(renderable)
    return out.getvalue()


def services() -> data.Rows:
    return data.read_csv(SERVICES)


def test_the_module_exports_native_objects_and_the_rust_names():
    for name in data.__all__:
        assert getattr(data, name) is getattr(_native, name)
    assert data.DataError is data.DataSourceError
    assert data.not_null is data.check_not_null
    assert data.unique is data.check_unique
    assert ext_diff.ConflictView is data.ConflictView
    assert ext_data.RecordView is data.RecordView


# ---------------------------------------------------------------------------
# Adapters


def test_csv_reads_text_cells_under_the_header():
    rows = services()
    assert rows.columns == ["service", "p99", "up"]
    assert len(rows) == 3
    assert rows.rows[2] == ["db", "", "true"]
    assert rows.column("service") == ["web", "api", "db"]
    with pytest.raises(KeyError):
        rows.column("nope")


def test_the_header_modes():
    assert data.read_csv("a,b\n1,2\n", header=False).columns == ["1", "2"]
    assert data.read_csv("1,2\n3,4\n").columns == ["1", "2"]
    assert data.read_csv("x;y\n1;2\n", delimiter=";").columns == ["x", "y"]
    assert data.read_tsv("x\ty\n1\t2\n").rows == [["1", "2"]]
    with pytest.raises(ValueError):
        data.read_csv("a\n", header="maybe")
    with pytest.raises(ValueError):
        data.read_csv("a\n", delimiter=";;")


def test_jsonl_reads_keys_in_the_order_first_seen():
    rows = data.read_jsonl('{"a": 1, "b": "x"}\n{"b": "y", "c": true}\n')
    assert rows.columns == ["a", "b", "c"]
    assert rows.rows[0][:2] == [1, "x"]
    with pytest.raises(data.DataSourceError) as error:
        data.read_jsonl('{"a": 1}\nnope\n')
    assert error.value.line == 2


def test_files_by_extension_and_by_content(tmp_path):
    csv = tmp_path / "s.csv"
    csv.write_text(SERVICES, encoding="utf-8")
    assert data.read_file(csv).columns == ["service", "p99", "up"]
    lines = tmp_path / "events"
    lines.write_text('{"k": 1}\n{"k": 2}\n', encoding="utf-8")
    assert data.read_file(lines).column("k") == [1, 2]
    with pytest.raises(OSError):
        data.read_file(tmp_path / "missing.csv")
    with pytest.raises(ValueError):
        data.read_file(csv, format="xlsx")


def non_utf8_file(directory, text: str):
    """A file whose name holds a byte that is not UTF-8, or skip where the
    platform or filesystem has no such names."""
    if sys.platform == "win32":
        pytest.skip("Windows paths are UTF-16")
    path = directory / os.fsdecode(b"bad\xff.csv")
    try:
        path.write_text(text, encoding="utf-8")
    except (OSError, UnicodeError):
        pytest.skip("the filesystem refuses non-UTF-8 names")
    return path


def test_files_with_non_utf8_names(tmp_path):
    path = non_utf8_file(tmp_path, SERVICES)
    assert data.read_file(path).columns == ["service", "p99", "up"]
    profile = data.Profile.from_path(path)
    assert profile.rows == 3
    assert profile.name.endswith("bad\ufffd.csv")


def test_rows_from_python_pad_and_cut():
    rows = data.Rows(["a", "b"], [[1], [1, 2, 3]])
    assert rows.rows == [[1, None], [1, 2]]
    assert rows.push(["x", 2.5]) is rows
    assert len(rows) == 3
    table = rows.to_table_data()
    assert isinstance(table, TableData)
    assert table.headers == ["a", "b"]


# ---------------------------------------------------------------------------
# Inference and statistics


def test_inference_carries_its_evidence_and_converts_on_request():
    rows = services()
    inference = data.infer(rows)
    types = {c.name: c.type for c in inference.columns}
    assert types == {"service": "text", "p99": "float", "up": "boolean"}
    p99 = inference.columns[1]
    assert p99.evidence["floats"] == 2 and p99.evidence["nulls"] == 1
    assert p99.summary == "float: 2 of 2 values, 1 null"
    # Nothing changes until it is applied.
    assert rows.rows[0][1] == "120"
    inference.apply(rows)
    assert rows.rows[0][1] == 120.0 and rows.rows[2][1] is None
    assert [f.type for f in rows.schema.fields] == ["string", "float", "boolean"]
    assert "│ p99     │ float   │" in drawn(inference)


def test_inference_overrides_and_null_tokens():
    rows = data.read_csv("a,b\n1,-\n2,3\n")
    inference = rows.infer(null_tokens=["-"], overrides={"a": "text"})
    assert [(c.type, c.overridden) for c in inference.columns] == [("text", True), ("integer", False)]
    with pytest.raises(ValueError):
        rows.infer(overrides={"a": "decimal"})


def test_statistics_per_column():
    rows = services()
    data.infer(rows).apply(rows)
    stats = data.Stats(rows, quantiles=[0.5], top=1)
    p99 = stats.columns[1]
    assert (p99.name, p99.count, p99.nulls, p99.numeric) == ("p99", 2, 1, True)
    assert p99.mean == pytest.approx(77.75)
    assert p99.quantiles == [(0.5, pytest.approx(77.75))]
    assert stats.columns[2].top == [("true", 2)]
    assert len(stats.headers()) == 3
    with pytest.raises(ValueError):
        data.Stats(rows, quantiles=[2.0])


# ---------------------------------------------------------------------------
# Profiles and quality


def test_a_profile_of_text_matches_the_crate_example():
    profile = data.Profile.from_text("service,p99\nweb,120\napi,35\ndb,\nweb,80\n")
    assert profile.rows == 4 and not profile.sampled
    p99 = profile.column("p99")
    assert (p99["type"], p99["nulls"]) == ("integer", 1)
    assert p99["mean"] == pytest.approx(78.33333333333333)
    assert profile.column("service")["distinct"] == 3
    assert profile.column("nope") is None
    out = drawn(profile)
    assert out.startswith("4 rows, 2 columns\n")
    assert "p99 · integer · 1 null (25%)" in out
    as_json = json.loads(profile.to_json())
    assert as_json["columns"][1]["distribution"]["kind"] == "histogram"
    assert profile.to_dict() == as_json
    assert json.loads(profile.to_json(indent=2)) == as_json


def test_profile_options_and_files(tmp_path):
    path = tmp_path / "events.jsonl"
    path.write_text("".join(json.dumps({"n": i, "tag": "ab"[i % 2]}) + "\n" for i in range(50)))
    profile = data.Profile.from_path(path, sample=10, columns=["n"])
    assert profile.name == str(path)
    assert (profile.rows, profile.sample_size, profile.sampled) == (50, 10, True)
    assert [c["name"] for c in profile.columns] == ["n"]
    rows = services()
    named = data.Profile(rows, name="svc.csv", top=1)
    assert named.heading.startswith("svc.csv: 3 rows")
    with pytest.raises(data.DataSourceError):
        data.Profile(rows, columns=["nope"])
    with pytest.raises(ValueError):
        data.Profile(rows, sample=0)


def test_quality_checks_and_the_report():
    rows = services()
    report = data.QualityReport(
        [
            data.check_not_null(rows, "p99"),
            data.check_unique(rows, "service"),
            data.CheckResult("row_count", "warn", observed="3", expected=">= 5"),
            data.CheckResult(
                "schema", "error", message="no such table", failing_rows=(["id"], [["7"]], 12)
            ),
        ]
    )
    statuses = [(r.check, r.status) for r in report.results]
    assert statuses == [("not_null", "fail"), ("unique", "pass"), ("row_count", "warn"), ("schema", "error")]
    assert report.summary == "1 passed, 1 warned, 1 failed, 1 errored"
    assert not report.ok
    assert report.totals == {"passed": 1, "warned": 1, "failed": 1, "errored": 1}
    assert report.results[3].failing_rows == (["id"], [["7"]], 12)
    out = drawn(report, width=70)
    assert "FAIL not_null › p99" in out and "PASS" in out
    again = data.QualityReport.from_json(report.to_json())
    assert again.summary == report.summary
    listed = data.QualityReport.from_json(json.dumps(json.loads(report.to_json())["results"]))
    assert len(listed) == 4
    with pytest.raises(KeyError):
        data.check_unique(rows, "nope")
    with pytest.raises(ValueError):
        data.CheckResult("x", "maybe")


# ---------------------------------------------------------------------------
# Result sets and virtualised tables


def test_a_result_set_marks_nulls_and_counts_rows():
    rows = services()
    data.infer(rows).apply(rows)
    result = data.ResultSet(rows, elapsed=0.012)
    out = drawn(result)
    assert "│ db      │ NULL │ true  │" in out
    assert out.endswith("(3 rows, 12ms)\n")
    assert result.footer() == "(3 rows, 12ms)"
    assert not result.windowed
    assert data.ResultSet(rows, limit=1, null_marker="∅").windowed


def test_a_virtual_table_shows_one_window():
    rows = data.Rows(["n"], [[i] for i in range(100)])
    table = data.VirtualTable(rows, height=5, offset=10)
    out = drawn(table)
    assert "│ 10 │" in out and "│ 15 │" not in out
    assert "rows 11–15 of 100" in out
    assert table.window() == (10, 15)
    table.scroll_by(200)
    assert table.offset == table.max_offset == 95
    assert table.position(ascii=True) == "rows 96-100 of 100"
    table.offset = 0
    table.height = 3
    assert table.window() == (0, 3)
    # Widths come from the first rows unless fitted to the window.
    wide = data.Rows(["n"], [[i * 1000] for i in range(200)])
    assert "│ 150000 │" not in drawn(data.VirtualTable(wide, offset=150, height=1))
    assert "│ 150000 │" in drawn(data.VirtualTable(wide, offset=150, height=1, fit_window=True))


# ---------------------------------------------------------------------------
# Schemas


def test_the_schema_model_from_sql():
    schema = data.Schema.from_sql(DDL)
    assert [t.name for t in schema.tables] == ["users", "posts"]
    posts = schema.table("posts")
    author = posts.fields[1]
    assert (author.name, author.type, author.required, author.references) == ("author", "INT", True, "users.id")
    assert posts.primary_key == ["id"]
    assert schema.table("users").fields[1].unique
    with pytest.raises(data.SchemaError) as error:
        data.Schema.from_sql("CREATE TABLE (")
    assert error.value.line == 1


def test_the_schema_tree_matches_the_crate_example():
    schema = {
        "title": "User",
        "type": "object",
        "required": ["email"],
        "properties": {
            "email": {"type": "string", "format": "email"},
            "age": {"type": "integer", "minimum": 0},
        },
    }
    expected = "User  object\n├── email (required)  string  format=email\n└── age  integer  minimum=0\n"
    assert drawn(data.SchemaTree(schema)) == expected
    assert drawn(data.SchemaTree(json.dumps(schema))) == expected
    model = data.Schema.from_json_schema(schema)
    assert drawn(model) == expected
    assert [f.constraints for f in model.fields] == [["format=email"], ["minimum=0"]]
    assert drawn(data.SchemaTree(schema, title="Account")).startswith("Account  object\n")
    with pytest.raises(data.SchemaError):
        data.SchemaTree("[1, 2]")
    with pytest.raises(TypeError):
        data.SchemaTree(42)


def test_schema_diffs_of_json_schemas_and_of_ddl():
    diff = data.SchemaDiff(OLD, NEW)
    assert [(c.kind, c.path, c.detail, c.breaking) for c in diff.changes] == [
        ("changed", "id", "became required", True),
        ("changed", "id", "type integer → string", True),
        ("added", "name", "property added (string)", False),
    ]
    assert diff.breaking == 2 and len(diff) == 3
    ddl = data.SchemaDiff(
        data.Schema.from_sql("CREATE TABLE users (id INT PRIMARY KEY, name TEXT);"),
        data.Schema.from_sql(
            "CREATE TABLE users (id BIGINT PRIMARY KEY, name TEXT NOT NULL, email TEXT UNIQUE);"
        ),
        old_name="v1",
        new_name="v2",
    )
    assert [f"{c.marker} {c.path} {c.detail}" for c in ddl.changes] == [
        "~ users.name became required",
        "~ users.id type INT → BIGINT",
        "+ users.email field added (TEXT)",
    ]
    assert ddl.summary.startswith("v1 → v2")


def test_a_schema_timeline():
    timeline = data.SchemaTimeline([("v1", OLD)]).push("v2", NEW, at=3.0)
    assert timeline.labels == ["v1", "v2"] and len(timeline) == 2
    [(label, diff)] = timeline.changes()
    assert label == "v2" and diff.breaking == 2
    out = drawn(timeline, width=70)
    assert "v2: +1 ~2, 2 breaking" in out


def test_an_er_diagram_of_ddl():
    diagram = data.ErDiagram.from_sql(DDL)
    assert diagram.entities == ["users", "posts"]
    assert diagram.relationships == [("posts", "users", "N:1")]
    assert diagram.notes == []
    out = drawn(diagram, width=80)
    assert "users" in out and "author" in out and "N:1" in out
    same = data.ErDiagram(data.Schema.from_sql(DDL), direction="TD", ascii=True)
    assert "+" in drawn(same, width=80)
    with pytest.raises(ValueError):
        data.ErDiagram.from_sql(DDL, direction="up")


# ---------------------------------------------------------------------------
# Conflicts and records


def test_a_conflict_view():
    view = data.ConflictView(CONFLICT, layout="stacked")
    [conflict] = view.conflicts
    assert (conflict.number, conflict.start_line, conflict.end_line) == (1, 2, 6)
    assert (conflict.ours, conflict.ours_label, conflict.theirs, conflict.theirs_label) == ("b", "HEAD", "c", "topic")
    assert conflict.base is None
    assert view.has_conflicts and len(view) == 1
    out = io.StringIO()
    Console(file=out, width=40, no_color=True).print(
        data.ConflictView("<<<<<<< HEAD\nb\n=======\nc\n>>>>>>> topic\n", layout="stacked")
    )
    assert out.getvalue() == "conflict 1 of 1, lines 1-5\n  ours: HEAD\n2 < b\n  theirs: topic\n4 > c\n"
    diff3 = data.ConflictView("<<<<<<< a\nx\n||||||| base\nw\n=======\ny\n>>>>>>> b\n")
    assert diff3.conflicts[0].base == "w" and diff3.conflicts[0].base_label == "base"
    with pytest.raises(data.ConflictError) as error:
        data.ConflictView("<<<<<<< a\nx\n")
    assert error.value.line == 1
    with pytest.raises(ValueError):
        data.ConflictView(CONFLICT, layout="diagonal")


def test_a_record_view():
    record = {"id": 7, "name": "web", "ports": [80, 443], "owner": {"team": "infra", "oncall": {"primary": "ana"}}}
    view = data.RecordView(record, expand=["owner.oncall"])
    out = drawn(view, width=44)
    assert "│ id    │ int  │ 7" in out
    assert 'primary: "ana"' in out
    assert view.branches == ["ports", "owner"]
    assert view.is_open("owner.oncall")
    folded = data.RecordView(record, depth=0, show_types=False, title="service")
    assert not folded.is_open("owner")
    assert "service" in drawn(folded)
    node = ext_data.DataNode.from_python(record)
    assert drawn(data.RecordView(node, expand=["owner.oncall"]), width=44) == out
    with pytest.raises(ValueError):
        data.RecordView(record, expand=["owner..x["])
