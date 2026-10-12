"""The pictures on docs/python/data.md: run it, or scripts/capture_python_docs.py."""

from _shots import Shots

from rs_rich import data


def read_rows(console):
    # --8<-- [start:rows]
    rows = data.read_csv("service,p99,up\nweb,120,true\napi,35.5,false\ndb,,true\n")
    print(rows.columns, rows.rows[0])
    console.print(rows)
    # --8<-- [end:rows]
    return rows


def inference(console, rows):
    # --8<-- [start:inference]
    inference = data.infer(rows)
    console.print(inference)
    print(inference.columns[1].summary)
    inference.apply(rows)
    print(rows.rows[2])
    p99 = data.Stats(rows).columns[1]
    print(p99.mean, p99.quantiles)
    # --8<-- [end:inference]
    return rows


def profile(console):
    # --8<-- [start:profile]
    profile = data.Profile.from_text("service,p99\nweb,120\napi,35\ndb,\nweb,80\n", name="p99.csv")
    print(profile.heading, profile.rows, profile.sampled)
    print(profile.column("p99")["mean"], profile.column("service")["distinct"])
    # --8<-- [end:profile]
    wide = console
    # --8<-- [start:profile-print]
    wide.print(profile)
    # --8<-- [end:profile-print]


def quality(console, rows):
    # --8<-- [start:quality]
    report = data.QualityReport([
        data.check_not_null(rows, "p99"),
        data.check_unique(rows, "service"),
        data.CheckResult("row_count", "warn", observed="3", expected=">= 100"),
    ])
    console.print(report)
    print(report.ok, report.totals)
    # --8<-- [end:quality]


def result_set(console, rows):
    # --8<-- [start:result-set]
    console.print(data.ResultSet(rows, elapsed=0.012))
    numbers = data.Rows(["n", "square"], [[i, i * i] for i in range(1, 1001)])
    table = data.VirtualTable(numbers, offset=500, height=3)
    console.print(table)
    table.scroll_by(1000)
    print(table.window(), table.position())
    # --8<-- [end:result-set]


# --8<-- [start:ddl]
ddl = """
CREATE TABLE users (id INT PRIMARY KEY, email TEXT UNIQUE);
CREATE TABLE posts (id INT PRIMARY KEY, author INT NOT NULL REFERENCES users (id));
"""
# --8<-- [end:ddl]


def schema(console):
    # --8<-- [start:schema]
    shop = data.Schema.from_sql(ddl)
    console.print(data.SchemaTree(shop, title="shop"))
    console.print(data.ErDiagram(shop))
    print(data.ErDiagram(shop).relationships)

    old = {"type": "object", "properties": {"id": {"type": "integer"}}}
    new = {"type": "object", "required": ["id"],
           "properties": {"id": {"type": "string"}, "name": {"type": "string"}}}
    diff = data.SchemaDiff(old, new, old_name="v1", new_name="v2")
    console.print(diff)
    print([(c.marker, c.path, c.breaking) for c in diff.changes])
    # --8<-- [end:schema]


def conflict(console):
    # --8<-- [start:conflict]
    console.print(data.ConflictView("a\n<<<<<<< HEAD\nb\n=======\nc\n>>>>>>> topic\nd\n", layout="stacked"))
    record = {"id": 7, "name": "web", "owner": {"team": "infra", "oncall": {"primary": "ana"}}}
    console.print(data.RecordView(record, expand=["owner.oncall"]))
    # --8<-- [end:conflict]


shots = Shots("data")
rows = shots.shot("rows", read_rows, width=60)
rows = shots.shot("inference", inference, rows, width=60)
shots.shot("profile", profile, width=72)
shots.shot("quality", quality, rows, width=60)
shots.shot("result-set", result_set, rows, width=60)
shots.shot("schema", schema, width=60)
shots.shot("conflict", conflict, width=60)
shots.finish()
