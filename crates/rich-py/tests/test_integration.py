"""Console features wired at integration, byte for byte against rich 15.0.0.

``Console(highlighter=...)``, ``log`` of any renderable and ``log_locals``,
every ``print_json`` option, ``tab_size`` and ``emoji_variant``, the export
``code_format`` and ``font_aspect_ratio``, render hooks and the live stack
(so a ``Live`` redraws through the console and is recorded), ``Emoji``
printing without a newline, and ``set_window_title``. Each program runs
under ``rich`` and ``rs_rich``, as in ``test_compat.py``.
"""

from __future__ import annotations

import datetime
import importlib
import io
import re
from types import SimpleNamespace

import pytest

PACKAGES = ["rich", "rs_rich"]


def modules(package: str) -> SimpleNamespace:
    names = [
        "console", "text", "style", "panel", "table", "highlighter", "emoji", "live",
        "terminal_theme", "progress",
    ]
    return SimpleNamespace(**{name: importlib.import_module(f"{package}.{name}") for name in names})


def make(m, color=True, **options):
    options.setdefault("width", 50)
    return m.console.Console(
        file=io.StringIO(),
        force_terminal=color,
        color_system="truecolor" if color else None,
        **options,
    )


def compare(program, **options):
    outputs = [program(modules(package), **options) for package in PACKAGES]
    assert outputs[1] == outputs[0]


# ---------------------------------------------------------------------------
# Console(highlighter=...)


def highlighters(m):
    class Shout(m.highlighter.RegexHighlighter):
        base_style = "repr."
        highlights = [r"(?P<number>\d+)", r"(?P<str>'[a-z]+')"]

    out = []
    for highlighter in [Shout(), m.highlighter.NullHighlighter(), m.highlighter.ReprHighlighter()]:
        c = make(m, highlighter=highlighter, log_path=False)
        c.print("abc 123 'word' [bold]True[/] None", 42)
        c.print(m.panel.Panel("inside 7 'x'"))
        table = m.table.Table("h")
        table.add_row("cell 9")
        c.print(table)
        c.log("logged 5", log_locals=False)
        out.append(re.sub(r"\[\d\d:\d\d:\d\d\]", "[time]", c.file.getvalue()))
        out.append(repr(c.render_str("x 1 'y'").spans))
    return out


def test_console_highlighter():
    compare(highlighters)


# ---------------------------------------------------------------------------
# log


def logs(m):
    clock = lambda: datetime.datetime(2024, 1, 2, 3, 4, 5)  # noqa: E731
    c = make(m, width=70, get_datetime=clock, log_path=False)
    c.log(m.panel.Panel("a panel"), "and text")
    table = m.table.Table("x")
    table.add_row("1")
    c.log(table)
    c.log({"a": [1, 2, 3]})
    c.log("styled", style="bold", justify="right")
    log_with_locals(c)
    return c.file.getvalue()


def log_with_locals(c):
    answer = 42  # noqa: F841
    names = ["value", 1.5, None]  # noqa: F841
    c.log("with locals", log_locals=True)


def test_log_renderables_and_locals():
    compare(logs)


# ---------------------------------------------------------------------------
# print_json


def jsons(m):
    c = make(m, width=40)
    data = {"b": [1, 2.5, None, True], "a": "é", "n": {"x": {}}}
    c.print_json(data=data)
    c.print_json(data=data, indent=None)
    c.print_json(data=data, indent=4, sort_keys=True)
    c.print_json(data=data, indent="\t")
    c.print_json(data=data, ensure_ascii=True, highlight=False)
    c.print_json('{"z": 1, "y": [1, 2]}', sort_keys=True)
    c.print_json(data={1: 2, (1, 2): 3}, skip_keys=True)
    c.print_json(data={"when": datetime.date(2024, 1, 2)}, default=str)
    return c.file.getvalue()


def test_print_json_options():
    compare(jsons)


# ---------------------------------------------------------------------------
# tab_size and emoji_variant


def tabs_and_variants(m):
    out = []
    for options in [{"tab_size": 4}, {"tab_size": 2}, {"emoji_variant": "text"}, {"emoji_variant": "emoji"}]:
        c = make(m, **options)
        c.print("a\tb\tc :heart: [bold]:heart:[/]")
        c.print(m.panel.Panel("x\ty :heart:"))
        out.append(c.file.getvalue())
        out.append(repr(c.render_str(":heart: x").plain))
    return out


def test_tab_size_and_emoji_variant():
    compare(tabs_and_variants)


# ---------------------------------------------------------------------------
# Exports


def exports(m):
    c = make(m, record=True, width=30)
    c.print("[bold red]export[/] me")
    html = c.export_html(clear=False, code_format="<pre>{code}</pre><style>{stylesheet}</style>")
    inline = c.export_html(clear=False, inline_styles=True, code_format="[{code}]")
    svg = c.export_svg(clear=False, unique_id="x", font_aspect_ratio=0.5)
    custom = c.export_svg(unique_id="x", code_format="{chrome}|{lines}")
    return [html, inline, svg, custom]


def test_export_formats():
    compare(exports)


