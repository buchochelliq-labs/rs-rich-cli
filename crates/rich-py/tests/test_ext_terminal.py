"""rs_rich.ext: capabilities, fidelity, accessibility, ANSI explained,
render targets, sanitizing and decoding.

Expected output comes from `rich-ext` itself (see test_ext_expected.py).
"""

from __future__ import annotations

import io

import pytest
from conftest import render
from test_ext_expected import EXPECTED

from rs_rich.ext import a11y, ansi_explain, capabilities, encoding, fidelity, sanitize, target
from rs_rich.console import Console
from rs_rich.ext.diagnostic import Diagnostic, Location
from rs_rich.panel import Panel
from rs_rich.style import Style
from rs_rich.text import Text

WEZTERM = {
    "TERM": "xterm-256color",
    "TERM_PROGRAM": "WezTerm",
    "COLORTERM": "truecolor",
    "LANG": "en_US.UTF-8",
}


def check(name, renderable, width):
    assert render(renderable, width=width) == EXPECTED[name]
    assert render(renderable, width=width, color=True) == EXPECTED[f"{name}/color"]


def wezterm(**extra):
    return capabilities.detect_capabilities({**WEZTERM, **extra}, terminal=True, size=(120, 40))


def test_capability_reports():
    report = wezterm()
    assert (report.color, report.color_system, report.hyperlinks) == ("truecolor", "truecolor", True)
    assert (report.width, report.height, report.terminal) == (120, 40, "WezTerm")
    check("capabilities/map", report, 90)
    ci = capabilities.detect_capabilities(
        {"GITHUB_ACTIONS": "true", "CI": "true", "RICH_WIDTH": "100", "RICH_HYPERLINKS": "maybe"}
    )
    assert ci.width == 100 and not ci.interactive and ci.ci is not None
    assert "|".join(ci.warnings) == EXPECTED["capabilities/ci-warnings"]
    check("capabilities/ci", ci, 90)
    overridden = capabilities.detect_capabilities(
        WEZTERM, terminal=True, size=(120, 40), color="256", unicode=False, width=40
    )
    rows = "\n".join(f"{n}={v}:{o}:{r}" for n, v, o, r in overridden.rows())
    assert rows == EXPECTED["capabilities/rows"]


def test_fidelity():
    levels = []
    for name, report in [
        ("WezTerm", wezterm()),
        ("NO_COLOR", wezterm(NO_COLOR="1")),
        ("piped", capabilities.detect_capabilities(WEZTERM, terminal=False, size=(120, 40))),
        ("LANG", wezterm(LANG="C.ISO-8859-1")),
    ]:
        facts = dict(unicode=report.unicode, color=report.color != "none", interactive=report.interactive,
                     animation=report.animation)
        levels.append(
            f"{name}:{fidelity.select_fidelity(**facts)}:{fidelity.select_fidelity(**facts, ceiling='rich')}"
        )
    assert ",".join(levels) == EXPECTED["fidelity/levels"]
    panel = Panel(
        Text.from_markup("[bold green]✔ ok[/]  [red]✖ failed[/]  → [link=https://ci.example/1]log[/]"),
        title="CI",
    )
    for level in ("rich", "styled", "plain", "ascii"):
        check(f"fidelity/degrade-{level}", fidelity.Degrade(panel, level=level), 50)
    assert fidelity.ascii_text("─ “quoted” → ok") == EXPECTED["fidelity/ascii"]


def test_accessibility_policy():
    lines = []
    for name, env in [
        ("none", {}),
        ("NO_COLOR", {"NO_COLOR": "1"}),
        ("sr", {"RICH_A11Y": "screen-reader"}),
        ("rm", {"RICH_A11Y": "reduced-motion,ascii-symbols"}),
        ("loud", {"RICH_A11Y": "loud"}),
    ]:
        policy = a11y.AccessibilityPolicy.from_env(env)
        lines.append(
            f"{name}:{policy.fidelity_ceiling()}:{policy.status('ok')}:{policy.status('error')}:"
            f"{policy.status('warning')}:{';'.join(policy.warnings)}"
        )
    assert "\n".join(lines) == EXPECTED["a11y/policies"]
    policy = a11y.AccessibilityPolicy.for_screen_reader()
    assert policy.screen_reader and policy.status_symbols == "words"
    assert a11y.status_marker("ok", "ascii") == "[OK]"
    themed = a11y.AccessibilityPolicy.for_monochrome().theme()
    out = io.StringIO()
    Console(file=out, width=20, force_terminal=True, color_system="truecolor", theme=themed).print(
        Text("12 True", style="repr.number")
    )
    assert out.getvalue() == EXPECTED["a11y/mono-render"]


def test_contrast():
    findings = a11y.check_theme()
    assert str(len(findings)) == EXPECTED["a11y/findings-count"]
    described = "\n".join(f"{f.style_name}|{f.describe()}" for f in findings[:5])
    assert described == EXPECTED["a11y/findings"]
    check("a11y/contrast", a11y.ContrastReport(findings), 100)
    numbers = EXPECTED["a11y/numbers"].split("|")
    assert a11y.contrast_ratio("#ff0000", "#ffffff") == float(numbers[0])
    assert a11y.contrast_ratio((255, 0, 0), "#ffffff") == float(numbers[0])
    assert a11y.relative_luminance((0, 0, 128)) == float(numbers[1])
    assert a11y.delta_e((255, 0, 0), (0, 0, 128)) == float(numbers[2])
    assert a11y.to_lab((255, 0, 0)) == [float(x) for x in numbers[3].strip("[]").split(", ")]
    red_deutan = numbers[4]
    simulated = a11y.simulate_deficiency((255, 0, 0), "deutan")
    assert red_deutan == f"ColorTriplet {{ red: {simulated[0]}, green: {simulated[1]}, blue: {simulated[2]} }}"
    suggested = a11y.suggest_color((120, 120, 120), (255, 255, 255), 4.5)
    assert numbers[5] == f"Some(ColorTriplet {{ red: {suggested[0]}, green: {suggested[1]}, blue: {suggested[2]} }})"


