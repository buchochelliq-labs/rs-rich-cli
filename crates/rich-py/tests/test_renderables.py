"""The static renderables: Rule, Padding, Align, VerticalCenter, Columns,
Group, Constrain, Styled, Tree, Layout, Bar, Spinner and the containers.

Each program is written once against Rich's API and run with ``rich`` 15.0.0
and with ``rs_rich``; the outputs must be identical, in colour and without.
"""

from __future__ import annotations

import importlib
import io
from types import SimpleNamespace

import pytest

from rs_rich import _native

MODULES = [
    "console", "text", "style", "table", "panel", "box", "measure", "rule", "padding", "align",
    "columns", "constrain", "styled", "tree", "layout", "bar", "spinner", "containers",
]


def modules(package: str) -> SimpleNamespace:
    loaded = {name: importlib.import_module(f"{package}.{name}") for name in MODULES}
    namespace = SimpleNamespace(**loaded)
    # `rs_rich.console` re-exports `Group` and `group` once the foundation
    # adds them; until then they come from `_native`.
    namespace.Group = getattr(loaded["console"], "Group", None) or _native.Group
    namespace.group = getattr(loaded["console"], "group", None) or _native.group
    return namespace


def make_console(m, color: bool, **options):
    return m.console.Console(
        file=io.StringIO(),
        width=options.pop("width", 50),
        force_terminal=color,
        color_system="truecolor" if color else None,
        **options,
    )


def compare(program, color: bool = True, **options) -> str:
    outputs = []
    for package in ["rich", "rs_rich"]:
        m = modules(package)
        c = make_console(m, color, **dict(options))
        program(m, c)
        outputs.append(c.file.getvalue())
    expected, actual = outputs
    assert actual == expected
    return actual


# ---------------------------------------------------------------------------
# Programs, each printed by both libraries


def rules(m, c):
    c.print(m.rule.Rule())
    c.print(m.rule.Rule("[b]Title[/] 1"))
    c.print(m.rule.Rule("left", align="left", style="red"))
    c.print(m.rule.Rule(m.text.Text("text title", style="italic"), characters="=", align="right"))
    c.print(m.rule.Rule("multi", characters="-~"))
    c.print(m.rule.Rule("wide", characters="🎉"))
    c.print(m.rule.Rule("line\nbreak\ttab"))
    c.print(m.rule.Rule("a very long title " * 5))
    c.print(m.rule.Rule("x"), width=4)
    c.print(m.rule.Rule("styled", style=m.style.Style(color="magenta", bold=True)))
    c.print(m.rule.Rule("ends twice", end="\n\n"))
    c.print(m.panel.Panel(m.rule.Rule("in a panel")))


def paddings(m, c):
    c.print(m.padding.Padding("hi", 1))
    c.print(m.padding.Padding("styled", (1, 2), style="on blue"))
    c.print(m.padding.Padding("fits", (0, 1, 0, 2), expand=False, style="reverse"))
    c.print(m.padding.Padding.indent("indented by four", 4))
    c.print(m.padding.Padding(m.panel.Panel("boxed"), (1, 3)))
    c.print(m.padding.Padding("one", (2,)))
    table = m.table.Table("a", "b")
    table.add_row(m.padding.Padding("cell", (0, 2)), "x")
    c.print(table)
    c.print(m.panel.Panel.fit(m.padding.Padding("in fit", (1, 1))))


def aligns(m, c):
    for align in ["left", "center", "right"]:
        c.print(m.align.Align("text", align))
        c.print(m.align.Align(m.panel.Panel.fit("panel"), align, style="on red"))
    c.print(m.align.Align.center("no pad", pad=False))
    c.print(m.align.Align.right("in ten", width=10))
    c.print(m.align.Align.center("wrapped words here", width=8))
    for vertical in ["top", "middle", "bottom"]:
        c.print(m.align.Align.center("v", vertical=vertical, height=4, style="on blue"))
    c.print(m.align.Align.left("tall", vertical="middle"), height=3)
    c.print(m.align.Align.right("unpadded", vertical="bottom", height=3, pad=False))
    c.print(m.panel.Panel(m.align.Align.center("centred in a panel")))
    c.print(m.align.VerticalCenter("vc", style="on green"), height=5)
    table = m.table.Table("aligned")
    table.add_row(m.align.Align.right("r"))
    c.print(table)


