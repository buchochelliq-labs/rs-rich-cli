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

import json
import os
import pathlib
import subprocess
import sys

from rich import box
from rich.align import Align
from rich.ansi import AnsiDecoder
from rich.bar import Bar as HBar
from rich.columns import Columns
from rich.console import Console
from rich.errors import MarkupError
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
from rich.prompt import Confirm as RichConfirm
from rich.prompt import FloatPrompt as RichFloatPrompt
from rich.prompt import IntPrompt as RichIntPrompt
from rich.prompt import Prompt as RichPrompt
from rich.text import Text
from rich.theme import Theme as RichTheme
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


def _pad_edge_table() -> Table:
    table = Table(box=box.SQUARE, pad_edge=False)
    table.add_column("Name")
    table.add_column("Age")
    table.add_row("Alice", "30")
    table.add_row("Bob", "7")
    return table


def _no_edge_table() -> Table:
    table = Table(box=box.SQUARE, show_edge=False)
    table.add_column("Name")
    table.add_column("Age")
    table.add_row("Alice", "30")
    table.add_row("Bob", "7")
    return table


def _collapse_table() -> Table:
    table = Table(box=box.SQUARE, collapse_padding=True)
    table.add_column("Name")
    table.add_column("Age")
    table.add_row("Alice", "30")
    table.add_row("Bob", "7")
    return table


def _table_style() -> Table:
    table = Table(box=box.SQUARE, style="blue")
    table.add_column("Name")
    table.add_column("Age")
    table.add_row("Alice", "30")
    table.add_row("Bob", "7")
    return table


def _csv_table() -> Table:
    # Reproduces rich-cli's render_csv styling: HEAVY_HEAD, blue border, and a
    # numeric column (Age) right-justified with a bold-green body + header.
    table = Table(show_header=True, box=box.HEAVY_HEAD, border_style="blue")
    table.add_column("Name")
    table.add_column("Age")
    table.columns[-1].justify = "right"
    table.columns[-1].style = "bold green"
    table.columns[-1].header_style = "bold green"
    table.add_column("City")
    table.add_row("Alice", "30", "NYC")
    table.add_row("Bob", "25", "LA")
    return table


def _table_ratio() -> Table:
    table = Table(box=box.SQUARE, expand=True)
    table.add_column("A", ratio=1)
    table.add_column("B", ratio=2)
    table.add_row("x", "y")
    return table


def _table_min_width() -> Table:
    table = Table(box=box.SQUARE)
    table.add_column("A", min_width=10)
    table.add_row("hi")
    return table


