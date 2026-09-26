"""rs_rich.ext: workflows, transfers, countdowns, notifications, badges,
size bars, formatters and redaction.

Expected output comes from `rich-ext` itself (see test_ext_expected.py).
"""

from __future__ import annotations

import datetime
import io
import sys

import pytest
from conftest import render
from test_ext_expected import EXPECTED

from rs_rich.ext import badge, cancel, countdown, format, notify, redact, size_bar, transfer, workflow
from rs_rich.ext.a11y import AccessibilityPolicy
from rs_rich.panel import Panel
from rs_rich.text import Text


def check(name, renderable, width):
    assert render(renderable, width=width) == EXPECTED[name]
    assert render(renderable, width=width, color=True) == EXPECTED[f"{name}/color"]


def test_command_records():
    record = workflow.CommandRecord(
        "cargo",
        ["build", "--release"],
        stdout="   Compiling rs-rich v0.0.11\n   Compiling rs-rich-ext v0.0.10",
        status=0,
        duration=41.3,
    )
    record.push("stderr", "warning: unused variable: `width`")
    record.push("stdout", "    Finished `release` profile [optimized] target(s)")
    assert record.state == "succeeded" and record.success and record.returncode == 0
    assert record.output("stderr") == "warning: unused variable: `width`"
    check("workflow/command", record.view(tail=3), 70)

    failure = workflow.CommandRecord(
        "cargo",
        ["test", "-p", "demo"],
        cwd="crates/demo",
        stdout="running 3 tests\ntest parse::empty ... ok\ntest parse::quoted ... FAILED",
        stderr="error: test failed, to rerun pass `--lib`",
        status=101,
        duration=4.25,
    )
    assert failure.detail == "exit 101"
    assert failure.command_line == "cargo test -p demo"
    check("workflow/failure", failure.view(help=["rerun one test with `cargo test parse::quoted`"]), 70)
    check("workflow/failure-diagnostic", failure.diagnostic(), 70)

    running = workflow.CommandRecord("npm", ["install"], stdout="added 212 packages", duration=3.4)
    assert running.status == "running" and running.diagnostic() is None
    check("workflow/running", running, 70)
    check("workflow/running-reduced", running.view(policy=AccessibilityPolicy.for_reduced_motion()), 70)


def test_run_command():
    lines = []
    record = workflow.run_command(
        [sys.executable, "-c", "print('out'); import sys; print('err', file=sys.stderr); sys.exit(3)"],
        on_update=lambda r: lines.append(len(r.lines)),
    )
    assert record.returncode == 3 and record.state == "failed"
    assert sorted(record.lines) == [("stderr", "err"), ("stdout", "out")]
    assert lines and lines[-1] == 2
    missing = workflow.run_command(["definitely-not-a-program-xyz"])
    assert missing.status[0] == "failed_to_start"


def deploy():
    clock = workflow.ManualClock()
    tree = workflow.TaskTree("Deploy", clock=clock)
    build = tree.add("build")
    compile_ = tree.add("compile", build)
    link = tree.add("link", build)
    upload = tree.add("upload")
    assets = tree.add("assets", upload)
    images = tree.add("images", upload)
    tree.add("verify")
    tree.start(compile_)
    clock.advance(9.2)
    tree.succeed(compile_)
    tree.start(link)
    clock.advance(3.2)
    tree.succeed(link)
    tree.start(assets)
    clock.advance(1.1)
    tree.warn(assets, "2 files unchanged")
    tree.start(images)
    tree.progress(images, 21, 50)
    clock.advance(1.9)
    return tree, clock


def test_task_tree():
    tree, _ = deploy()
    check("workflow/tree", tree, 70)
    check("workflow/tree-collapsed", tree.view(collapse_finished=True, animate=False), 70)
    assert tree.state(0) == "succeeded" and tree.state(3) == "running"
    assert tree.overall() == "running" and not tree.is_finished
    assert tree.elapsed(1) == pytest.approx(9.2)
    assert tree.counts() == {"warning": 1, "running": 1, "pending": 1, "succeeded": 2}
    assert tree.children(3) == [4, 5] and tree.parent(4) == 3 and tree.roots() == [0, 3, 6]
    assert tree.get_progress(5) == (21, 50)
    with pytest.raises(IndexError):
        tree.start(99)