def columns(m, c):
    words = "alpha beta gamma delta epsilon zeta eta theta iota kappa lambda mu".split()
    c.print(m.columns.Columns(words))
    c.print(m.columns.Columns(words, equal=True, expand=True))
    c.print(m.columns.Columns(words, column_first=True))
    c.print(m.columns.Columns(words, right_to_left=True, padding=(0, 3)))
    c.print(m.columns.Columns(words, align="center", equal=True))
    c.print(m.columns.Columns(words, width=12, align="right"))
    c.print(m.columns.Columns(words[:5], title="[b]Title[/]"))
    mixed = m.columns.Columns(
        [m.text.Text("text", style="red"), m.panel.Panel.fit("panel"), "[i]markup[/] 42"],
    )
    mixed.add_renderable(m.rule.Rule("r"))
    c.print(mixed)
    c.print(m.columns.Columns([]))
    c.print(m.columns.Columns(["one"] * 7, column_first=True, equal=True))
    c.print(m.panel.Panel(m.columns.Columns(words[:6])))


def groups(m, c):
    group = m.Group("first [b]line[/]", m.panel.Panel("in panel"), m.rule.Rule("rule"), "")
    c.print(group)
    c.print(m.panel.Panel.fit(m.Group("fit", "to the widest line")))
    c.print(m.panel.Panel.fit(m.Group("fill", fit=False)))
    c.print(m.panel.Panel(m.Group()))

    @m.group()
    def lines():
        yield "decorated"
        yield m.text.Text("group", style="bold")

    c.print(m.panel.Panel.fit(lines()))
    c.print(m.containers.Renderables(["a", m.text.Text("b", style="red")]))
    c.print(m.panel.Panel.fit(m.containers.Renderables(["one", "three"])))


def constrained_and_styled(m, c):
    c.print(m.constrain.Constrain(m.panel.Panel("constrained"), 14))
    c.print(m.constrain.Constrain(m.panel.Panel("unconstrained"), None))
    c.print(m.constrain.Constrain("wraps inside eight cells", 8))
    c.print(m.styled.Styled(m.panel.Panel("styled"), "on blue"))
    c.print(m.styled.Styled("[red]red[/] on green", m.style.Style(bgcolor="green")))
    table = m.table.Table("c")
    table.add_row(m.constrain.Constrain("constrained cell text", 6))
    c.print(table)


def trees(m, c):
    tree = m.tree.Tree("[b]root[/]")
    node = tree.add("child [i]one[/] 1")
    node.add("leaf")
    node.add(m.text.Text("text leaf", style="green"))
    heavy = tree.add("heavy", guide_style="bold red")
    heavy.add("a").add("b")
    heavy.add("c")
    double = tree.add("double", guide_style="underline2 blue", style="on grey23")
    double.add("x")
    collapsed = tree.add("collapsed", expanded=False)
    collapsed.add("hidden")
    tree.add(m.panel.Panel.fit("panel label"))
    tree.add("a long label that has to wrap onto another line to fit")
    c.print(tree)
    c.print(m.tree.Tree("hidden root", hide_root=True, highlight=True).add("shown 1").add("deeper 2"))
    hidden = m.tree.Tree("hidden root", hide_root=True)
    hidden.add("first").add("nested")
    hidden.add("second")
    c.print(hidden)
    c.print(m.tree.Tree("highlighted 42 'str'", highlight=True))
    c.print(m.panel.Panel.fit(tree, title="in a panel"))
    c.print(tree, justify="right")