def _table_max_width() -> Table:
    table = Table(box=box.SQUARE)
    table.add_column("A", max_width=5)
    table.add_row("a very long cell value here")
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
    # Two tags over the *exact same* range — the inner one must win. Sorting the
    # spans by (start, end desc) makes these compare equal, leaves the innermost
    # first, and hands the win to the outer tag. Only an identical range shows
    # it; `nested_inner_wins` above has differing ends and passes either way.
    ("coincident_inner_wins", "[red][blue]x[/][/]"),
    ("coincident_inner_wins_swapped", "[blue][red]x[/][/]"),
    ("coincident_attrs_merge", "[bold][dim]y[/][/]"),
    # Tag names are normalized (`Style.normalize`) before they are stored or
    # matched, so an abbreviation opens what its long form closes, and a
    # mixed-case name reaches the theme lowercased.
    ("abbrev_tag", "[b]x[/]"),
    ("abbrev_open_long_close", "[b]x[/bold]"),
    ("long_open_abbrev_close", "[bold]x[/b]"),
    ("normalized_multi_word", "[dim i]x[/]"),
    ("mixed_case_unknown_is_noop", "[nOpE]x[/]"),
    ("not_attribute", "[not bold]x[/]"),
    # An uppercase tag is not a tag at all (RE_TAGS is `[a-z#/@]`), so this is
    # literal text followed by a close with nothing open.
    ("uppercase_is_not_a_tag", "[BOLD]x"),
    # Colour names normalize to lower case, so a mixed-case open matches a
    # lower-case close and `Style::definition` round-trips.
    ("mixed_case_hex_colour", "[on #FF0000]x[/on #ff0000]"),
    ("mixed_case_named_colour", "[RED]x"),
    # A bare `link` is a syntax error, so this tag resolves to nothing rather
    # than emitting a hyperlink to nowhere.
    ("bare_link_is_not_a_style", "[link]y[/]"),
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
    # Decomposed base+combining (U+0301) folds by grapheme without a grapheme
    # table — the combining marks are 0-width and stay with their base char.
    ("wrap_combining", 3, Text("".join(ch + "́" for ch in "abcdef"))),
    ("panel_wrap", 14, Panel("The quick brown fox", box=box.SQUARE)),
    ("panel_just_center", 14, Panel(Text("hi", justify="center"), box=box.SQUARE)),
    ("panel_just_right", 14, Panel(Text("hi", justify="right"), box=box.SQUARE)),
    ("panel_just_left", 14, Panel(Text("hi", justify="left"), box=box.SQUARE)),
    # A bare justified Text is shrunk to its content width (measurement-fit),
    # so it appears unpadded — the payoff of the Measurement port.
    ("text_justify_bare", 10, Text("hi", justify="center")),
    # Tabs must be expanded to 8-cell stops before anything measures or wraps.
    ("text_tabs", 40, Text("a\tb\tc")),
    ("text_tabs_multiline", 40, Text("ab\tc\nd\te")),
    # Control codes are stripped on construction (BEL, BS, VT, FF, CR) while
    # tab and newline survive.
    ("text_control_codes", 40, Text("a\rb\x07c\x08d")),
    ("table_square", 40, _table(box.SQUARE)),
    ("table_default", 40, _table(box.HEAVY_HEAD)),
    ("table_simple", 40, _table(box.SIMPLE)),
    ("table_double_edge", 40, _table(box.DOUBLE_EDGE)),
    ("table_shrink", 30, _shrink_table()),
    ("table_expand", 30, _expand_table()),
    ("table_justify", 30, _justify_table()),
    ("table_title", 30, _title_table()),
    ("table_lines", 30, _lines_table()),
    ("table_col_width", 40, _width_table()),
    ("table_col_style", 40, _style_table()),
    ("table_nowrap", 20, _nowrap_table()),
    ("table_pad_edge", 40, _pad_edge_table()),
    ("table_no_edge", 40, _no_edge_table()),
    ("table_collapse", 40, _collapse_table()),
    ("table_style", 40, _table_style()),
    ("table_csv_style", 40, _csv_table()),
    ("table_ratio", 30, _table_ratio()),
    ("table_min_width", 30, _table_min_width()),
    ("table_max_width", 40, _table_max_width()),
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
    # Non-ASCII strings: rich's JSON defaults to ensure_ascii=False, so accented
    # characters and symbols render as UTF-8 (not \uXXXX). Keys keep input order.
    ("json_unicode", 40, JSON('{"name": "café", "emoji": "❤"}')),
    ("markdown_doc", 24, Markdown("# Title\n\nHello **bold** and *italic* and `code`.")),
    ("markdown_list", 20, Markdown("Items:\n\n- one\n- two\n\n1. a\n2. b")),
    ("markdown_quote_hr", 20, Markdown("Note:\n\n> important\n\n---\n\ndone")),
    # A document ending with a thematic break emits one extra trailing blank line.
    ("markdown_hr_end", 20, Markdown("a\n\n---")),
    (
        "markdown_table",
        40,
        Markdown(
            "| Name | Age |\n| :--- | ---: |\n| Alice | 30 |\n| Bob | 7 |\n"
        ),
    ),
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

THEMES_HEADER = """\
# Golden parity fixtures for TERMINAL THEMES — captured from real Python `rich`.
# Regenerate with: python scripts/capture_golden.py
#
# Format: <name>\\t{"background": [r,g,b], "foreground": [r,g,b], "ansi": [[r,g,b] x16]}
#
# 18 hand-transcribed colour triplets per theme; capturing them means a typo
# fails the build instead of quietly shifting an exported palette.
"""

FUNCTIONS_HEADER = """\
# Golden parity fixtures for PURE FUNCTIONS — captured from real Python `rich`.
# Regenerate with: python scripts/capture_golden.py
#
# Format: <function>\\t<json-args>\\t<json-result>
#
# These underpin every renderable (cell widths decide all layout, ratio_resolve
# sizes every Layout split) but had no golden coverage — they were verified only
# against our own unit tests, which is how two Color::downgrade bugs survived.
"""

#: `(function, args)` pairs. Args are JSON-encodable and passed positionally.
#: Chosen to hit the awkward cases: double-width CJK, zero-width combining
#: marks, cropping that would split a wide glyph, and ratio edge cases.
FUNCTION_CASES: list[tuple[str, list]] = [
    # cell_len
    ("cell_len", [""]),
    ("cell_len", ["hello"]),
    ("cell_len", ["宽宽"]),
    ("cell_len", ["a宽b"]),
    ("cell_len", ["é"]),          # e + combining acute = 1 cell
    ("cell_len", ["abćdef"]),
    # set_cell_size — pad, exact, crop, and cropping onto a wide glyph
    ("set_cell_size", ["hi", 5]),
    ("set_cell_size", ["hello", 5]),
    ("set_cell_size", ["hello", 3]),
    ("set_cell_size", ["宽宽", 3]),
    ("set_cell_size", ["宽宽", 2]),
    ("set_cell_size", ["宽宽", 1]),
    ("set_cell_size", ["", 3]),
    ("set_cell_size", ["abc", 0]),
    # chop_cells — the over-long-word fold
    ("chop_cells", ["abcdefghij", 4]),
    ("chop_cells", ["宽宽宽宽", 3]),
    ("chop_cells", ["宽宽", 1]),
    # ratio_resolve — args are (total, [[size, ratio, minimum_size], ...])
    ("ratio_resolve", [10, [[None, 1, 1], [None, 1, 1]]]),
    ("ratio_resolve", [10, [[None, 1, 1], [None, 2, 1]]]),
    ("ratio_resolve", [10, [[3, 1, 1], [None, 1, 1]]]),
    ("ratio_resolve", [10, [[None, 1, 4], [None, 1, 4]]]),
    ("ratio_resolve", [3, [[None, 1, 2], [None, 1, 2]]]),
    ("ratio_resolve", [20, [[None, 1, 1], [None, 1, 1], [None, 1, 1]]]),
    ("ratio_resolve", [7, [[None, 3, 1], [None, 1, 1]]]),
]