def test_a_bad_code_format_is_a_key_error():
    for package in PACKAGES:
        c = modules(package).console.Console(file=io.StringIO(), record=True)
        c.print("x")
        with pytest.raises(KeyError):
            c.export_html(code_format="{nope}")


# ---------------------------------------------------------------------------
# Emoji


def emojis(m):
    c = make(m)
    c.print(m.emoji.Emoji("rocket"))
    c.print(m.emoji.Emoji("smiley", style="on blue"))
    c.print("text")
    c.print(m.emoji.Emoji("heart"), "joined")
    return [c.file.getvalue(), repr(list(c.render(m.emoji.Emoji("x"))))]


def test_emoji_prints_without_a_newline():
    compare(emojis)


# ---------------------------------------------------------------------------
# Render hooks, the live stack and window titles


def hooks(m):
    class Around:
        def process_renderables(self, renderables):
            return ["[bold]before[/]", *renderables, m.panel.Panel("after")]

    class Drop:
        def process_renderables(self, renderables):
            return renderables[1:]

    c = make(m, width=30)
    c.push_render_hook(Around())
    c.print("one", m.panel.Panel("two"))
    c.rule("rule")
    c.push_render_hook(Drop())
    c.print("dropped", m.panel.Panel("kept"))
    c.pop_render_hook()
    c.pop_render_hook()
    c.print("plain")
    live = object()
    stack = [c.set_live(live), c.set_live(live), len(c._live_stack)]
    c.clear_live()
    c.clear_live()
    stack.append(len(c._live_stack))
    titled = [c.set_window_title("title"), make(m, color=False).set_window_title("t")]
    return [c.file.getvalue(), stack, titled]


def test_render_hooks_and_the_live_stack():
    compare(hooks)


def recorded_live(m):
    c = make(m, width=30, record=True)
    with m.live.Live("[bold]live", console=c, auto_refresh=False) as live:
        c.print("printed while live")
        live.update("updated", refresh=True)
        c.print(m.panel.Panel("panel"))
    return [c.file.getvalue(), c.export_text(styles=True)]


def test_a_live_display_is_recorded_like_any_print():
    compare(recorded_live)


def recorded_progress(m):
    c = make(m, width=40, record=True)
    clock = iter(float(n) for n in range(100))
    progress = m.progress.Progress(
        m.progress.TextColumn("{task.description}"),
        m.progress.BarColumn(bar_width=10),
        console=c,
        auto_refresh=False,
        get_time=lambda: next(clock),
    )
    with progress:
        task = progress.add_task("work", total=4)
        progress.console.print("log line")
        progress.advance(task, 2)
        progress.refresh()
    return [c.file.getvalue(), c.export_text()]


def test_progress_redraws_are_recorded():
    compare(recorded_progress)


# ---------------------------------------------------------------------------
# Rule(end=...), Table and Panel options


def rules(m):
    rule = importlib.import_module(f"{m.console.__name__.split('.')[0]}.rule").Rule
    c = make(m, width=12)
    c.print(rule("x", end=""))
    c.print("after")
    c.print(rule(end="!"))
    c.print(rule("t", end="!"))
    return [c.file.getvalue(), repr(list(c.render(rule("a", end="")))), repr(list(c.render(rule(end=""))))]


def test_rule_end():
    compare(rules)


def tables_and_panels(m):
    c = make(m, width=40)
    table = m.table.Table("a", "b", padding=(0, 2), collapse_padding=True, pad_edge=False,
                          style="blue", highlight=True, safe_box=True)
    table.add_row("1 'x'", m.panel.Panel("p 2"))
    c.print(table)
    grid = m.table.Table.grid("x", "y", padding=(0, 1))
    grid.add_row("left 1", "right 2")
    c.print(grid)
    grid = m.table.Table.grid(expand=True)
    grid.add_column(justify="right")
    grid.add_column(vertical="middle")
    grid.add_row("a\nb\nc", "mid")
    c.print(grid)
    table = m.table.Table("h")
    table.add_column("v", vertical="bottom", highlight=True)
    table.add_row("1\n2\n3", "4 5")
    align = importlib.import_module(f"{m.console.__name__.split('.')[0]}.align").Align
    table.add_row(align("mid", vertical="middle"), "a\nb\nc")
    c.print(table)
    title = m.text.Text("T", style="red")
    c.print(m.panel.Panel("x 1 'a'", style="on blue", height=5, highlight=True, title=title, subtitle="s"))
    c.print(m.panel.Panel.fit("fit 2", style="italic", border_style="green", height=3, safe_box=True))
    return c.file.getvalue()


def test_table_and_panel_options():
    compare(tables_and_panels)


def test_table_options_core_lacks_are_refused():
    from rs_rich.table import Table

    for options in [{"width": 30}, {"show_footer": True}, {"leading": 1}, {"row_styles": ["dim"]},
                    {"title_justify": "left"}, {"header_style": "red"}]:
        with pytest.raises(NotImplementedError):
            Table("a", **options)
    table = Table("a")
    with pytest.raises(NotImplementedError):
        table.add_row("x", end_section=True)
    with pytest.raises(NotImplementedError):
        table.add_section()