def test_task_tree_cancel_and_summary():
    tree, _ = deploy()
    worker = tree.token(5)
    tree.cancel(3)
    assert worker.is_cancelled
    check("workflow/tree-cancel", tree.view(collapse_finished=True), 70)

    tree, clock = deploy()
    clock.advance(2.6)
    tree.fail(5, "403 Forbidden")
    tree.skip(6, "upload failed")
    summary = tree.summary(next_steps=["check the bucket policy", "rerun with `deploy --resume`"])
    check("workflow/summary", summary, 70)
    assert summary.overall() == "failed"
    plain = workflow.CompletionSummary(
        "Lint", counts={"succeeded": 118, "warning": 2}, duration=6.8, next_steps=["run `lint --fix`"], symbols="ascii"
    )
    check("workflow/summary-plain", plain, 70)
    assert workflow.state_marker("running", "ascii") == "[RUN]"
    assert workflow.state_label("warning", 3) == "3 warnings"


def test_cancel_tokens():
    token = cancel.CancelToken()
    child = token.child()
    child.cancel()
    assert child.is_cancelled and not token.is_cancelled
    token.cancel()
    assert token.child().is_cancelled and bool(token)


def session():
    iso = transfer.Transfer("debian-13.iso", total=650_000_000)
    pkg = transfer.Transfer("packages.tar.zst", total=48_000_000, max_attempts=3)
    log = transfer.Transfer("session.log", direction="upload")
    docs = transfer.Transfer("docs.zip", total=2_400_000)
    for s in range(11):
        iso.update(s * 21_000_000, now=s)
        log.update(s * 40_000, now=s)
    pkg.advance(19_500_000, now=6)
    assert pkg.retry("connection reset by peer")
    docs.finish(now=4)
    return [iso, pkg, log, docs]


def test_transfers():
    items = session()
    group = transfer.Transfers(items, summary=True)
    check("transfer/group", group, 90)
    assert render(transfer.Transfers(items, summary=True, symbols="words"), width=90) == EXPECTED["transfer/words"]
    check("transfer/one", items[0], 90)
    iso, pkg, _, docs = items
    assert (iso.state, pkg.state, docs.state) == ("active", "retrying", "done")
    assert iso.rate == pytest.approx(21_000_000) and iso.eta == 21  # ceil(440 MB / 21 MB/s)
    assert pkg.attempt == 2 and pkg.error == "connection reset by peer"
    assert iso.task_fields() == {"total": 650_000_000.0, "completed": 210_000_000.0, "description": "debian-13.iso"}


def test_transfer_file_wrappers():
    clock = workflow.ManualClock()
    down = transfer.Transfer("data.bin", total=10, clock=clock)
    reader = down.wrap_reader(io.BytesIO(b"0123456789"))
    assert reader.read(4) == b"0123"
    clock.advance(1)
    assert reader.read() == b"456789"
    assert (down.completed, down.state) == (10, "done")

    up = transfer.Transfer("out.txt", direction="upload")
    sink = io.BytesIO()
    writer = up.wrap_writer(sink)
    assert writer.write("héllo") == 6
    assert sink.getvalue() == "héllo".encode() and up.completed == 6

    token = cancel.CancelToken()
    stopped = transfer.Transfer("x")
    token.cancel()
    with pytest.raises(transfer.TransferCancelled):
        stopped.wrap_reader(io.BytesIO(b"abc"), cancel=token).read()
    assert stopped.state == "cancelled"


def test_countdowns():
    backoff = countdown.Backoff(1, factor=2.0, max=30, attempts=5, jitter=0.25, seed=7)
    expected = [None if d == "None" else float(d[5:-1]) for d in EXPECTED["countdown/delays"].split(",")]
    assert [backoff.delay(a) for a in range(1, 7)] == expected
    for attempt in range(1, 6):
        check(f"countdown/retry-{attempt}", backoff.status(attempt, "503 Service Unavailable"), 80)
    assert countdown.Backoff(8, attempts=3).status(1, "connection refused").status == "warning"
    status = countdown.RetryStatus(1, max_attempts=3, reason="connection refused", retrying_in=8, bar=8)
    for left in (8, 5, 2):
        check(f"countdown/at-{left}", status.at(left), 80)
    limit = countdown.RateLimit(42, scope="search API", limit=30, remaining=0, bar=60)
    check("countdown/rate-limit", limit, 80)
    check("countdown/bar", countdown.CountdownBar(10, 4, width=12), 40)
    assert countdown.remaining_label(3.1) == "4s" and countdown.remaining_label(65) == "1m 05s"


def test_countdown_wait():
    ticks, sleeps = [], []
    outcome = countdown.countdown_wait(1, tick=0.25, on_tick=ticks.append, sleep=sleeps.append)
    assert outcome == "elapsed"
    assert ticks == [1.0, 0.75, 0.5, 0.25, 0.0] and sleeps == [0.25] * 4
    token = cancel.CancelToken()
    token.cancel()
    assert countdown.countdown_wait(5, cancel=token) == "cancelled"