def layouts(m, c):
    layout = m.layout.Layout()
    layout.split_column(
        m.layout.Layout(name="header", size=3),
        m.layout.Layout(name="main", ratio=1),
        m.layout.Layout(name="footer", size=3),
    )
    layout["main"].split_row(m.layout.Layout(name="side"), m.layout.Layout(name="body", ratio=2))
    layout["header"].update(m.panel.Panel("header"))
    layout["body"].update("body [b]text[/] that wraps across the region")
    c.print(layout)
    c.print(layout.tree)
    layout["side"].visible = False
    c.print(layout)
    c.print(layout.tree)
    layout["main"].unsplit()
    layout["main"].add_split(m.layout.Layout(name="extra", minimum_size=2), "plain")
    c.print(layout, height=8)


def narrow_layouts(m, c):
    layout = m.layout.Layout(name="header")
    layout.split_row(m.layout.Layout(name="a"), m.layout.Layout(name="bbbbbbbbbbbbb", size=8))
    c.print(layout, width=30)
    c.print(layout.tree, width=30)
    unnamed = m.layout.Layout(size=2, minimum_size=3, ratio=4)
    c.print(unnamed, width=12, height=6)
    c.print(m.layout.Layout(m.panel.Panel("content")), height=4)


def bars(m, c):
    c.print(m.bar.Bar(100, 0, 50))
    c.print(m.bar.Bar(100, 10.5, 60.25, width=20, color="red", bgcolor="blue"))
    c.print(m.bar.Bar(10, 5, 5, width=8))
    c.print(m.bar.Bar(10, -3, 20, width=9))
    for end in range(0, 9):
        c.print(m.bar.Bar(8, 1, end, width=1))
    table = m.table.Table("bar")
    table.add_row(m.bar.Bar(4, 1, 3))
    c.print(table)


def spinners(m, c):
    c.print(m.spinner.Spinner("dots").render(0.5))
    spinner = m.spinner.Spinner("line", "loading [b]data[/]", style="green")
    spinner.render(0.0)
    c.print(spinner.render(0.3))
    spinner.update(text="updated", speed=2.0)
    c.print(spinner.render(0.4))
    c.print(spinner.render(0.9))
    c.print(m.spinner.Spinner("arc", m.panel.Panel.fit("renderable")).render(0.2))
    c.print(m.spinner.Spinner("dots", m.text.Text("text", style="red")).render(1.0))


def measures(m, c):
    for renderable in [
        m.rule.Rule("x"),
        m.padding.Padding("abc", (0, 2)),
        m.align.Align.center("abcdef"),
        m.tree.Tree("root").add("a child"),
        m.bar.Bar(1, 0, 1),
        m.bar.Bar(1, 0, 1, width=7),
        m.Group("a", "bbb"),
        m.Group("a", fit=False),
        m.constrain.Constrain("a b c d e f g", 5),
        m.styled.Styled("styled", "red"),
        m.columns.Columns(["a", "b"]),
        m.containers.Renderables([]),
    ]:
        measured = c.measure(renderable)
        c.print(measured.minimum, measured.maximum)
    measured = m.measure.measure_renderables(c, c.options, ["a", "longer"])
    c.print(measured.minimum, measured.maximum)


PROGRAMS = [
    rules, paddings, aligns, columns, groups, constrained_and_styled, trees, layouts,
    narrow_layouts, bars, spinners, measures,
]


@pytest.mark.parametrize("color", [True, False], ids=["truecolor", "plain"])
@pytest.mark.parametrize("program", PROGRAMS, ids=lambda p: p.__name__)
def test_output_matches_rich_byte_for_byte(program, color):
    compare(program, color, height=12)