COLORS_HEADER = """\
# Golden parity fixtures for COLOR SYSTEMS — captured from real Python `rich`.
# Regenerate with: python scripts/capture_golden.py
#
# Format: <name>\\t<color-system>\\t<console-markup>\\t<expected-ansi>
# Console: force_terminal=True, width=20, highlight=False, no_color=False, and
#          the <color-system> column as `color_system`.
#
# This is what pins `Color::downgrade`: the SAME markup is captured against
# truecolor, 256 and standard, so a truecolor value's fall-back to the 8-bit
# palette and then to the 16 ANSI colors is byte-checked against upstream
# rather than only against our own unit tests.
"""

# Markup chosen to exercise downgrade paths, not just to be colourful: a
# truecolor value has to fall back twice, an 8-bit index once, and a named
# standard colour not at all.
COLOR_CASES: list[tuple[str, str]] = [
    ("truecolor_fg", "[#ff8800]x[/]"),
    ("truecolor_bg", "[on #003366]x[/]"),
    ("truecolor_both", "[#00ff00 on #330000]x[/]"),
    ("truecolor_dark", "[#101010]x[/]"),
    ("truecolor_light", "[#f5f5f5]x[/]"),
    ("eight_bit_fg", "[color(214)]x[/]"),
    ("eight_bit_bg", "[on color(57)]x[/]"),
    ("eight_bit_grey", "[color(244)]x[/]"),
    ("standard_named", "[red]x[/]"),
    ("standard_bright", "[bright_cyan]x[/]"),
    ("standard_on", "[white on blue]x[/]"),
    ("attrs_with_color", "[bold underline #cc3366]x[/]"),
    ("default_color", "[default on default]x[/]"),
]

def _span_text(plain: str, style: str, start: int, end: int) -> Text:
    text = Text(plain)
    text.stylize(style, start, end)
    return text


# (name, width, Text, overflow, no_wrap) — the Rust test rebuilds each Text by
# name and renders it with the same overflow/no_wrap.
#
# "supercalifragilistic" is one unbreakable word, which is the only thing that
# tells `fold` apart from `crop`: a line of ordinary words wraps identically
# under every method, so a case built from those would pass against a stub.
OVERFLOW_CASES = [
    ("fold_long_word", 8, Text("supercalifragilistic"), "fold", False),
    ("crop_long_word", 8, Text("supercalifragilistic"), "crop", False),
    ("ellipsis_long_word", 8, Text("supercalifragilistic"), "ellipsis", False),
    ("ignore_long_word", 8, Text("supercalifragilistic"), "ignore", False),
    ("fold_sentence", 8, Text("the quick brown fox jumps"), "fold", False),
    ("crop_sentence", 8, Text("the quick brown fox jumps"), "crop", False),
    ("ellipsis_sentence", 8, Text("the quick brown fox jumps"), "ellipsis", False),
    ("ignore_sentence", 8, Text("the quick brown fox jumps"), "ignore", False),
    # no_wrap keeps each hard line whole, then the overflow method cuts it.
    ("nowrap_fold", 8, Text("the quick brown fox"), "fold", True),
    ("nowrap_crop", 8, Text("the quick brown fox"), "crop", True),
    ("nowrap_ellipsis", 8, Text("the quick brown fox"), "ellipsis", True),
    ("nowrap_ignore", 8, Text("the quick brown fox"), "ignore", True),
    # Hard newlines are still honoured when wrapping is off.
    ("nowrap_multiline", 8, Text("first line here\nsecond line here"), "ellipsis", True),
    # A double-width character straddling the cut is dropped whole and the
    # leftover cell padded with a space.
    ("wide_crop", 5, Text("aa你好世"), "crop", False),
    ("wide_ellipsis", 5, Text("aa你好世"), "ellipsis", False),
    ("wide_ellipsis_exact", 6, Text("aa你好世"), "ellipsis", False),
    # Which style the ellipsis inherits depends on where the cut lands relative
    # to a span: inside one, exactly on its start, and after it has ended.
    ("ellipsis_in_span", 4, _span_text("abcdefgh", "bold", 2, 5), "ellipsis", False),
    ("ellipsis_at_span_start", 5, _span_text("aaaabbbb", "bold red", 4, 8), "ellipsis", False),
    ("ellipsis_after_span", 5, _span_text("aaaabbbb", "bold red", 0, 4), "ellipsis", False),
    # Text that exactly fills the width must not be cut, and a width of 1 leaves
    # room for nothing but the marker itself.
    ("crop_exact_width", 5, Text("hello"), "crop", False),
    ("ellipsis_exact_width", 5, Text("hello"), "ellipsis", False),
    ("ellipsis_width_one", 1, Text("hello"), "ellipsis", False),
]

