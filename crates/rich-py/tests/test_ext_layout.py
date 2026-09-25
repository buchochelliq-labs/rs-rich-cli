"""rs_rich.ext: bounded layouts, overflow policies and the live coordinator;
CLI authoring (help, errors, completions, docs, config and precedence).

Expected output comes from `rich-ext` itself (see test_ext_expected.py).
"""

from __future__ import annotations

import io

import pytest
from conftest import render
from test_ext_expected import EXPECTED

from rs_rich.console import Console
from rs_rich.ext import cli_doc, layout, live, target
from rs_rich.panel import Panel
from rs_rich.segment import Segment
from rs_rich.style import Style
from rs_rich.text import Text


def check(name, renderable, width):
    assert render(renderable, width=width) == EXPECTED[name]
    assert render(renderable, width=width, color=True) == EXPECTED[f"{name}/color"]


def dashboard():
    header = layout.LayoutNode(Text("acme deploy", style="bold white on blue"), height=1)
    sidebar = layout.LayoutNode(
        Text("api\nworker\nweb\ncron"), width=layout.Constraint(min=6, max=12), content_width=True
    )
    body = layout.LayoutNode(Panel(Text("v2.4.1 on 3 of 4 services\nweb: waiting for health check"), title="status"))
    footer = layout.LayoutNode(Text("q quit · r retry", style="dim"), height=1, align=("end", "start"))
    return layout.LayoutNode.split(
        "vertical", [header, layout.LayoutNode.split("horizontal", [sidebar, body]), footer]
    )


def test_layout_nodes():
    node = dashboard()
    node.validate()
    for name, width in [("layout/wide", 60), ("layout/narrow", 30)]:
        assert render(node, width=width, height=8) == EXPECTED[name]
        assert render(node, width=width, height=8, color=True) == EXPECTED[f"{name}/color"]
    check("layout/free", node, 40)
    with pytest.raises(layout.ConstraintError):
        layout.Constraint(min=5, max=2)


def test_allocate():
    constraints = [layout.Constraint.fixed(20), layout.Constraint(min=10, max=30), layout.Constraint(min=5, flex=2)]
    rows = []
    for total in (100, 60, 30, 12):
        sizes, padding, relaxed = layout.allocate(total, constraints)
        rows.append(f"{sizes}/{padding}/{relaxed}")
    assert ",".join(rows).replace(" ", "") == EXPECTED["layout/allocate"].replace(" ", "")


def test_overflowing():
    code = "let answer = compute_the_answer(universe, everything, 42);"
    for policy in ("fold", "crop", "ellipsis", "wrap"):
        check(f"layout/overflow-{policy}", layout.Overflowing(Text.from_markup(f"[bold]{code}[/]"), policy), 40)
    lines = layout.fit_segments([Segment("hello ", Style(bold=True)), Segment("wide world")], 7)
    shown = "\n".join("".join(f"{s.text}[{s.style or ''}]" for s in line) for line in lines)
    assert shown == EXPECTED["layout/fit"]


def test_live_coordinator():
    for name, kind, interactive in [("live/terminal", "terminal", True), ("live/stream", "plain_stream", False)]:
        out = io.StringIO()
        rt = target.RenderTarget(kind, width=40, height=10, color_system=None, interactive=interactive,
                                 hyperlinks=False)
        with live.LiveCoordinator(out, target=rt) as coordinator:
            build = coordinator.add(Text("build   running"))
            coordinator.add("tests   queued")
            coordinator.refresh()
            coordinator.print(Text("compiled 12 crates"))
            coordinator.update(build, "build   done")
            coordinator.refresh()
        assert out.getvalue() == EXPECTED[name]
    with pytest.raises(live.LiveCoordinatorError):
        coordinator.add("closed")


def test_live_countdown_and_toasts():
    from rs_rich.ext.notify import Notification, Notifications

    out = io.StringIO()
    rt = target.RenderTarget("plain_stream", width=40, height=10, color_system=None)
    coordinator = live.LiveCoordinator(out, target=rt)
    outcome = coordinator.countdown(2, lambda left: f"retrying in {left:.0f}s", tick=1, sleep=lambda s: None)
    assert outcome == "elapsed"
    toasts = Notifications(transient=False)
    toasts.push(Notification("saved", status="ok"), 0)
    coordinator.present(toasts, 0)
    coordinator.finish()
    assert out.getvalue().splitlines() == ["retrying in 2s", "✔ ok saved"]