@pytest.mark.parametrize("encoding", ["utf-8", "ascii"])
def test_ascii_consoles_draw_ascii_guides_and_rules(encoding):
    def program(m, c):
        tree = m.tree.Tree("root")
        tree.add("a").add("b")
        tree.add("c")
        c.print(tree)
        c.print(m.rule.Rule("title"))
        c.print(m.rule.Rule())

    outputs = []
    for package in ["rich", "rs_rich"]:
        m = modules(package)

        class File(io.StringIO):
            pass

        File.encoding = encoding  # type: ignore[assignment]
        file = File()
        c = m.console.Console(file=file, width=30, color_system=None)
        program(m, c)
        outputs.append(file.getvalue())
    assert outputs[1] == outputs[0]


@pytest.mark.skipif(
    not hasattr(_native.Console(), "get_time"),
    reason="rs_rich.console.Console does not expose get_time yet",
)
def test_a_spinner_animates_from_the_consoles_clock():
    def program(m, c):
        clock = iter([0.0, 0.0, 0.25, 0.5, 1.0])
        timed = m.console.Console(
            file=c.file, width=20, color_system=None, get_time=lambda: next(clock)
        )
        spinner = m.spinner.Spinner("dots", "working")
        for _ in range(4):
            timed.print(spinner)

    compare(program)


def test_live_layout_regions_are_mapped():
    for package in ["rich", "rs_rich"]:
        m = modules(package)
        c = make_console(m, False, width=20, height=4)
        layout = m.layout.Layout()
        layout.split_row(m.layout.Layout("L", name="left"), m.layout.Layout("R", name="right", size=5))
        c.print(layout)
        regions = {key.name: tuple(value.region) for key, value in layout.map.items()}
        assert regions == {"left": (0, 0, 15, 4), "right": (15, 0, 5, 4)}
        rendered = layout.render(c, c.options)
        assert [len(render.render) for render in rendered.values()] == [4, 4]


# ---------------------------------------------------------------------------
# The Python API


def test_reprs_match_rich():
    def reprs(m):
        return [
            repr(m.rule.Rule("t", characters="=")),
            repr(m.padding.Padding("p", (1, 2))),
            repr(m.align.Align("a", "center")),
            repr(m.align.VerticalCenter("v")),
            repr(m.bar.Bar(10, -1, 20)),
            repr(m.bar.Bar(1.5, 0.25, 1)),
            repr(m.layout.Layout(name="x", size=3, ratio=2)),
            repr(m.layout.Layout()),
        ]

    assert reprs(modules("rs_rich")) == reprs(modules("rich"))


def test_attributes_match_rich():
    def attributes(m):
        padding = m.padding.Padding("p", (1, 2, 3, 4), expand=False)
        tree = m.tree.Tree("root", guide_style="red")
        child = tree.add("child", style="blue")
        bar = m.bar.Bar(10, -1, 20)
        layout = m.layout.Layout(name="root")
        layout.split_row(m.layout.Layout(name="a"), "text")
        return [
            (padding.top, padding.right, padding.bottom, padding.left, padding.expand),
            m.padding.Padding.unpack((1, 2)),
            m.padding.Padding.unpack(3),
            (child.style, child.guide_style, child.expanded, child.highlight, len(tree.children)),
            tree.children[0] is child,
            (bar.begin, bar.end, bar.size),
            (layout.splitter.name, [c.name for c in layout.children]),
            layout["a"].name,
            layout.get("missing"),
            m.Group("a", "b").renderables,
            m.spinner.Spinner("dots").interval,
            m.spinner.Spinner("line").frames,
            m.tree.Tree.ASCII_GUIDES,
            m.tree.Tree.TREE_GUIDES,
        ]

    assert attributes(modules("rs_rich")) == attributes(modules("rich"))


def test_spinners_table_matches_rich():
    from rich._spinners import SPINNERS

    from rs_rich.spinner import SPINNERS as ours

    assert list(ours) == list(SPINNERS)
    for name, data in SPINNERS.items():
        assert ours[name]["interval"] == data["interval"]
        assert ours[name]["frames"] == list(data["frames"]), name