def _styled(plain: str, *spans) -> Text:
    text = Text(plain)
    for style, start, end in spans:
        text.stylize(style, start, end)
    return text


def _op_divide():
    return _styled("hello world", ("bold", 0, 5), ("red", 6, 11)).divide([5])


def _op_divide_span_across():
    return _styled("abcdefgh", ("bold", 2, 7)).divide([4])


def _op_split_newline():
    return _styled("one\ntwo\nthree", ("bold", 4, 7)).split("\n")


def _op_split_include():
    return _styled("one\ntwo\nthree", ("bold", 4, 7)).split("\n", include_separator=True)


def _op_split_trailing():
    return Text("one\ntwo\n").split("\n")


def _op_split_trailing_blank():
    return Text("one\ntwo\n").split("\n", allow_blank=True)


def _op_split_absent():
    return Text("no separator here").split("|")


def _op_split_word():
    return _styled("a-b-c", ("bold", 2, 3)).split("-")


def _mutated(text: Text, mutate) -> list:
    mutate(text)
    return [text]


def _op_pad():
    return _mutated(_styled("hi", ("bold", 0, 2)), lambda t: t.pad(3))


def _op_pad_left():
    return _mutated(_styled("hi", ("bold", 0, 2)), lambda t: t.pad_left(3, "."))


def _op_pad_right():
    return _mutated(_styled("hi", ("bold", 0, 2)), lambda t: t.pad_right(3, "."))


def _op_right_crop():
    return _mutated(_styled("hello world", ("bold", 3, 9)), lambda t: t.right_crop(4))


def _op_rstrip():
    return _mutated(_styled("hi there   ", ("bold", 0, 2)), lambda t: t.rstrip())


def _op_rstrip_end_partial():
    # 6 cells of trailing space but only 3 cells of excess: 3 must survive.
    return _mutated(Text("hello      "), lambda t: t.rstrip_end(8))


def _op_rstrip_end_noop():
    return _mutated(Text("hi   "), lambda t: t.rstrip_end(10))


def _op_expand_tabs():
    return _mutated(Text("a\tb\tc"), lambda t: t.expand_tabs(4))


def _op_expand_tabs_span_across_tabs():
    # A span crossing several tabs. Upstream splits it into one span per
    # tab-part, so this renders as three segments — remapping offsets instead
    # would keep one span, same colours but different bytes.
    return _mutated(_styled("a\tb\tc", ("bold", 0, 5)), lambda t: t.expand_tabs(4))


def _op_expand_tabs_styled():
    return _mutated(_styled("a\tb", ("bold", 0, 2)), lambda t: t.expand_tabs(8))


def _op_expand_tabs_multiline():
    return _mutated(Text("ab\tc\nd\te"), lambda t: t.expand_tabs(4))


def _op_join():
    return [Text(", ").join([Text("a"), _styled("b", ("bold", 0, 1)), Text("c")])]


def _op_join_styled_sep():
    return [_styled(" | ", ("red", 0, 3)).join([Text("x"), Text("y")])]


def _op_join_empty_sep():
    return [Text("").join([Text("a"), Text("b")])]


def _op_highlight_words():
    text = Text("the cat sat on the mat")
    text.highlight_words(["cat", "mat"], "bold red")
    return [text]


def _op_highlight_words_nocase():
    text = Text("Cat cat CAT")
    text.highlight_words(["cat"], "bold", case_sensitive=False)
    return [text]


def _op_highlight_regex():
    text = Text("abc 123 def 456")
    text.highlight_regex(r"\d+", "bold cyan")
    return [text]


# (name, factory) — the factory returns a list of Text, each rendered and joined
# with US (\x1f) so a multi-piece result stays one fixture row.
TEXT_OPS_CASES = [
    ("divide", _op_divide),
    ("divide_span_across", _op_divide_span_across),
    ("split_newline", _op_split_newline),
    ("split_include", _op_split_include),
    ("split_trailing", _op_split_trailing),
    ("split_trailing_blank", _op_split_trailing_blank),
    ("split_absent", _op_split_absent),
    ("split_word", _op_split_word),
    ("pad", _op_pad),
    ("pad_left", _op_pad_left),
    ("pad_right", _op_pad_right),
    ("right_crop", _op_right_crop),
    ("rstrip", _op_rstrip),
    ("rstrip_end_partial", _op_rstrip_end_partial),
    ("rstrip_end_noop", _op_rstrip_end_noop),
    ("expand_tabs", _op_expand_tabs),
    ("expand_tabs_span_across_tabs", _op_expand_tabs_span_across_tabs),
    ("expand_tabs_styled", _op_expand_tabs_styled),
    ("expand_tabs_multiline", _op_expand_tabs_multiline),
    ("join", _op_join),
    ("join_styled_sep", _op_join_styled_sep),
    ("join_empty_sep", _op_join_empty_sep),
    ("highlight_words", _op_highlight_words),
    ("highlight_words_nocase", _op_highlight_words_nocase),
    ("highlight_regex", _op_highlight_regex),
]

