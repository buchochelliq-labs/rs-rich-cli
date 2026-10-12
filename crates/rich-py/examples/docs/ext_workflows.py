"""The pictures on docs/python/ext/workflows.md: run it, or scripts/capture_python_docs.py."""

from _shots import Shots

from rs_rich.ext import countdown, notify, transfer, workflow


def task_tree(console):
    # --8<-- [start:tree]
    clock = workflow.ManualClock()
    tree = workflow.TaskTree("Deploy", clock=clock)
    build = tree.add("build")
    compile_ = tree.add("compile", build)
    upload = tree.add("upload")
    tree.start(compile_)
    clock.advance(9.2)
    tree.succeed(compile_)
    tree.start(upload)
    tree.progress(upload, 21, 50)
    clock.advance(1.9)
    console.print(tree.view(animate=False))
    print(tree.overall(), tree.counts())
    # --8<-- [end:tree]
    return tree, upload


def summary(console, tree, upload):
    # --8<-- [start:summary]
    tree.fail(upload, "403 Forbidden")
    console.print(tree.summary(next_steps=["check the bucket policy"]))
    # --8<-- [end:summary]


def command(console):
    # --8<-- [start:command]
    record = workflow.CommandRecord(
        "cargo", ["test", "-p", "demo"],
        stdout="running 3 tests\ntest parse::quoted ... FAILED",
        stderr="error: test failed",
        status=101,
        duration=4.25,
    )
    print(record.state, record.detail, record.command_line)
    console.print(record.view(tail=2))
    # --8<-- [end:command]


def transfers(console):
    # --8<-- [start:transfers]
    iso = transfer.Transfer("debian-13.iso", total=650_000_000)
    for second in range(11):
        iso.update(second * 21_000_000, now=second)
    docs = transfer.Transfer("docs.zip", total=2_400_000)
    docs.finish(now=4)
    console.print(transfer.Transfers([iso, docs], summary=True))
    print(iso.state, iso.eta)
    # --8<-- [end:transfers]


def retries(console):
    # --8<-- [start:retries]
    backoff = countdown.Backoff(1, factor=2.0, max=30, attempts=4)
    print([backoff.delay(attempt) for attempt in range(1, 6)])
    # --8<-- [end:retries]
    wide = console
    # --8<-- [start:retries-print]
    wide.print(backoff.status(2, "503 Service Unavailable"))
    wide.print(countdown.RateLimit(42, scope="search API", limit=30, remaining=0, bar=20))
    # --8<-- [end:retries-print]


def notifications(console):
    # --8<-- [start:notifications]
    toasts = notify.Notifications(default_ttl=4)
    toasts.push(notify.Notification("checksum verified", status="ok", title="Download"), 0)
    toasts.push(notify.Notification("92% used", status="warning", title="Disk", ttl=30), 1)
    toasts.expire(5)
    console.print(toasts)
    # --8<-- [end:notifications]


shots = Shots("ext/workflows")
tree, upload = shots.shot("tree", task_tree, width=60)
shots.shot("summary", summary, tree, upload, width=60)
shots.shot("command", command, width=60)
shots.shot("transfers", transfers, width=60)
shots.shot("retries", retries, width=90)
shots.shot("notifications", notifications, width=60)
shots.finish()