def test_notifications():
    toasts = notify.Notifications(default_ttl=4, max_visible=3)
    toasts.push(notify.Notification("debian-13.iso verified", status="ok", title="Checksum"), 0)
    toasts.push(notify.Notification("switched to mirror 2", status="info", title="Net"), 1)
    toasts.push(notify.Notification("92% used", status="warning", title="Disk", ttl=30), 2)
    toasts.push(notify.Notification("session.log: 403", status="error", title="Upload"), 3)
    check("notify/stack", toasts, 70)
    assert toasts.next_expiry() == 4.0
    assert toasts.expire(5) == 2
    check("notify/expired", toasts, 70)
    panel = notify.Notification("upload rejected: 403 Forbidden", status="error", title="session.log", toast="panel")
    check("notify/panel", panel, 70)
    log = notify.Notifications(transient=False)
    log.push(notify.Notification("done"), 0)
    assert len(log) == 0 and [n.message for n in log.take_log()] == ["done"]


def release_badges():
    return badge.Badges(
        [
            badge.Badge.status("ok", "build"),
            badge.Badge.status("error", "tests"),
            badge.Badge.status("warning", "lint"),
            badge.Badge.status("pending", "deploy"),
            badge.Badge.label("beta"),
            badge.Badge.meta("version", "0.0.11"),
            badge.Badge.link("docs", "https://example.com/docs"),
        ]
    )


def test_badges_and_size_bars():
    check("badge/badges", release_badges(), 80)
    assert release_badges().plain == EXPECTED["badge/plain"]
    for name, size in [("rs-rich", 2_400_000), ("rs-rich-ext", 9_300_000), ("rs-rich-art", 12_600_000)]:
        check(f"size_bar/{name}", size_bar.SizeBar.limit(size, 10_000_000, label=f"{name:<12}"), 70)
    disk = size_bar.SizeBar(3 << 30, 8 << 30, label="disk        ", units="binary")
    check("size_bar/disk", disk, 70)
    assert disk.ratio == 0.375 and not disk.is_high
    assert size_bar.SizeBar.limit(12, 10).is_over


def test_formatters():
    then = 1_790_000_000
    rows = [
        format.format_size(1_500_000),
        format.format_size(1_572_864, units="binary"),
        format.format_rate(2_400_000.0),
        format.format_duration(3723),
        format.format_duration(datetime.timedelta(microseconds=12), ascii=True),
        format.format_clock(3723),
        format.format_relative(then, then + 3 * 3600),
        format.format_relative(then + 90, then),
        format.format_timestamp(then),
        format.format_percent(0.4251, 1),
        format.format_number(1_234_567),
        format.format_compact(1_250_000.0),
    ]
    assert "|".join(rows) == EXPECTED["format/all"]


LOG = (
    "GET /api?token=abc123 200\n"
    "Authorization: Bearer eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxIn0.c2ln\n"
    "push with ghp_0123456789abcdefghijklmnopqrstuvwxyzAB\n"
    "DATABASE_URL=postgres://app:s3cret@db/app\n"
    "order 1234-5678 shipped"
)


def test_redaction():
    redactor = redact.Redactor(secrets=True, patterns=[("order", r"order (?P<secret>\d{4})")])
    check("redact/panel", redact.Redacted(Panel(Text(LOG), title="server.log"), redactor), 72)
    assert redactor.redact(LOG) == EXPECTED["redact/str"]
    secrets = redact.Redactor.secrets()
    assert secrets.redact_ansi("\x1b[1mpassword=hunter2\x1b[0m ok") == EXPECTED["redact/ansi"]
    assert " ".join(secrets.redact_args(["--token", "abc123", "--name", "x", "--password=pw"])) == EXPECTED[
        "redact/args"
    ]
    assert "|".join(secrets.redact_chunks(["api_key=ab", "cdef rest\n"])) == EXPECTED["redact/chunks"]
    found = redactor.find("token=abc123 and order 9876")
    assert ",".join(f"{m.start}:{m.end}:{m.kind}" for m in found) == EXPECTED["redact/find"]
    assert secrets.capture(Text("password=hunter2"), width=40) == EXPECTED["redact/capture"]
    assert secrets.export_text(Text("password=hunter2"), width=40) == EXPECTED["redact/export-text"]
    assert "hunter2" not in secrets.export_svg(Text("password=hunter2"))
    assert "hunter2" not in secrets.export_html(Text("password=hunter2"))
    assert redact.is_secret_key("API-KEY")
    with pytest.raises(redact.RedactPatternError):
        redact.Redactor(patterns=["("])