# (name, builder, default) — `builder(console)` returns a prompt object, and the
# fixture records `make_prompt(default)`. `...` means "no default", which is what
# upstream uses to distinguish it from a default of "" or 0.
PROMPT_CASES = [
    ("prompt_plain", lambda c: RichPrompt("Name", console=c), ...),
    ("prompt_default", lambda c: RichPrompt("Name", console=c), "World"),
    ("prompt_markup", lambda c: RichPrompt("[bold]Name[/]", console=c), ...),
    (
        "prompt_choices",
        lambda c: RichPrompt("Pick", console=c, choices=["a", "b"]),
        ...,
    ),
    (
        "prompt_choices_default",
        lambda c: RichPrompt("Pick", console=c, choices=["a", "b"]),
        "a",
    ),
    (
        "prompt_no_show_choices",
        lambda c: RichPrompt("Pick", console=c, choices=["a", "b"], show_choices=False),
        "a",
    ),
    (
        "prompt_no_show_default",
        lambda c: RichPrompt("Pick", console=c, choices=["a", "b"], show_default=False),
        "a",
    ),
    ("confirm_plain", lambda c: RichConfirm("Sure", console=c), ...),
    ("confirm_default_true", lambda c: RichConfirm("Sure", console=c), True),
    ("confirm_default_false", lambda c: RichConfirm("Sure", console=c), False),
    ("int_plain", lambda c: RichIntPrompt("Age", console=c), ...),
    ("int_default", lambda c: RichIntPrompt("Age", console=c), 42),
    ("float_default", lambda c: RichFloatPrompt("Ratio", console=c), 1.5),
]

# (name, theme-overrides, input) — rendered with `highlight=True` so the built-in
# ReprHighlighter runs, then the span style *names* resolve against the theme.
#
# This is the only fixture set in the corpus with highlighting on: every other
# one pins `highlight=False`, which is why the theme-resolution path had no net
# under it. A port that resolves highlighter styles against a global default
# instead of the console's theme passes every other fixture and fails these.
HIGHLIGHT_CASES = [
    ("default_number", {}, "n = 42"),
    ("themed_number", {"repr.number": "bold red"}, "n = 42"),
    ("themed_bool_none", {"repr.bool_true": "bold magenta", "repr.none": "dim"}, "True and None"),
    ("themed_str", {"repr.str": "italic yellow"}, "greeting = 'hello'"),
    # A name the theme does not define falls back to the default table.
    ("partial_override", {"repr.number": "underline"}, "call(1, 'two', True)"),
    # Markup and highlighting in the same string, both theme-resolved.
    ("markup_and_highlight", {"accent": "bold blue", "repr.number": "green"}, "[accent]total[/] = 7"),
    # An unknown tag name is a no-op upstream, not an error.
    ("unknown_tag_is_noop", {}, "[nope]x[/]"),
    # An explicit tag must BEAT the highlighter where they overlap. The
    # highlighter runs on the markup-stripped text and its spans go on first;
    # decorating the markup `Text` in place inverts that, which is the obvious
    # way to write it and produces the wrong colour on all four of these.
    ("markup_beats_highlight_colour", {}, "[green]123[/]"),
    ("markup_beats_highlight_bool", {}, "[red]True[/]"),
    ("markup_beats_highlight_str", {}, "[bold]'hi'[/]"),
    ("markup_over_highlight_inline", {}, "x = [magenta]99[/] ok"),
    # Attributes that do not collide merge instead of replacing.
    ("markup_merges_with_highlight", {}, "[underline]3.14[/]"),
]

# Markup edge cases, chosen to pin the three places a hand-rolled scanner
# diverges from upstream's RE_TAGS: backslash-run parity, `[` inside a tag body,
# and zero-length spans.
#
# Unlike CASES these may legitimately RAISE, so the fixture records `<ERROR>` and
# the Rust test asserts that too — for half of these, *which side errors* is the
# entire point.
MARKUP_EDGE_CASES = [
    # Backslash runs: only an ODD run escapes the tag. An even run emits half as
    # many literal backslashes and the tag still fires.
    ("bs1_escapes", r"\[b]x"),
    ("bs2_tag_fires", r"\\[b]x[/b]"),
    ("bs3_escapes", r"\\\[b]x"),
    ("bs4_tag_fires", r"\\\\[b]x[/b]"),
    ("bs5_escapes", r"\\\\\[b]x"),
    ("bs2_midtext", r"a\\[red]b[/red]c"),
    ("bs2_before_wide", "\\\\[b]\u4f60[/b]"),
    # A `[` inside a tag body means it is not a tag at all: the text is literal
    # and scanning resumes at the inner bracket.
    ("bracket_in_body", "[a[b]"),
    ("bracket_in_body_literal", "[bold[]x"),
    ("bracket_splits_tag", "[b[i]x[/i]"),
    ("bracket_in_close", "[b]x[/i[]"),
    ("bracket_nested_literal", "[b][c[]d[/b]"),
    ("bracket_breaks_link", "[link=a[b]c[/link]"),
    # Zero-length spans still contribute a segment boundary.
    ("empty_span_splits", "[b]a[i][/i]b[/b]"),
    ("empty_span_only", "[b][/b]x"),
    ("empty_nested", "[b][i][/i][/b]"),
    # The tag-start class, and other things that only look like tags.
    ("upper_not_tag", "[Hello]x"),
    ("digit_not_tag", "[42]x"),
    ("hex_colour", "[#ff0000]x[/]"),
    ("meta_tag", "[@meta]x[/]"),
    ("close_nothing_open", "[/]x"),
    ("unclosed_open", "[b]x"),
    ("close_unopened", "x[/b]"),
    ("empty_brackets", "[]x"),
    ("space_brackets", "[ ]x"),
    ("trailing_space_tag", "[b ]x[/]"),
    ("leading_space_not_tag", "[ b]x"),
    ("double_open_literal", "a[[b]c"),
    ("double_open_in_span", "[b]a[[/b]"),
    ("lone_escape", "\\["),
    ("lone_backslashes", "\\\\"),
    ("escaped_close", "[b]\\[/b]"),
    ("nested_same_range", "[red][blue]x[/][/]"),
    ("wide_chars", "[b]\u4f60\u597d[/]"),
]