@pytest.mark.parametrize(
    "make, error",
    [
        (lambda m: m.rule.Rule(characters=""), "ValueError"),
        (lambda m: m.rule.Rule(align="middle"), "ValueError"),
        (lambda m: m.align.Align("x", "middle"), "ValueError"),
        (lambda m: m.align.Align("x", vertical="centre"), "ValueError"),
        (lambda m: m.padding.Padding("x", (1, 2, 3)), "ValueError"),
        (lambda m: m.spinner.Spinner("no-such-spinner"), "KeyError"),
        (lambda m: m.layout.Layout().split("x", splitter="diagonal"), "NoSplitter"),
        (lambda m: m.layout.Layout()["missing"], "KeyError"),
    ],
)
def test_errors_match_rich(make, error):
    messages = []
    for package in ["rich", "rs_rich"]:
        m = modules(package)
        with pytest.raises(Exception) as raised:
            make(m)
        assert type(raised.value).__name__ == error
        messages.append(str(raised.value))
    assert messages[1] == messages[0]


def test_the_group_decorator_wraps_the_function():
    from rs_rich._native import Group, group

    @group(fit=False)
    def renderables():
        """Some renderables."""
        yield "a"

    made = renderables()
    assert isinstance(made, Group)
    assert (made.fit, made.renderables) == (False, ["a"])
    assert (renderables.__name__, renderables.__doc__) == ("renderables", "Some renderables.")


def test_no_splitter_is_a_layout_error():
    from rs_rich.layout import LayoutError, NoSplitter

    assert issubclass(NoSplitter, LayoutError)


def test_refresh_screen_is_not_supported():
    from rs_rich.console import Console
    from rs_rich.layout import Layout

    layout = Layout(name="x")
    with pytest.raises(NotImplementedError, match="refresh_screen"):
        layout.refresh_screen(Console(), "x")


def test_a_tree_that_contains_itself_is_refused():
    from rs_rich.console import Console
    from rs_rich.tree import Tree

    tree = Tree("root")
    tree.children.append(tree)
    with pytest.raises(RecursionError):
        Console(file=io.StringIO()).print(tree)


def test_a_deep_tree_renders_without_overflowing_the_stack():
    from rs_rich.console import Console
    from rs_rich.tree import Tree

    tree = Tree("0")
    node = tree
    for depth in range(1, 5000):
        node = node.add(str(depth))
    out = io.StringIO()
    Console(file=out, width=12, color_system=None).print(tree)
    assert out.getvalue() == "0\n└── 1\n    └── 2\n"


def test_objects_can_change_until_printed():
    def program(m, c):
        rule = m.rule.Rule("before")
        padding = m.padding.Padding("x", 0)
        tree = m.tree.Tree("root")
        rule.title = "after"
        padding.left = 3
        tree.label = "changed"
        tree.add("added later")
        c.print(rule, padding, tree)

    compare(program, width=12)


def test_user_renderables_work_inside_every_container():
    class Mine:
        def __rich_console__(self, console, options):
            yield f"mine at {options.max_width}"

    def program(m, c):
        mine = Mine()
        c.print(m.padding.Padding(mine, (0, 2)))
        c.print(m.align.Align.right(mine))
        c.print(m.columns.Columns([mine, "x"]))
        c.print(m.Group(mine, mine))
        c.print(m.tree.Tree(mine).add(mine))
        c.print(m.constrain.Constrain(mine, 12))
        c.print(m.styled.Styled(mine, "bold"))

    compare(program)


def test_module_paths():
    import rs_rich.align
    import rs_rich.containers
    import rs_rich.layout
    import rs_rich.measure
    import rs_rich.spinner

    assert rs_rich.align.Align.__module__ == "rs_rich.align"
    assert rs_rich.containers.Renderables is _native.Renderables
    assert rs_rich.layout.Layout is _native.Layout
    assert rs_rich.measure.measure_renderables is _native.measure_renderables
    assert rs_rich.spinner.SPINNERS is _native.SPINNERS
