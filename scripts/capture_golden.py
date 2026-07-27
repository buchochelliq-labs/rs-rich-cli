#!/usr/bin/env python3
"""Regenerate golden parity fixtures from the real Python `rich` library.

This is the source of truth for the byte-parity tests in
`crates/rich/tests/golden.rs`. When syncing a new upstream release (see the
`sync-upstream` skill), install the matching `rich` version and re-run this to
refresh the expected output:

    pip install "rich==<version-from-UPSTREAM.toml>"
    python scripts/capture_golden.py

Only add cases here whose behavior the Rust core is expected to reproduce
exactly. Features that are *our* additions (e.g. the [error]/[warning]/[info]
theme conveniences) are NOT upstream and must not be captured here.
"""

from __future__ import annotations

import pathlib

from rich import box
from rich.align import Align
from rich.ansi import AnsiDecoder
from rich.bar import Bar as HBar
from rich.columns import Columns
from rich.console import Console
from rich.constrain import Constrain
from rich.control import Control
from rich.json import JSON
from rich.layout import Layout
from rich.markdown import Markdown
from rich.padding import Padding
from rich.panel import Panel
from rich.progress import BarColumn, Progress, TaskProgressColumn, TextColumn
from rich.progress_bar import ProgressBar

JSON_SAMPLE = (
    '{"name": "Alice", "age": 30, "admin": true, '
    '"tags": ["a", "b"], "meta": null}'
)
from rich.rule import Rule
from rich.styled import Styled
from rich.table import Table
from rich.text import Text
from rich.tree import Tree


def _progress_table():
    """The default-column Progress display (no time columns) as a static grid."""
    progress = Progress(
        TextColumn("[progress.description]{task.description}"),
        BarColumn(),
        TaskProgressColumn(),
    )
    progress.add_task("Downloading", total=100, completed=50)
    progress.add_task("Processing", total=100, completed=100)
    progress.add_task("Waiting", total=100, completed=0)
    return progress.make_tasks_table(progress.tasks)


def _ansi(text: str):
    """Decode an ANSI string into a single styled Text (fresh decoder)."""
    return AnsiDecoder().decode_line(text)


def _layout_column() -> Layout:
    layout = Layout()
    layout.split_column(Layout(Text("top")), Layout(Text("bottom")))
    return layout


def _layout_row() -> Layout:
    layout = Layout()
    layout.split_row(Layout(Text("L")), Layout(Text("R")))
    return layout


def _layout_nested() -> Layout:
    layout = Layout()
    top = Layout()
    top.split_row(Layout(Text("A")), Layout(Text("B")))
    layout.split_column(top, Layout(Text("bottom"), size=1))
    return layout


def _layout_panel() -> Layout:
    # A single Panel leaf expands to fill the region height.
    return Layout(Panel("hi", box=box.SQUARE))


def _layout_row_panels() -> Layout:
    layout = Layout()
    layout.split_row(
        Layout(Panel("L", box=box.SQUARE)),
        Layout(Panel("R", box=box.SQUARE)),
    )
    return layout


def _tree() -> Tree:
    tree = Tree("root")
    child_a = tree.add("child A")
    child_a.add("leaf A1")
    child_a.add("leaf A2")
    tree.add("child B")
    return tree


def _table(box_set) -> Table:
    table = Table(box=box_set)
    table.add_column("Name")
    table.add_column("Age")
    table.add_row("Alice", "30")
    table.add_row("Bob", "7")
    return table


def _shrink_table() -> Table:
    table = Table(box=box.SQUARE)
    table.add_column("Name")
    table.add_column("Description")
    table.add_row("Alice", "A software engineer who likes Rust")
    table.add_row("Bob", "Short bio")
    return table


def _expand_table() -> Table:
    table = Table(box=box.SQUARE, expand=True)
    table.add_column("Name")
    table.add_column("Age")
    table.add_row("Alice", "30")
    table.add_row("Bob", "7")
    return table


def _title_table() -> Table:
    table = Table(box=box.SQUARE, title="Users", caption="2 rows")
    table.add_column("Name")
    table.add_column("Age")
    table.add_row("Alice", "30")
    table.add_row("Bob", "7")
    return table