COLOR_SYSTEMS = ["truecolor", "256", "standard"]

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


OVERFLOW_HEADER = """\
# Golden parity fixtures for Text overflow — captured from real Python `rich`.
# Regenerate with: python scripts/capture_golden.py  (see AGENTS.md → Parity)
#
# Format: <name>\\t<width>\\t<overflow>\\t<no_wrap>\\t<expected-ansi>
#
# Captured via `Console.print(text, overflow=..., no_wrap=...)`, so these cover
# the whole pipeline: wrap (folding only under "fold"), justify, per-line
# truncate, and the console-level crop that "ignore" relies on.
"""


HIGHLIGHT_HEADER = """\
# Golden parity fixtures for theme-resolved highlighting — captured from real
# Python `rich`. Regenerate with: python scripts/capture_golden.py
#
# Format: <name>\\t<theme-overrides-json>\\t<input>\\t<expected-ansi>
#
# Console: highlight=True (the ONLY fixture set with it on), width 80, truecolor.
# The theme column is applied over the default theme before rendering, so these
# pin that highlighter and markup styles resolve against the *console's* theme
# rather than a process-global default.
"""

MARKUP_EDGE_HEADER = """\
# Golden parity fixtures for markup edge cases — captured from real Python `rich`.
# Regenerate with: python scripts/capture_golden.py  (see AGENTS.md → Parity)
#
# Format: <name>\\t<markup>\\t<expected-ansi, or <ERROR>>
#
# `<ERROR>` means upstream raises MarkupError. Which side errors is often the
# whole point of the case, so the test asserts that too.
"""

PROMPT_HEADER = """\
# Golden parity fixtures for prompts — captured from real Python `rich`.
# Regenerate with: python scripts/capture_golden.py  (see AGENTS.md → Parity)
#
# Format: <name>\\t<expected-ansi>
#
# Each row is `make_prompt(default)` rendered at width 80 — the question line as
# the user sees it, including the `prompt.choices` and `prompt.default` styles.
# Reading the answer is not captured here; that half is covered by unit tests
# driving a scripted input source.
"""

TEXT_OPS_HEADER = """\
# Golden parity fixtures for Text manipulation — captured from real Python `rich`.
# Regenerate with: python scripts/capture_golden.py  (see AGENTS.md → Parity)
#
# Format: <name>\\t<expected-ansi>
#
# Each case renders its result at width 80 (so nothing wraps) — the ANSI output
# pins the plain text, the span boundaries and the styles in one go. Ops that
# return several pieces (split/divide) join them with \\x1f.
"""


def escape(text: str) -> str:
    """Render ESC and newline as the literal markers the Rust test unescapes."""
    return (
        text.replace("\x1b", "\\x1b").replace("\n", "\\n").replace("\x1f", "\\x1f")
    )


#: Worker run in a fresh interpreter, one per colour system. Reads
#: `[[name, markup], ...]` as JSON on stdin, writes `[[name, markup, out], ...]`.
_COLOR_WORKER = r"""
import json, sys
from rich.console import Console

system = sys.argv[1]
cases = json.load(sys.stdin)
out = []
for name, markup in cases:
    console = Console(force_terminal=True, color_system=system, width=20,
                      highlight=False, no_color=False)
    with console.capture() as capture:
        console.print(markup, end="")
    out.append([name, markup, capture.get()])
json.dump(out, sys.stdout)
"""