def deploy_spec():
    A = cli_doc.ArgSpec
    return cli_doc.CommandSpec(
        "deploy",
        version="2.1.0",
        about="Ship a build to one or more regions",
        args=[
            A.positional("artifact", value="file", required=True, help="The build archive to upload"),
            A.option(
                "region",
                short="r",
                value_name="NAME",
                choices=[("eu-west-1", "Ireland"), ("us-east-2", "Ohio")],
                multiple=True,
                env="DEPLOY_REGION",
                config_key="deploy.region",
                help="Region to deploy to; repeat for several",
                heading="Targets",
            ),
            A.option(
                "parallel",
                short="j",
                value_name="N",
                default=4,
                config_key="deploy.parallel",
                help="Upload this many files at once",
                heading="Targets",
            ),
            A.flag("dry-run", short="n", help="Show the plan without uploading"),
            A.flag("verbose", short="v", help="Log every request"),
        ],
        subcommands=[
            cli_doc.CommandSpec("rollback", about="Restore the previous release"),
            cli_doc.CommandSpec("status", about="Show what is running where"),
        ],
        examples=[
            ("deploy build.tar -r eu-west-1", "Deploy to one region"),
            ("deploy build.tar -n -r eu-west-1 -r us-east-2", "Preview a two-region deploy"),
        ],
        sections=[("Exit status", "0 on success, 1 when an upload fails, 2 on a usage error.")],
    )


def test_help_and_docs():
    spec = deploy_spec()
    check("cli/help-wide", spec.help(), 80)
    check("cli/help-narrow", cli_doc.HelpView(spec), 48)
    check("cli/help-sub", spec.help(long=True, path=["rollback"]), 80)
    assert spec.to_markdown() == EXPECTED["cli/markdown"]
    assert spec.to_man(date="2026-09-25") == EXPECTED["cli/man"]
    assert render(spec.markdown(), width=80) == EXPECTED["cli/markdown-view"]
    for shell in cli_doc.COMPLETION_SHELLS:
        assert spec.completion(shell) == EXPECTED[f"cli/completion-{shell}"]
    check("cli/catalog", spec.completion_catalog(), 80)
    assert spec.args[1].names == "-r, --region <NAME>..."  # `multiple`
    assert "  -r, --region <NAME>..." in EXPECTED["cli/help-wide"]
    assert spec.find_subcommand("status").about == "Show what is running where"


def test_cli_errors():
    spec = deploy_spec()
    unknown = cli_doc.CliError("unknown_argument")  # the constructors below are the usual way
    assert unknown.kind == "unknown_argument"
    error = spec.unknown("--paralel")
    error = cli_doc.CliError(
        "unknown_argument",
        argument="--paralel",
        suggestions=error.suggestions,
        usage="\n".join(spec.usage_lines()),
        help_flag="--help",
    )
    check("cli/error-unknown", error.to_diagnostic(), 72)
    check("cli/error-unknown", error, 72)
    assert error.headline == EXPECTED["cli/error-headline"] and error.exit_code == 2
    invalid = cli_doc.CliError.invalid_value("--region <NAME>", "eu-west", ["eu-west-1", "us-east-2"])
    check("cli/error-invalid", invalid, 72)
    assert ",".join(cli_doc.suggest("eu-west", ["eu-west-1", "us-east-2"])) == EXPECTED["cli/suggest"]


def test_config_and_precedence():
    spec = deploy_spec()
    reference = cli_doc.ConfigReference.from_spec(
        spec,
        description="Settings can live in a file, the environment or on the command line.",
        sources=[
            ("defaults", "", "Built in"),
            ("user", "~/.config/deploy.toml", "Your settings"),
            ("environment", "DEPLOY_*", ""),
            ("command line", "", ""),
        ],
        entries=[
            cli_doc.ConfigEntry(
                "deploy.timeout", "duration", default="30s", description="Give up on an upload after this long"
            )
        ],
    )
    check("cli/config", reference, 88)
    assert reference.to_markdown() == EXPECTED["cli/config-md"]
    L = cli_doc.ConfigLayer
    precedence = cli_doc.Precedence(
        [
            L("defaults", {"deploy.parallel": "4", "deploy.timeout": "30s"}),
            L("user", {"deploy.parallel": 8, "deploy.region": "eu-west-1"}, origin="~/.config/deploy.toml"),
            L("env", {"deploy.region": "us-east-2"}, origin="DEPLOY_REGION"),
            L("flags", [("deploy.parallel", "2")]),
        ]
    )
    check("cli/precedence", precedence, 80)
    explanation = precedence.explain("deploy.parallel")
    check("cli/explain", explanation, 80)
    assert (explanation.value, explanation.winner, explanation.shadowed) == ("2", 3, [(0, "4"), (1, "8")])
    assert dict(precedence.resolve())["deploy.region"] == "us-east-2"