def _lines_table() -> Table:
    table = Table(box=box.SQUARE, show_lines=True)
    table.add_column("Name")
    table.add_column("Age")
    table.add_row("Alice", "30")
    table.add_row("Bob", "7")
    return table


def _width_table() -> Table:
    table = Table(box=box.SQUARE)
    table.add_column("Id")
    table.add_column("Note", width=8)
    table.add_row("1", "alpha beta gammagammagamma")
    table.add_row("2", "ok")
    return table


def _style_table() -> Table:
    table = Table(box=box.SQUARE)
    table.add_column("Name", style="red")
    table.add_column("Age")
    table.add_row("Alice", "30")
    table.add_row("Bob", "7")
    return table


def _nowrap_table() -> Table:
    # A single no_wrap column that must crop to one line (with ellipsis).
    table = Table(box=box.SQUARE)
    table.add_column("Note", no_wrap=True)
    table.add_row("this is a fairly long note that will not fit")
    table.add_row("short")
    return table


def _justify_table() -> Table:
    table = Table(box=box.SQUARE)
    table.add_column("L", justify="left")
    table.add_column("C", justify="center")
    table.add_column("R", justify="right")
    table.add_row("a", "bb", "ccc")
    table.add_row("xxxx", "y", "zz")
    return table

# (name, console-markup) — keep in sync with the Rust test's expectations.
CASES: list[tuple[str, str]] = [
    ("bold_red", "[bold red]hello[/]"),
    ("two_tags", "[bold]Hello[/] [red]World[/]"),
    ("fg_on_bg", "[white on blue]bg[/]"),
    ("nested_inner_wins", "[red]a[blue]x[/]b[/]"),
    ("hex_truecolor", "[#ff8800]x[/]"),
    ("plain_text", "no styles here"),
    ("emoji_rocket", ":rocket: launch :fire:"),
    ("emoji_full_table", "deploy :ship: at :clock3: with :zap:"),
    ("named_color_orange1", "[orange1]sun[/]"),
    ("named_color_grey37", "[grey37]dim[/]"),
    ("named_on_named", "[white on deep_sky_blue1]hi[/]"),
    # `[…]` is only a tag when it starts with [a-z#/@]; else it is literal.
    ("literal_bracket_upper", "[Hello] world"),
    ("literal_bracket_num", "[42] items"),
    ("meta_tag_no_style", "[@handler]y[/]"),
]