def _capture_colors_isolated(system: str) -> list[tuple[str, str, str]]:
    """Capture every colour case for `system` in a dedicated interpreter.

    See the call site: rich memoises rendered SGR codes on `Style`, so mixing
    colour systems in one process produces wrong fixtures.
    """
    proc = subprocess.run(
        [sys.executable, "-c", _COLOR_WORKER, system],
        input=json.dumps([[n, m] for n, m in COLOR_CASES]),
        capture_output=True,
        text=True,
        encoding="utf-8",
        env={**os.environ, "PYTHONUTF8": "1"},
    )
    if proc.returncode != 0:
        raise SystemExit(
            f"colour capture for {system!r} failed:\n{proc.stderr.strip()}"
        )
    return [(n, m, o) for n, m, o in json.loads(proc.stdout)]


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
    markup_path.write_text("\n".join(lines) + "\n", encoding="utf-8", newline="\n")
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
    renderable_path.write_text("\n".join(rlines) + "\n", encoding="utf-8", newline="\n")
    print(f"wrote {len(RENDERABLE_CASES)} renderable cases to {renderable_path}")

    highlight_path = golden_dir() / "highlight.tsv"
    hlines = [HIGHLIGHT_HEADER.rstrip("\n")]
    for name, overrides, source in HIGHLIGHT_CASES:
        hconsole = Console(
            force_terminal=True,
            color_system="truecolor",
            width=80,
            highlight=True,
            safe_box=False,
            legacy_windows=False,
            no_color=False,
            theme=RichTheme(overrides) if overrides else None,
        )
        with hconsole.capture() as capture:
            hconsole.print(source, end="")
        hlines.append(
            f"{name}\t{json.dumps(overrides, sort_keys=True)}\t{source}\t{escape(capture.get())}"
        )
    highlight_path.write_text("\n".join(hlines) + "\n", encoding="utf-8", newline="\n")
    print(f"wrote {len(HIGHLIGHT_CASES)} highlight cases to {highlight_path}")

    edge_path = golden_dir() / "markup_edge.tsv"
    elines = [MARKUP_EDGE_HEADER.rstrip("\n")]
    for name, markup in MARKUP_EDGE_CASES:
        econsole = Console(
            force_terminal=True,
            color_system="truecolor",
            width=80,
            highlight=False,
            safe_box=False,
            legacy_windows=False,
            no_color=False,
        )
        try:
            with econsole.capture() as capture:
                econsole.print(markup, end="")
            expected = escape(capture.get())
        except MarkupError:
            expected = "<ERROR>"
        elines.append(f"{name}\t{escape(markup)}\t{expected}")
    edge_path.write_text("\n".join(elines) + "\n", encoding="utf-8", newline="\n")
    print(f"wrote {len(MARKUP_EDGE_CASES)} markup edge cases to {edge_path}")

    prompt_path = golden_dir() / "prompts.tsv"
    plines = [PROMPT_HEADER.rstrip("\n")]
    for name, builder, default in PROMPT_CASES:
        pconsole = Console(
            force_terminal=True,
            color_system="truecolor",
            width=80,
            highlight=False,
            safe_box=False,
            legacy_windows=False,
            no_color=False,
        )
        rendered = builder(pconsole).make_prompt(default)
        with pconsole.capture() as capture:
            pconsole.print(rendered, end="")
        plines.append(f"{name}\t{escape(capture.get())}")
    prompt_path.write_text("\n".join(plines) + "\n", encoding="utf-8", newline="\n")
    print(f"wrote {len(PROMPT_CASES)} prompt cases to {prompt_path}")

    ops_path = golden_dir() / "text_ops.tsv"
    oplines = [TEXT_OPS_HEADER.rstrip("\n")]
    for name, factory in TEXT_OPS_CASES:
        pieces = []
        for piece in factory():
            opconsole = Console(
                force_terminal=True,
                color_system="truecolor",
                width=80,
                highlight=False,
                safe_box=False,
                legacy_windows=False,
                no_color=False,
            )
            with opconsole.capture() as capture:
                opconsole.print(piece, end="")
            pieces.append(capture.get())
        oplines.append(f"{name}\t{escape(chr(31).join(pieces))}")
    ops_path.write_text("\n".join(oplines) + "\n", encoding="utf-8", newline="\n")
    print(f"wrote {len(TEXT_OPS_CASES)} text-op cases to {ops_path}")

    overflow_path = golden_dir() / "overflow.tsv"
    olines = [OVERFLOW_HEADER.rstrip("\n")]
    for name, width, text, overflow, no_wrap in OVERFLOW_CASES:
        oconsole = Console(
            force_terminal=True,
            color_system="truecolor",
            width=width,
            highlight=False,
            safe_box=False,
            legacy_windows=False,
            no_color=False,
        )
        with oconsole.capture() as capture:
            oconsole.print(text, overflow=overflow, no_wrap=no_wrap)
        olines.append(
            f"{name}\t{width}\t{overflow}\t{str(no_wrap).lower()}\t{escape(capture.get())}"
        )
    overflow_path.write_text("\n".join(olines) + "\n", encoding="utf-8", newline="\n")
    print(f"wrote {len(OVERFLOW_CASES)} overflow cases to {overflow_path}")

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
    layout_path.write_text("\n".join(llines) + "\n", encoding="utf-8", newline="\n")
    print(f"wrote {len(LAYOUT_CASES)} layout cases to {layout_path}")

    # --- colour systems -------------------------------------------------
    # The same markup through each system, so `Color::downgrade` is pinned to
    # upstream's exact fall-back rather than just to our own unit tests.
    colors_path = golden_dir() / "colors.tsv"
    clines = [COLORS_HEADER.rstrip("\n")]
    for system in COLOR_SYSTEMS:
        # Each system MUST be captured in a fresh interpreter. `Style.parse` is
        # lru_cached and `Style._ansi` memoises the rendered SGR codes on the
        # (shared) Style object, so whichever colour system renders a given
        # style first wins for the rest of the process — capturing all three
        # in-process silently yields three copies of the first system's output.
        for name, markup, output in _capture_colors_isolated(system):
            clines.append(f"{name}\t{system}\t{markup}\t{escape(output)}")
    colors_path.write_text("\n".join(clines) + "\n", encoding="utf-8", newline="\n")
    print(
        f"wrote {len(COLOR_CASES) * len(COLOR_SYSTEMS)} colour cases "
        f"({len(COLOR_SYSTEMS)} systems) to {colors_path}"
    )

    # --- pure functions -------------------------------------------------
    from dataclasses import dataclass

    from rich._ratio import ratio_resolve
    from rich.cells import cell_len, chop_cells, set_cell_size

    @dataclass
    class _Edge:
        """Matches `rich._ratio`'s Edge protocol (size / ratio / minimum_size)."""

        size: "int | None"
        ratio: int
        minimum_size: int

    def call(fn: str, args: list):
        if fn == "cell_len":
            return cell_len(*args)
        if fn == "set_cell_size":
            return set_cell_size(*args)
        if fn == "chop_cells":
            return list(chop_cells(*args))
        if fn == "ratio_resolve":
            total, raw_edges = args
            return ratio_resolve(total, [_Edge(*edge) for edge in raw_edges])
        raise SystemExit(f"no capture wired for function {fn!r}")

    functions_path = golden_dir() / "functions.tsv"
    flines = [FUNCTIONS_HEADER.rstrip("\n")]
    for fn, args in FUNCTION_CASES:
        result = call(fn, args)
        flines.append(
            f"{fn}\t{json.dumps(args, ensure_ascii=False)}"
            f"\t{json.dumps(result, ensure_ascii=False)}"
        )
    functions_path.write_text("\n".join(flines) + "\n", encoding="utf-8", newline="\n")
    print(f"wrote {len(FUNCTION_CASES)} function cases to {functions_path}")

    # --- terminal themes -------------------------------------------------
    import rich.terminal_theme as _tt

    themes_path = golden_dir() / "themes.tsv"
    tlines = [THEMES_HEADER.rstrip("\n")]
    for theme_name in [
        "DEFAULT_TERMINAL_THEME",
        "MONOKAI",
        "DIMMED_MONOKAI",
        "NIGHT_OWLISH",
        "SVG_EXPORT_THEME",
    ]:
        theme = getattr(_tt, theme_name)
        payload = {
            "background": list(theme.background_color),
            "foreground": list(theme.foreground_color),
            "ansi": [list(c) for c in theme.ansi_colors._colors],
        }
        tlines.append(f"{theme_name}\t{json.dumps(payload)}")
    themes_path.write_text("\n".join(tlines) + "\n", encoding="utf-8", newline="\n")
    print(f"wrote {len(tlines) - 1} terminal themes to {themes_path}")

    # --- exports --------------------------------------------------------
    # These were previously pasted into Rust source as string literals, which
    # put them outside the CI drift check entirely. Capturing them here means
    # `git diff --exit-code` on the golden dir now covers exports too.
    def recording_console(width: int) -> Console:
        return Console(
            force_terminal=True,
            color_system="truecolor",
            width=width,
            highlight=False,
            record=True,
            no_color=False,
        )

    # Inputs below MUST match the corresponding Rust tests exactly, or the
    # fixtures are meaningless — including how many lines each prints, since
    # the two HTML tests deliberately differ.
    # console.rs::export_html_matches_upstream (width 20, TWO printed lines):
    html_inline_console = recording_console(20)
    html_inline_console.print("[bold red]hi[/] there")
    html_inline_console.print("plain line")
    (golden_dir() / "export_html.html").write_text(
        html_inline_console.export_html(clear=False, inline_styles=True),
        encoding="utf-8",
        newline="\n",
    )

    # console.rs::export_html_classes_matches_upstream (width 20, ONE line):
    html_classes_console = recording_console(20)
    html_classes_console.print("[bold red]hi[/] there")
    (golden_dir() / "export_html_classes.html").write_text(
        html_classes_console.export_html(clear=False),
        encoding="utf-8",
        newline="\n",
    )

    # SVG: svg.rs::export_svg_matches_upstream (width 10, one printed line).
    # A FIXED unique_id — upstream's default is adler32 over Python `repr()`
    # output, which Rust cannot reproduce (see docs/DIVERGENCES.md #15).
    svg_console = recording_console(10)
    svg_console.print("[bold red]Hi[/] ok")
    (golden_dir() / "svg_export.svg").write_text(
        svg_console.export_svg(title="X", unique_id="test", clear=False),
        encoding="utf-8",
        newline="\n",
    )
    print("wrote export fixtures (html inline, html classes, svg)")


if __name__ == "__main__":
    main()
