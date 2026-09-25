"""rs_rich.ext: workflows, transfers, countdowns, notifications, badges,
size bars, formatters and redaction.

Expected output comes from `rich-ext` itself (see test_ext_expected.py).
"""

from __future__ import annotations

import io
import sys

import pytest
from conftest import render
from test_ext_expected import EXPECTED

from rs_rich.ext import badge, cancel, countdown, format, notify, redact, size_bar, transfer, workflow
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
    assert running.status == "running"
    check("workflow/running", running, 70)
    from rs_rich.ext.a11y import AccessibilityPolicy

    check("workflow/running-reduced", running.view(policy=AccessibilityPolicy.for_reduced_motion()), 70)


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
    assert tree.state(0) == "succeeded" and tree.state(3) == "running"
    assert tree.overall() == "running"
    assert tree.elapsed(1) == pytest.approx(9.2)
    assert tree.counts() == {"warning": 1, "running": 1, "pending": 1, "succeeded": 2}
    assert tree.children(3) == [4, 5] and tree.parent(4) == 3 and tree.roots() == [0, 3, 6]
    collapsed = workflow.TaskTree("x")  # a view's options live on the tree
    del collapsed
    tree.collapse_finished = True  # noqa: B018 - read-only attributes raise
    """


def test_task_tree_views_and_cancel():
    tree, _ = deploy()
    view = workflow.TaskTree  # noqa: F841
"""