def test_screen_reader_text():
    from rs_rich.ext.badge import Badge, Badges
    from rs_rich.ext.table import DataColumn, TableData

    assert a11y.semantic_text("[bold]Summary[/]", 40) == "Summary"
    diagnostic = Diagnostic.error(
        "mismatched types", code="E0308", location=Location("src/main.rs", 4, 18), help=["change the type to `u64`"]
    )
    assert a11y.accessible_text(diagnostic) == EXPECTED["a11y/accessible-diagnostic"]
    badges = Badges(
        [
            Badge.status("ok", "build"),
            Badge.status("error", "tests"),
            Badge.status("warning", "lint"),
            Badge.status("pending", "deploy"),
            Badge.label("beta"),
            Badge.meta("version", "0.0.11"),
            Badge.link("docs", "https://example.com/docs"),
        ]
    )
    assert a11y.accessible_text(badges) == EXPECTED["a11y/accessible-badges"]
    table = TableData(
        ["service", "region", DataColumn("errors", justify="right"),
         DataColumn("p99", justify="right", format=lambda v: "[dim]-[/]" if v is None else f"{v:.0f} ms")],
        [["api", "eu", 3, 120.0], ["web", "us", 0, 80.0], ["db", "eu", 7, None], ["cache", "us", 1, 4.0],
         ["worker-10", "eu", 3, 95.0], ["worker-9", None, 0, 60.0]],
    )
    assert a11y.accessible_text(table) == EXPECTED["a11y/accessible-table"]


CAPTURE = (
    "\x1b]0;build\x07\x1b[1;32m✔\x1b[0m compiled \x1b[38;5;208m3 crates\x1b[39m\n"
    "\x1b]8;;https://ci.example/42\x1b\\log\x1b]8;;\x1b\\ \x1b[2K\x1b[1A\x1b["
)


def test_ansi_explained():
    explanation = ansi_explain.explain(CAPTURE)
    assert explanation.visible_text == "✔ compiled 3 crates\nlog "
    assert len(explanation.invalid()) == 1
    check("ansi/table", explanation, 88)
    check("ansi/table", ansi_explain.ExplanationView(CAPTURE), 88)
    check("ansi/escapes-only", ansi_explain.ExplanationView(explanation, escapes_only=True, show_visible=False, raw_width=24), 88)
    check("ansi/inline", ansi_explain.ExplanationView(explanation, mode="inline"), 88)
    assert ansi_explain.ExplanationView(explanation).inline_text(ascii=True) == EXPECTED["ansi/inline-text"]
    # Offsets and lengths are in characters: "✔" is one, not three bytes.
    rust = []
    for line in EXPECTED["ansi/tokens"].split("\n"):
        offset, length, kind, raw, meaning = line.split("|", 4)
        rust.append((int(offset), int(length), kind, meaning))
    ours = [(t.offset, t.length, t.kind, t.meaning) for t in explanation.tokens]
    byte_offsets = [len(CAPTURE[:t.offset].encode()) for t in explanation.tokens]
    assert [(o, k, m) for o, (_, _, k, m) in zip(byte_offsets, ours)] == [(o, k, m) for o, _, k, m in rust]
    sgr = "\n".join(f"{c}={d}" for c, d in ansi_explain.sgr_effects("4:3;58;2;255;0;0;1;38;5;208"))
    assert sgr == EXPECTED["ansi/sgr"]
    misc = "|".join(
        [
            ansi_explain.csi_meaning("2", "", "J"),
            ansi_explain.osc_meaning("8;;https://x.dev"),
            ansi_explain.control_name(0x1B),
            ansi_explain.escape_visible("\x1b[0m\x07"),
            ansi_explain.explain(b"\x9b31mred\x9b0m").visible_text,
        ]
    )
    assert misc == EXPECTED["ansi/misc"]


def test_render_targets():
    text = Text("docs", style="bold red")
    text.stylize(Style(link="https://acme.dev"), 0, 4)
    for kind in ("terminal", "plain_stream", "capture"):
        rt = target.RenderTarget(kind, width=20, height=1, color_system="standard", interactive=True)
        assert rt.text(text) == EXPECTED[f"target/{kind}"]
    stream = target.RenderTarget("plain_stream", interactive=True)
    assert stream.capabilities["color_system"] is None and not stream.capabilities["interactive"]
    caps, origins = target.resolve_target_capabilities(width=100, is_terminal=True, overrides={"width": 60})
    assert caps["width"] == 60 and origins["width"] == "configured" and origins["height"] == "default"


def test_sanitize_and_decode():
    line = "|".join(
        [
            sanitize.sanitize_terminal_controls("a\x1b[31mb\x07c\td\ne"),
            sanitize.sanitize_terminal_and_bidi_controls("x‮y"),
            sanitize.sanitize_single_line("one\ntwo\r"),
        ]
    )
    assert line == EXPECTED["sanitize/all"]
    assert sanitize.is_bidi_control("‮") and not sanitize.is_bidi_control("a")
    assert encoding.decode_text("héllo".encode("utf-16"), "utf-16") == "héllo"
    assert encoding.has_utf16_bom(b"\xff\xfeh\x00")
    with pytest.raises(encoding.EncodingError):
        encoding.decode_text(b"\xff", "utf-8")
    with pytest.raises(ValueError):
        encoding.decode_text(b"x", "latin-1")
