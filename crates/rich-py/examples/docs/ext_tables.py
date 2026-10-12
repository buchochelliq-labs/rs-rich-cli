"""The pictures on docs/python/ext/tables.md: run it, or scripts/capture_python_docs.py."""

from _shots import Shots

import dataclasses

from rs_rich.ext import badge, derive, redact, size_bar, table
from rs_rich.panel import Panel


def typed(console):
    # --8<-- [start:typed]
    def latency(value):
        return "[dim]-[/]" if value is None else f"{value:.0f} ms"

    services = table.TableData(
        ["service", "region", table.Column("errors", justify="right"),
         table.Column("p99", justify="right", format=latency)],
        [["api", "eu", 3, 120.0], ["web", "us", 0, 80.0], ["db", "eu", 7, None],
         ["worker-10", "eu", 3, 95.0], ["worker-9", "us", 0, 60.0]],
        sort=[1, 0],
        group_by=table.GroupBy(1, aggregates=[table.Aggregate("sum", 2)]),
        totals=[table.Aggregate("count", 0), table.Aggregate("mean", 3)],
    )
    console.print(services)
    # --8<-- [end:typed]


def streaming(console):
    # --8<-- [start:streaming]
    jobs = table.StreamingTable(["job", "state"], window=("tail", 3))
    for job in ["fetch", "build", "test", "lint"]:
        jobs.upsert(job, [job, "queued"])
    jobs.update_cell("build", 1, "running")
    console.print(jobs)
    print(jobs["build"], len(jobs))
    # --8<-- [end:streaming]


def records(console):
    # --8<-- [start:records]
    @dataclasses.dataclass
    class Server:
        name: str
        port: int

    console.print(derive.Record.from_object(Server("web", 8080), presentation="panel"))
    console.print(derive.records_table([Server("web", 8080), Server("db", 5432)]))
    # --8<-- [end:records]


def badges(console):
    # --8<-- [start:badges]
    console.print(badge.Badges([
        badge.Badge.status("ok", "build"),
        badge.Badge.status("error", "tests"),
        badge.Badge.meta("version", "0.0.11"),
    ]))
    console.print(size_bar.SizeBar.limit(9_300_000, 10_000_000, label="rs-rich-ext "))
    console.print(size_bar.SizeBar(3 << 30, 8 << 30, label="disk        ", units="binary"))
    # --8<-- [end:badges]


def redaction(console):
    # --8<-- [start:redaction]
    redactor = redact.Redactor(secrets=True, patterns=[("order", r"order (?P<secret>\d{4})")])
    print(redactor.redact("GET /api?token=abc123 for order 1234"))
    print(" ".join(redact.Redactor.secrets().redact_args(["--password", "pw", "--name", "x"])))
    console.print(redact.Redacted(Panel("DATABASE_URL=postgres://app:s3cret@db/app"), redactor))
    # --8<-- [end:redaction]


shots = Shots("ext/tables")
shots.shot("typed", typed, width=60)
shots.shot("streaming", streaming, width=80)
shots.shot("records", records, width=80)
shots.shot("badges", badges, width=80)
shots.shot("redaction", redaction, width=80)
shots.finish()