# (name, width, renderable) — the Rust test builds a matching renderable per name.
# `legacy_windows`/`safe_box` are pinned OFF for deterministic, platform-neutral
# box glyphs (the port does not yet do platform box substitution).
RENDERABLE_CASES = [
    ("rule_plain", 20, Rule()),
    ("rule_title", 20, Rule("Hi")),
    ("rule_title_odd", 21, Rule("Hi")),
    ("rule_left", 20, Rule("Hi", align="left")),
    ("rule_right", 20, Rule("Hi", align="right")),
    ("panel_plain", 20, Panel("hello")),
    ("panel_title", 20, Panel("hello", title="T")),
    ("panel_title_left", 20, Panel("x", title="T", title_align="left", box=box.SQUARE)),
    ("panel_title_right", 20, Panel("x", title="T", title_align="right", box=box.SQUARE)),
    ("panel_square", 20, Panel("hi", box=box.SQUARE)),
    ("panel_subtitle", 20, Panel("x", subtitle="S", box=box.SQUARE)),
    ("panel_subtitle_left", 20, Panel("x", subtitle="S", subtitle_align="left", box=box.SQUARE)),
    ("panel_title_and_sub", 20, Panel("x", title="T", subtitle="S", box=box.SQUARE)),
    ("padding_1_2", 10, Padding("hi", (1, 2))),
    ("padding_0_1", 10, Padding("hi", (0, 1))),
    ("wrap_words", 10, Text("The quick brown fox")),
    ("wrap_fold", 6, Text("abcdefghij")),
    ("panel_wrap", 14, Panel("The quick brown fox", box=box.SQUARE)),
    ("panel_just_center", 14, Panel(Text("hi", justify="center"), box=box.SQUARE)),
    ("panel_just_right", 14, Panel(Text("hi", justify="right"), box=box.SQUARE)),
    ("panel_just_left", 14, Panel(Text("hi", justify="left"), box=box.SQUARE)),
    # A bare justified Text is shrunk to its content width (measurement-fit),
    # so it appears unpadded — the payoff of the Measurement port.
    ("text_justify_bare", 10, Text("hi", justify="center")),
    ("table_square", 40, _table(box.SQUARE)),
    ("table_default", 40, _table(box.HEAVY_HEAD)),
    ("table_shrink", 30, _shrink_table()),
    ("table_expand", 30, _expand_table()),
    ("table_justify", 30, _justify_table()),
    ("table_title", 30, _title_table()),
    ("table_lines", 30, _lines_table()),
    ("table_col_width", 40, _width_table()),
    ("table_col_style", 40, _style_table()),
    ("table_nowrap", 20, _nowrap_table()),
    ("tree_nested", 40, _tree()),
    ("align_center", 20, Align.center("hi")),
    ("align_right", 20, Align.right("hi")),
    ("align_center_odd", 21, Align.center("hi")),
    ("constrain_panel", 20, Constrain(Panel("hi", box=box.SQUARE), width=10)),
    ("columns_two_rows", 20, Columns(["one", "two", "three", "four", "five", "six"])),
    ("columns_one_row", 30, Columns(["alpha", "beta", "gamma", "delta"])),
    ("bar_empty", 20, ProgressBar(total=100, completed=0, width=20)),
    ("bar_half", 20, ProgressBar(total=100, completed=50, width=20)),
    ("bar_third", 20, ProgressBar(total=100, completed=33, width=20)),
    ("bar_full", 20, ProgressBar(total=100, completed=100, width=20)),
    ("json_object", 40, JSON(JSON_SAMPLE)),
    ("markdown_doc", 24, Markdown("# Title\n\nHello **bold** and *italic* and `code`.")),
    ("markdown_list", 20, Markdown("Items:\n\n- one\n- two\n\n1. a\n2. b")),
    ("markdown_quote_hr", 20, Markdown("Note:\n\n> important\n\n---\n\ndone")),
    ("hbar_full", 20, HBar(size=100, begin=0, end=100, width=20)),
    ("hbar_mid", 20, HBar(size=100, begin=25, end=75, width=20)),
    ("hbar_edge", 20, HBar(size=100, begin=0, end=33, width=20)),
    ("control_clear", 20, Control.clear()),
    ("control_move", 20, Control.move(2, -1)),
    ("control_move_to", 20, Control.move_to(3, 4)),
    ("control_hide_cursor", 20, Control.show_cursor(False)),
    ("ansi_bold_red", 20, _ansi("\x1b[1;31mhi\x1b[0m")),
    ("ansi_8bit", 20, _ansi("\x1b[38;5;214mx\x1b[0m")),
    ("ansi_truecolor", 20, _ansi("\x1b[38;2;255;136;0mx\x1b[0m")),
    ("ansi_attrs", 20, _ansi("\x1b[3;4;9mstyled\x1b[0m")),
    ("styled_on_red", 20, Styled(Text("hi"), "on red")),
    ("styled_panel", 8, Styled(Panel("x", box=box.SQUARE), "green")),
    ("progress_three", 50, _progress_table()),
]

# (name, width, height, layout) — layouts need an explicit console height.
LAYOUT_CASES = [
    ("layout_column", 24, 4, _layout_column()),
    ("layout_row", 24, 4, _layout_row()),
    ("layout_nested", 20, 4, _layout_nested()),
    ("layout_panel", 12, 4, _layout_panel()),
    ("layout_row_panels", 20, 5, _layout_row_panels()),
]

LAYOUT_HEADER = """\
# Golden parity fixtures for LAYOUTS — captured from real Python `rich`.
# Regenerate with: python scripts/capture_golden.py
#
# Format: <name>\\t<width>\\t<height>\\t<expected-ansi>
# Console: force_terminal=True, color_system="truecolor", highlight=False,
#          safe_box=False, legacy_windows=False, and an explicit height.
"""

RENDERABLE_HEADER = """\
# Golden parity fixtures for RENDERABLES — captured from real Python `rich`.
# Regenerate with: python scripts/capture_golden.py
#
# Format: <name>\\t<width>\\t<expected-ansi>
# `\\x1b` = ESC byte, `\\n` = newline (both unescaped by the test).
# Console: force_terminal=True, color_system="truecolor", highlight=False,
#          safe_box=False, legacy_windows=False.
# The Rust test builds the renderable matching each <name>; keep them in sync.
"""

HEADER = """\
# Golden parity fixtures — captured from the real Python `rich` library.
# Regenerate with: python scripts/capture_golden.py  (see AGENTS.md → Parity)
#
# Format: <name>\\t<console-markup>\\t<expected-ansi>
# `\\x1b` in the expected column is the ESC byte (unescaped by the test).
# Console: force_terminal=True, color_system="truecolor", highlight=False.
#
# Only cases whose output is byte-identical to upstream belong here. Our theme
# conveniences ([error]/[warning]/[info]) are NOT upstream and live in unit tests.
"""


def escape(text: str) -> str:
    """Render ESC and newline as the literal markers the Rust test unescapes."""
    return text.replace("\x1b", "\\x1b").replace("\n", "\\n")


def golden_dir() -> pathlib.Path:
    return (
        pathlib.Path(__file__).resolve().parent.parent
        / "crates"
        / "rich"
        / "tests"
        / "golden"
    )


def main() -> None:
    # highlight=False so no ReprHighlighter styling leaks in — the Rust core
    # ships no default highlighter.
    console = Console(
        force_terminal=True,
        color_system="truecolor",
        width=80,
        highlight=False,
        no_color=False,
    )
    markup_path = golden_dir() / "truecolor.tsv"
    lines = [HEADER.rstrip("\n")]
    for name, markup in CASES:
        with console.capture() as capture:
            console.print(markup, end="")
        lines.append(f"{name}\t{markup}\t{escape(capture.get())}")
    markup_path.write_text("\n".join(lines) + "\n", encoding="utf-8")
    print(f"wrote {len(CASES)} markup cases to {markup_path}")

    renderable_path = golden_dir() / "renderables.tsv"
    rlines = [RENDERABLE_HEADER.rstrip("\n")]
    for name, width, renderable in RENDERABLE_CASES:
        rconsole = Console(
            force_terminal=True,
            color_system="truecolor",
            width=width,
            highlight=False,
            safe_box=False,
            legacy_windows=False,
            no_color=False,
        )
        with rconsole.capture() as capture:
            rconsole.print(renderable)
        output = capture.get()
        # Guard: if the capture console's encoding isn't UTF-8, rich substitutes
        # box-drawing glyphs with ASCII, producing non-deterministic fixtures.
        # Fail loudly instead of writing a bad fixture.
        if name == "rule_plain" and "─" not in output:
            raise SystemExit(
                "box glyphs were ASCII-substituted — run with UTF-8 mode:\n"
                "    PYTHONUTF8=1 python scripts/capture_golden.py"
            )
        rlines.append(f"{name}\t{width}\t{escape(output)}")
    renderable_path.write_text("\n".join(rlines) + "\n", encoding="utf-8")
    print(f"wrote {len(RENDERABLE_CASES)} renderable cases to {renderable_path}")

    layout_path = golden_dir() / "layout.tsv"
    llines = [LAYOUT_HEADER.rstrip("\n")]
    for name, width, height, layout in LAYOUT_CASES:
        lconsole = Console(
            force_terminal=True,
            color_system="truecolor",
            width=width,
            height=height,
            highlight=False,
            safe_box=False,
            legacy_windows=False,
            no_color=False,
        )
        with lconsole.capture() as capture:
            lconsole.print(layout)
        llines.append(f"{name}\t{width}\t{height}\t{escape(capture.get())}")
    layout_path.write_text("\n".join(llines) + "\n", encoding="utf-8")
    print(f"wrote {len(LAYOUT_CASES)} layout cases to {layout_path}")


if __name__ == "__main__":
    main()
