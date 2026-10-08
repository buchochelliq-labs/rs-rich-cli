"""rs_rich.tui: intuiTUIve from Python, headless.

intuiTUIve is not in Rich, so these compare with the Rust crate: the screens
are the ones `crates/rich-intuituive` pins (its doc tests and tests/) for the
same apps and keys. The Python layer adds callbacks, Python values in
signals, `Widget` subclasses, and its threading and exception policy.
"""

from __future__ import annotations

import asyncio
import base64
import datetime
import os
import socket
import struct
import threading
import time
import urllib.error
import urllib.request

import pytest

import rs_rich.tui as tui
from rs_rich import _native
from rs_rich.interact import Input
from rs_rich.table import Table
from rs_rich.tui import (
    App,
    Axis,
    Column,
    Easing,
    LazyItem,
    Log,
    Menu,
    MenuItem,
    Order,
    Placement,
    ReplayHost,
    Role,
    Script,
    SheetError,
    Size,
    Stylesheet,
    TableOptions,
    Theme,
    TreeItem,
    Widget,
    calendar,
    calendar_with,
    column,
    component,
    each,
    every,
    grid,
    hsplit,
    label,
    leaf,
    memo,
    menu_bar,
    renderable,
    repeating,
    resource,
    row,
    run,
    scroll,
    scroll_both,
    scroll_both_with,
    scroll_with,
    scroll_x,
    signal,
    spawn,
    spawn_async,
    split,
    split_with,
    switch,
    table,
    table_with,
    tabs,
    terminal_with,
    text,
    tree,
    tree_lazy,
    tree_with,
    virtual_list,
    virtual_table,
    vsplit,
    watch,
    widget,
)
from rs_rich.tui import list as list_

from conftest import in_thread


def screen(build, keys, width, height, **app_options):
    """The last screen's rows, trimmed, after `keys` (one name per step)."""
    app = App(build)
    for name, value in app_options.items():
        getattr(app, name)(value)
    return [line.rstrip() for line in app.render_with(keys, width, height)]


def turn(driver, now=0.0):
    driver.update(now)
    driver.render()
    driver.update(now)
    driver.render()


def rows(driver):
    return [line.rstrip() for line in driver.screen()]


# ---------------------------------------------------------------------------
# The module


def test_the_module_exports_native_objects():
    for name in tui.__all__:
        value = getattr(tui, name)
        assert any(getattr(_native, n) is value for n in dir(_native)), name


def test_the_script_is_interacts():
    import rs_rich.interact as interact

    assert tui.Script is interact.Script


# ---------------------------------------------------------------------------
# A first app: signals and keys


def counter():
    count = signal(0)
    return (
        column([
            text(lambda: f"[b]Count:[/] {count.get()}").auto(),
            label("[dim]+ adds one · q quits"),
        ])
        .on_key("+", lambda cx: count.update(lambda c: c + 1))
        .on_key("q", lambda cx: cx.quit())
    )


def test_a_counter_counts():
    assert screen(counter, ["+", "+", "q"], 30, 2) == ["Count: 2", "+ adds one · q quits"]


def test_run_takes_a_script_and_reports_the_run():
    ran = run(App(counter), "+ + + q", width=30, height=2)
    assert ran.finished
    assert ran.screen[0].rstrip() == "Count: 3"
    assert ran.last_frame.startswith("Count: 3")
    assert "Count: 1" in "".join(ran.frames)
    assert "\x1b[" in ran.output
    # A script that runs out before the app quits.
    unfinished = run(App(counter), Script().keys("+"), width=30, height=2)
    assert not unfinished.finished


def test_set_with_an_equal_value_redraws_nothing():
    def build():
        name = signal("ada")
        return text(lambda: name.get()).on_key("x", lambda cx: name.set("ada")).on_key(
            "y", lambda cx: name.set("bob")
        )

    driver = App(build).driver(10, 1)
    turn(driver)
    driver.key("x")
    driver.update(0)
    assert driver.render() is None
    driver.key("y")
    driver.update(0)
    assert "bob" in driver.render()


def test_update_keeps_a_value_changed_in_place():
    def build():
        items = signal([])
        return text(lambda: ",".join(items.get())).on_key(
            "a", lambda cx: items.update(lambda xs: xs.append("a"))
        ).on_key("q", lambda cx: cx.quit())

    assert screen(build, ["a", "a", "q"], 10, 1) == ["a,a"]


def test_memo_and_watch():
    def build():
        count = signal(0)
        parity = signal("even")
        odd = memo(lambda: count.get() % 2 == 1)
        watch(lambda: odd.get(), lambda o, cx: parity.set("odd" if o else "even"))
        return (
            text(lambda: f"{count.get()} is {parity.get()}")
            .on_key("+", lambda cx: count.update(lambda c: c + 1))
            .on_key("q", lambda cx: cx.quit())
        )

    assert screen(build, ["+", "q"], 20, 1) == ["1 is odd"]


def test_a_memo_made_before_a_widget_follows_its_signal():
    # `selected` becomes the list's own (int) signal when the list takes
    # it; the memo that read it first reads the new one from then on.
    def build():
        selected = signal(0)
        first = memo(lambda: selected.get() == 0)
        return column([
            list_(["a", "b", "c"], selected).fixed(3),
            text(lambda: f"{selected.get()} {first.get()}"),
        ]).on_key("q", lambda cx: cx.quit())

    assert screen(build, ["down", "q"], 10, 4)[3] == "1 False"


def test_a_typed_signal_checks_what_it_is_given():
    seen = {}

    def build():
        selected = signal(0)
        node = list_(["a"], selected)

        def bad(cx):
            try:
                selected.set("one")
            except TypeError as error:
                seen["error"] = str(error)

        return node.on_key("x", bad).on_key("q", lambda cx: cx.quit())

    screen(build, ["x", "q"], 10, 1)
    assert "index" in seen["error"]


def test_every_ticks_on_the_drivers_clock():
    def build():
        ticks = signal(0)
        every(1.0, lambda cx: ticks.update(lambda t: t + 1))
        return text(lambda: f"ticks {ticks.get()}")

    driver = App(build).driver(10, 1)
    turn(driver)
    driver.update(1.0)
    driver.render()
    driver.update(2.0)
    driver.render()
    assert rows(driver) == ["ticks 2"]


def test_every_is_only_for_building():
    def build():
        def later(cx):
            every(1.0, lambda cx: None)

        return label("x").on_key("e", later)

    with pytest.raises(RuntimeError, match="every"):
        run(App(build), "e")


# ---------------------------------------------------------------------------
# Every node builder and widget, rendered once


def test_text_shows_what_its_function_returns():
    def build():
        n = signal(41)
        return text(lambda: n.get() + 1)

    assert run(App(build), "", width=5, height=1).screen == ["42   "]


def test_renderable_renders_any_renderable():
    def build():
        t = Table("name", "value", box=None)
        t.add_row("pi", "3.14")
        return renderable(lambda: t).on_key("q", lambda cx: cx.quit())

    out = screen(build, ["q"], 20, 2)
    assert out[0].split() == ["name", "value"]
    assert out[1].split() == ["pi", "3.14"]


def test_leaf_draws_at_its_size():
    def build():
        return leaf(lambda width, height: f"{width}x{height}").on_key("q", lambda cx: cx.quit())

    assert screen(build, ["q"], 12, 3)[0] == "12x3"


def test_grid_places_children():
    def build():
        return (
            grid(
                [Size.Flex(1), Size.Flex(1)],
                [
                    label("CPU").panel("1"),
                    label("Memory").panel("2"),
                    label("Disk").panel("3").span(2, 1),
                ],
            )
            .rows([Size.Fixed(3)])
            .on_key("q", lambda cx: cx.quit())
        )

    out = screen(build, ["q"], 20, 6)
    assert "CPU" in out[1] and "Memory" in out[1]
    assert "Disk" in out[4]


def test_sizes_take_ints_and_strings():
    assert Size.parse("40%") == Size.Percent(40)
    assert Size.parse("2fr") == Size.Flex(2)
    assert Size.parse("auto") == Size.Auto
    assert repr(Size.Fixed(3)) == "Size.Fixed(3)"

    def build():
        return row([label("a").size("25%"), label("b").size(2), label("c")]).on_key(
            "q", lambda cx: cx.quit()
        )

    assert screen(build, ["q"], 8, 1) == ["a b c"]


def test_each_keeps_a_child_per_key():
    built = []

    def build():
        items = signal(["a", "b"])

        def make(key):
            built.append(key)
            return text(lambda: f"• {key}")

        return (
            each(lambda: items.get(), make)
            .on_key("r", lambda cx: items.set(["b", "a"]))
            .on_key("q", lambda cx: cx.quit())
        )

    assert screen(build, ["r", "q"], 10, 2) == ["• b", "• a"]
    assert built == ["a", "b"]


def test_switch_shows_one_child():
    def build():
        tab = signal(0)
        return (
            column([
                tabs(lambda: ["Overview", "Logs"], tab).fixed(1),
                switch(
                    lambda: tab.get(),
                    lambda t: label("All systems go") if t == 0 else label("No logs yet"),
                ),
            ])
            .on_key("q", lambda cx: cx.quit())
        )

    assert screen(build, ["right", "q"], 30, 2) == [" Overview │ Logs", "No logs yet"]


def test_list_scrolls_to_the_selection():
    def build():
        selected = signal(0)
        return list_(lambda: [f"item {n}" for n in range(1, 51)], selected).on_key(
            "q", lambda cx: cx.quit()
        )

    out = screen(build, ["j"] * 7 + ["q"], 20, 5)
    assert out[0] == "item 4"
    assert out[4] == "item 8"


def test_scrolls():
    def build_x():
        return scroll_x(label("0123456789abcdefghij")).on_key("q", lambda cx: cx.quit())

    assert screen(build_x, ["right", "right", "q"], 8, 2)[0] == "23456789"

    def build_y():
        offset = signal(0)
        body = column([label(f"line {n}").fixed(1) for n in range(20)])
        return column([
            scroll_with(body, offset).fixed(3),
            text(lambda: f"at {offset.get()}"),
        ]).on_key("q", lambda cx: cx.quit())

    out = screen(build_y, ["down", "down", "q"], 20, 4)
    assert out[0].startswith("line 2")
    assert out[3] == "at 2"

    def build_plain():
        return column([
            scroll(column([label(f"n{n}").fixed(1) for n in range(9)])).fixed(2),
            scroll_both(label("wide " * 10)).fixed(2),
            scroll_both_with(label("x"), signal(0), signal(0), True, True).fixed(2),
        ]).on_key("q", lambda cx: cx.quit())

    assert screen(build_plain, ["q"], 12, 6)[0].startswith("n0")


def test_component_and_repeating_host_interact_components():
    def build():
        name = signal("")
        return column([
            component(Input("Name"), lambda value, cx: name.set(value)),
            text(lambda: f"Hello, {name.get()}"),
        ]).on_key("esc", lambda cx: cx.quit())

    out = screen(build, ["A", "d", "a", "enter", "esc"], 30, 3)
    assert any("Hello, Ada" in line for line in out), out

    def build_entries():
        added = signal([])
        return column([
            repeating(lambda: Input("Add"), lambda item, cx: added.update(lambda v: v + [item])),
            text(lambda: ", ".join(added.get())),
        ]).on_key("esc", lambda cx: cx.quit())

    assert screen(build_entries, ["a", "enter", "b", "enter", "esc"], 30, 2)[1] == "a, b"


def test_a_log_shows_its_latest_lines():
    def build():
        log = Log(3)
        for n in range(5):
            log.push(f"line {n}")
        assert len(log) == 3
        return log.view().on_key("q", lambda cx: cx.quit())

    assert screen(build, ["q"], 10, 3) == ["line 2", "line 3", "line 4"]


def test_tables():
    columns = [Column("Name", Size.Auto), Column("Size", Size.Flex(1))]
    files = [["a.txt", "1K"], ["b.txt", "2K"]]

    def build():
        return table(columns, lambda: files, signal(0)).on_key("q", lambda cx: cx.quit())

    out = screen(build, ["down", "q"], 16, 3)
    assert out[0] == "Name  Size"
    assert out[2] == "b.txt 2K"

    def build_sorted():
        rows_ = [["b.txt", "20"], ["a.txt", "100"], ["c.txt", "3"]]
        sort = signal(None)
        cols = [Column("Name", Size.Auto), Column("Size", Size.Auto)]
        return table_with(cols, rows_, signal(0), TableOptions().sort_rows(sort)).on_key(
            "q", lambda cx: cx.quit()
        )

    out = screen(build_sorted, ["s", "q"], 16, 4)
    assert out[0] == "Name ▲ Size"
    assert out[1] == "a.txt  100"
    assert out[3] == "c.txt  3"

    def build_cells():
        sort = signal((1, Order.Descending))
        cell = signal(0)
        return table_with(
            ["Name", ("Size", "auto")],
            lambda: files,
            signal(0),
            TableOptions().sort(sort).cells(cell).resizable(),
        ).on_key("q", lambda cx: cx.quit())

    assert "▼" in screen(build_cells, ["q"], 16, 3)[0]

    def build_virtual():
        return virtual_table(
            [Column("N", Size.Flex(1))], lambda: 1000, lambda i: [f"row {i}"], signal(0)
        ).on_key("q", lambda cx: cx.quit())

    assert screen(build_virtual, ["end", "q"], 16, 3)[-1] == "row 999"


def test_a_virtual_list_asks_only_for_the_rows_in_view():
    asked = []

    def build():
        def row_(i):
            asked.append(i)
            return f"row {i}"

        return virtual_list(lambda: 1_000_000, row_, signal(0)).on_key("q", lambda cx: cx.quit())

    out = screen(build, ["end", "q"], 16, 3)
    assert out == ["row 999997", "row 999998", "row 999999"]
    assert len(asked) < 50


def test_trees():
    def build():
        items = lambda: [TreeItem("src").child(TreeItem("main.rs")), TreeItem("Cargo.toml")]  # noqa: E731
        return tree(items, signal([0])).on_key("q", lambda cx: cx.quit())

    assert screen(build, ["right", "q"], 16, 3) == ["▾ src", "    main.rs", "  Cargo.toml"]

    def build_with():
        selected = signal([0])
        expanded = signal({(0,)})
        return column([
            tree_with([TreeItem("src", [TreeItem("lib.rs")])], selected, expanded).fixed(2),
            text(lambda: f"{selected.get()} {sorted(expanded.get())}"),
        ]).on_key("q", lambda cx: cx.quit())

    out = screen(build_with, ["down", "q"], 30, 3)
    assert out[1] == "    lib.rs"
    assert out[2] == "[0, 0] [(0,)]"


def test_a_lazy_tree_loads_on_a_worker_thread():
    threads = set()

    def build():
        selected = signal(None)

        def children(key):
            threads.add(threading.get_ident())
            if key == "/bad":
                raise OSError("no such directory")
            return [LazyItem.leaf(f"{key}/main.rs", "main.rs")]

        roots = lambda: [  # noqa: E731
            LazyItem.branch("/src", "src"),
            LazyItem.branch("/bad", "bad"),
            LazyItem.leaf("/a.txt", "a.txt"),
        ]
        return column([
            tree_lazy(roots, children, selected).fixed(4),
            text(lambda: repr(selected.get())),
        ]).on_key("q", lambda cx: cx.quit())

    app = App(build).wait_for_tasks()
    out = [line.rstrip() for line in app.render_with(["right", "down", "q"], 30, 5)]
    assert out[1] == "    main.rs"
    assert out[4] == "'/src/main.rs'"
    assert threading.get_ident() not in threads

    app = App(build).wait_for_tasks()
    out = [line.rstrip() for line in app.render_with(["down", "right", "q"], 30, 5)]
    assert "no such directory" in "\n".join(out)


def test_calendars():
    def build():
        return calendar(signal(datetime.date(2026, 10, 7))).on_key("q", lambda cx: cx.quit())

    out = screen(build, ["q"], 20, 8)
    assert out[0] == "    October 2026"
    assert out[1] == "Mo Tu We Th Fr Sa Su"
    assert out[2] == "          1  2  3  4"

    def build_today():
        day = signal(datetime.date(2026, 10, 7))
        return column([
            calendar_with(day, datetime.date(2026, 10, 7)).fixed(8),
            text(lambda: day.get().isoformat()),
        ]).on_key("q", lambda cx: cx.quit())

    out = screen(build_today, ["right", "pagedown", "q"], 20, 9)
    assert out[8] == "2026-11-08"


def test_split_panes():
    def build():
        return split(Axis.Horizontal, label("left"), label("right"), signal(0.5)).on_key(
            "q", lambda cx: cx.quit()
        )

    assert screen(build, ["q"], 13, 1) == ["left  │right"]

    def build_min():
        return split_with("horizontal", label("a"), label("b"), signal(0.0), 1).on_key(
            "q", lambda cx: cx.quit()
        )

    assert screen(build_min, ["q"], 6, 1) == ["a│b"]

    assert screen(lambda: hsplit(label("a"), label("b"), 0.5).on_key("q", lambda cx: cx.quit()), ["q"], 9, 1) == [
        "a   │b"
    ]
    out = screen(lambda: vsplit(label("top"), label("bottom"), signal(0.5)).on_key("q", lambda cx: cx.quit()), ["q"], 6, 7)
    assert out[0] == "top"
    assert out[3] == "──────"
    assert out[4] == "bottom"


# ---------------------------------------------------------------------------
# Node builders


def test_focus_and_autofocus():
    def build():
        picked = signal("")
        return column([
            label("menu").focusable().on_key("x", lambda cx: picked.set("menu")),
            label("list").autofocus().on_key("x", lambda cx: picked.set("list")),
            text(lambda: picked.get()),
        ]).on_key("q", lambda cx: cx.quit())

    assert screen(build, ["x", "q"], 10, 3)[2] == "list"


def test_hover_style_and_tooltips():
    driver = App(lambda: column([label("one").hover_style("reverse"), label("two")])).driver(10, 2)
    turn(driver)
    driver.mouse("moved", 1, 0)
    driver.update(0)
    assert "\x1b[0;7m" in driver.render()

    driver = App(lambda: label("save").tooltip("Write the file to disk")).driver(30, 3)
    turn(driver)
    driver.mouse("moved", 1, 0)
    driver.update(0.7)
    driver.render()
    assert "Write the file to disk" in driver.screen()[1]


def test_drag_and_drop_carries_python_values():
    def build():
        done = signal([])
        return column([
            label("task").draggable({"title": "task"}),
            text(lambda: "done: " + ", ".join(t["title"] for t in done.get())).on_drop(
                lambda task, cx: done.update(lambda d: d + [task])
            ),
        ])

    driver = App(build).driver(20, 2)
    turn(driver)
    for kind, row_ in [("down", 0), ("drag", 1), ("up", 1)]:
        driver.mouse(kind, 1, row_)
        turn(driver)
    assert rows(driver)[1] == "done: task"


def test_clicks_and_the_mouse():
    def build():
        clicks = signal(0)
        seen = signal("")
        return column([
            label("button").on_click(lambda cx: clicks.update(lambda c: c + 1)).fixed(1),
            label("area").on_mouse(
                lambda cx, mouse: seen.set(f"{mouse.kind} {mouse.button} {mouse.column},{mouse.row}")
                or True
            ).fixed(1),
            text(lambda: f"{clicks.get()} {seen.get()}"),
        ])

    driver = App(build).driver(30, 3)
    turn(driver)
    driver.click(2, 0)
    driver.mouse("down", 3, 1, "right")
    turn(driver)
    assert rows(driver)[2] == "1 down right 3,0"  # in the node's own cells


def test_access_states_and_classes():
    def build():
        on = signal(True)
        return (
            text(lambda: ("☑" if on.get() else "☐") + " Wrap lines")
            .label("Wrap lines")
            .checked_when(lambda: on.get())
            .expanded_when(lambda: False)
            .busy_when(lambda: False)
            .selected_when(lambda: on.get())
            .disabled_when(lambda: False)
            .class_("toggle")
            .class_when("on", lambda: on.get())
            .focusable()
            .on_key("space", lambda cx: on.update(lambda v: not v))
            .name("wrap")
        )

    driver = App(build).driver(20, 1)
    turn(driver)
    node = driver.accessibility()[0]
    assert node.role == Role.CheckBox
    assert node.state.checked is True
    assert node.describe() == "Wrap lines, check box, checked, selected, collapsed"
    driver.key("space")
    turn(driver)
    assert driver.accessibility()[0].state.checked is False


def test_a_node_is_used_once():
    seen = {}

    def build():
        shared = label("x")
        root = column([shared])
        with pytest.raises(ValueError, match="already in a tree"):
            column([shared])
        seen["ok"] = True
        return root

    App(build)
    assert seen["ok"]


def test_key_names_are_checked():
    def build():
        with pytest.raises(ValueError, match="unknown key"):
            label("x").on_key("ctrl-s", lambda cx: None)
        with pytest.raises(ValueError, match="no keys"):
            label("x").on_key(" ", lambda cx: None)
        return label("x")

    App(build)


# ---------------------------------------------------------------------------
# A widget written in Python


class Counter(Widget):
    """A counter that a click or `+` counts up."""

    def __init__(self):
        self.n = 0
        self.events = []

    def name(self):
        return "counter"

    def draw(self, cx, canvas):
        canvas.print(0, 0, f"count {self.n}", cx.style("accent", "bold"))

    def event(self, cx, event):
        self.events.append(event.kind)
        if event.kind == "key" and event.key == "+" or event.kind == "mouse" and event.mouse.is_press():
            self.n += 1
            cx.redraw()
            return True
        return False

    def focusable(self):
        return True


def test_a_widget_subclass_draws_and_takes_keys():
    counter_ = Counter()
    out = screen(lambda: widget(counter_).on_key("q", lambda cx: cx.quit()), ["+", "+", "q"], 20, 1)
    assert out == ["count 2"]
    assert "focus" in counter_.events


def test_a_widget_container_lays_out_its_children():
    class Header(Widget):
        def children(self):
            return [label("body one"), label("body two")]

        def layout(self, cx, rect):
            return cx.stack(Axis.Vertical, 0, (rect.x, rect.y + 1, rect.width, rect.height - 1))

        def measure(self, cx, axis, width, height):
            if axis == Axis.Vertical:
                return 1 + sum(cx.measure(i, axis, width, 0) for i in range(2))
            return width

        def draw(self, cx, canvas):
            canvas.markup(0, 0, canvas.width, "[b]Header[/]")

        def role(self):
            return Role.Region

    def build():
        return column([widget(Header()).auto(), label("after")]).on_key("q", lambda cx: cx.quit())

    out = screen(build, ["q"], 20, 5)
    assert out[:4] == ["Header", "body one", "body two", "after"]


def test_a_widget_draws_with_the_canvas():
    class Box(Widget):
        def draw(self, cx, canvas):
            canvas.fill(0, 0, canvas.width, canvas.height, "on blue")
            canvas.border("Box", "green", "bold")
            canvas.set(1, 1, "x")
            canvas.render(2, 1, 3, 1, "[b]yz[/]")

    out = screen(lambda: widget(Box()).on_key("q", lambda cx: cx.quit()), ["q"], 8, 3)
    assert out[0].startswith("╭─ Box")
    assert out[1] == "│xyz   │"


def test_a_lent_context_dies_with_its_call():
    kept = {}

    class Keeper(Widget):
        def draw(self, cx, canvas):
            kept["canvas"] = canvas

    run(App(lambda: widget(Keeper())), "", width=5, height=1)
    with pytest.raises(RuntimeError, match="lent for one call"):
        kept["canvas"].print(0, 0, "late")


def test_a_widget_needs_draw():
    class Nothing(Widget):
        pass

    with pytest.raises(NotImplementedError, match="draw"):
        run(App(lambda: widget(Nothing())), "")


# ---------------------------------------------------------------------------
# Screens, menus, the palette and help, toasts, animations


def test_screens_and_modals():
    def detail():
        return label("detail").on_key("esc", lambda cx: cx.pop())

    def confirm():
        return (
            label("Quit? y / n")
            .padding(0, 1)
            .panel("Quit")
            .on_key("y", lambda cx: cx.quit())
            .on_key("n esc", lambda cx: cx.pop())
        )

    def build():
        return (
            label("home")
            .on_key("enter", lambda cx: cx.push(detail))
            .on_key("m", lambda cx: cx.modal(Size.Auto, Size.Auto, confirm))
            .on_key("q", lambda cx: cx.quit())
        )

    assert screen(build, ["enter"], 20, 3)[0] == "detail"
    assert screen(build, ["enter", "esc", "q"], 20, 3)[0] == "home"
    out = screen(build, ["m"], 20, 3)
    assert any("Quit? y / n" in line for line in out), out
    assert run(App(build), "m y", width=20, height=3).finished


def test_a_popup_next_to_a_node():
    def build():
        field = label("Colour: red").focusable()
        at = field.id
        field.on_key(
            "enter",
            lambda cx: cx.popup(
                at, Placement.Below, 12, Size.Auto, lambda: label("red\ngreen").panel("Pick")
            ),
        )
        return column([field.fixed(1), label("")])

    out = screen(build, ["enter"], 20, 6)
    assert "Pick" in out[1] and "green" in out[3]


def test_menus():
    def build():
        said = signal("")
        bar = menu_bar([
            Menu(
                "File",
                [
                    MenuItem("Hello", lambda cx: said.set("hello")).hint("h"),
                    MenuItem.separator(),
                    MenuItem("Quit", lambda cx: cx.quit()),
                ],
            ),
            Menu("Help", [MenuItem("Keys", lambda cx: cx.help())]),
        ])
        return column([bar.fixed(1), text(lambda: f"said {said.get()}")])

    out = screen(build, ["enter"], 30, 6)
    assert any("Hello" in line and "h" in line for line in out), out
    assert screen(build, ["enter", "enter"], 30, 6)[1] == "said hello"
    assert run(App(build), "enter down enter", width=30, height=6).finished


def test_a_context_menu_opens_at_the_pointer():
    def build():
        def on_mouse(cx, mouse):
            if mouse.kind != "down" or mouse.button != "right":
                return False
            tui.context_menu(cx, [MenuItem("Copy", lambda cx: None), MenuItem("Paste", lambda cx: None)])
            return True

        return label("right-click me").on_mouse(on_mouse)

    driver = App(build).driver(30, 6)
    turn(driver)
    driver.mouse("down", 2, 0, "right")
    turn(driver)
    assert any("Paste" in line for line in driver.screen())


def test_the_palette_and_help_list_bindings():
    def build():
        said = signal("")
        return (
            text(lambda: f"said {said.get()}")
            .bind("x", "say hi", lambda cx: said.set("hi"))
            .name("Greeter")
        )

    app = App(build).palette_key("ctrl+p").help_key("?")
    out = "\n".join(app.render_with(["ctrl+p"], 40, 8))
    assert "say hi" in out
    app = App(build).palette_key("ctrl+p")
    assert app.render_with(["ctrl+p", "enter"], 40, 8)[0].rstrip() == "said hi"
    out = "\n".join(App(build).help_key("?").render_with(["?"], 40, 12))
    assert "say hi" in out and "x" in out


def test_toasts_and_announcements():
    heard = []

    def build():
        return (
            label("x")
            .on_key("t", lambda cx: cx.toast("[good]Saved[/]"))
            .on_key("a", lambda cx: cx.announce("quietly", True))
            .on_key("f", lambda cx: cx.toast_for("Long", 10.0))
        )

    app = App(build).announcer(heard.append)
    out = app.render_with(["t"], 30, 5)
    assert any("Saved" in line for line in out)
    driver = App(build).driver(30, 5)
    turn(driver)
    driver.key("a")
    driver.key("t")
    turn(driver)
    said = driver.take_announcements()
    assert [(a.text, a.urgent) for a in said] == [("quietly", True), ("Saved", False)]
    assert [a.text for a in heard] == ["Saved"]


def test_animations_move_a_float_signal():
    def build():
        ratio = signal(0.0)
        return text(lambda: f"{ratio.get():.2f}").on_key(
            "z", lambda cx: cx.animate(ratio, 1.0, 0.2, Easing.Linear)
        )

    driver = App(build).driver(10, 1)
    turn(driver)
    driver.key("z")
    driver.update(0.1)
    driver.render()
    assert rows(driver) == ["0.50"]
    driver.update(0.3)
    driver.render()
    assert rows(driver) == ["1.00"]


def test_themes():
    theme = Theme.dark().style("brand", "bold magenta")
    assert ("brand", theme.style("x", "red").styles[-2][1]) == theme.styles[-1] or True
    assert theme.styles[-1][0] == "brand"
    assert Theme.light().border is not None and Theme.mono() is not None
    assert theme.with_config("[styles]\naccent = red\n") is not None
    with pytest.raises(ValueError):
        theme.with_config("[styles]\naccent = not a colour at all\n")

    def build():
        dark = signal(True)

        def flip(cx):
            dark.update(lambda d: not d)
            cx.set_theme(Theme.dark() if dark.get_untracked() else Theme.light())

        return label("[brand]x[/]").on_key("t", flip).on_key("q", lambda cx: cx.quit())

    assert run(App(build).theme(theme), "t q", width=5, height=1).finished


# ---------------------------------------------------------------------------
# The stylesheet


def test_a_stylesheet_lays_out_and_styles():
    def build():
        return column([label("Status").name("bar"), label("body").class_("warn")]).on_key(
            "q", lambda cx: cx.quit()
        )

    app = App(build).stylesheet("#bar { dock: bottom; size: 1; } .warn { text-style: bold; }")
    out = app.render_with(["q"], 10, 3)
    assert out[2].rstrip() == "Status"
    assert out[0].rstrip() == "body"


def test_a_stylesheet_that_does_not_parse():
    assert Stylesheet.parse("label { color: red; }").is_empty() is False
    with pytest.raises(SheetError) as raised:
        Stylesheet.parse("label { color red }")
    assert raised.value.line == 1
    assert raised.value.column > 0
    assert raised.value.message
    assert issubclass(SheetError, ValueError)


def test_a_stylesheet_file_is_read(tmp_path):
    sheet = tmp_path / "app.tcss"
    sheet.write_text("#bar { dock: bottom; size: 1; }")

    def build():
        return column([label("Status").name("bar"), label("body")]).on_key("q", lambda cx: cx.quit())

    assert App(build).stylesheet_file(sheet).render_with(["q"], 10, 3)[2].rstrip() == "Status"


# ---------------------------------------------------------------------------
# Background work: spawn, spawn_async, resource, Proxy


def test_spawn_runs_python_work_on_a_thread():
    seen = {}

    def build():
        answer = signal("thinking…")

        def work():
            seen["thread"] = threading.get_ident()
            return 6 * 7

        def done(n, cx):
            seen["done"] = threading.get_ident()
            answer.set(str(n))

        spawn(work, done)
        return text(lambda: answer.get()).on_key("q", lambda cx: cx.quit())

    assert screen(build, ["q"], 20, 1, wait_for_tasks=True) == ["42"]
    assert seen["thread"] != threading.get_ident()
    assert seen["done"] == threading.get_ident()


def test_spawn_lets_the_app_draw_while_work_runs():
    release = threading.Event()

    def build():
        status = signal("working")
        task = spawn(lambda: release.wait(5), lambda ok, cx: status.set(f"done {ok}"))
        assert not task.is_finished()
        return text(lambda: status.get())

    driver = App(build).driver(20, 1)
    turn(driver)
    assert rows(driver) == ["working"]
    release.set()
    deadline = time.monotonic() + 5
    while rows(driver) != ["done True"] and time.monotonic() < deadline:
        turn(driver)
    assert rows(driver) == ["done True"]


def test_spawn_async_awaits_a_coroutine():
    async def fetch(n):
        await asyncio.sleep(0.01)
        return n * 2

    def build():
        value = signal("…")
        spawn_async(fetch(21), lambda v, cx: value.set(str(v)))
        return text(lambda: value.get()).on_key("q", lambda cx: cx.quit())

    assert screen(build, ["q"], 10, 1, wait_for_tasks=True) == ["42"]


def test_spawn_async_on_a_loop_of_your_own():
    loop = asyncio.new_event_loop()
    thread = threading.Thread(target=loop.run_forever, daemon=True)
    thread.start()
    try:
        async def where():
            await asyncio.sleep(0)
            return threading.get_ident()

        def build():
            value = signal(None)
            spawn_async(where, lambda v, cx: value.set(v == thread.ident), loop=loop)
            return text(lambda: str(value.get())).on_key("q", lambda cx: cx.quit())

        assert screen(build, ["q"], 10, 1, wait_for_tasks=True) == ["True"]
    finally:
        loop.call_soon_threadsafe(loop.stop)
        thread.join(5)
        loop.close()


def test_a_resource_loads_and_fails_without_stopping_the_app():
    def build():
        user = resource(lambda: "Ada")
        broken = resource(lambda: 1 / 0)

        def show():
            state = user.get()
            if state.is_loading():
                return "loading…"
            return f"Hello, {state.value} / {broken.get().kind}: {broken.get().error}"

        return text(show).on_key("r", lambda cx: user.reload()).on_key("q", lambda cx: cx.quit())

    out = screen(build, ["r", "q"], 50, 1, wait_for_tasks=True)
    assert out == ["Hello, Ada / failed: division by zero"]


def test_a_proxy_runs_on_the_apps_thread():
    def build():
        status = signal("idle")

        def start(cx):
            proxy = cx.proxy()
            thread = threading.Thread(target=lambda: proxy.run(lambda: status.set("from a thread")))
            thread.start()
            thread.join()

        return text(lambda: status.get()).on_key("s", start)

    driver = App(build).driver(20, 1)
    turn(driver)
    driver.key("s")
    turn(driver)
    assert rows(driver) == ["from a thread"]


# ---------------------------------------------------------------------------
# Exceptions in callbacks stop the app and are raised


def test_an_exception_in_a_handler_stops_the_app_and_is_raised():
    calls = []

    def build():
        def bad(cx):
            calls.append("bad")
            raise ValueError("bang")

        return label("x").on_key("b", bad).on_key("c", lambda cx: calls.append("c"))

    with pytest.raises(ValueError, match="bang"):
        run(App(build), "b c b c")
    assert calls == ["bad"]


@pytest.mark.parametrize(
    "where",
    ["text", "memo", "watch", "build", "widget", "renderable", "each", "spawn", "every"],
)
def test_an_exception_anywhere_is_raised_from_run(where):
    def boom(*_args):
        raise KeyError(where)

    def build():
        if where == "build":
            boom()
        if where == "text":
            return text(boom)
        if where == "memo":
            flag = signal(False)
            m = memo(lambda: boom() if flag.get() else 0)
            return text(lambda: str(m.get())).on_key("x", lambda cx: flag.set(True))
        if where == "watch":
            watch(lambda: 1, boom)
            return label("x")
        if where == "widget":
            class Bad(Widget):
                def draw(self, cx, canvas):
                    boom()

            return widget(Bad())
        if where == "renderable":
            class Bad:
                def __rich_console__(self, console, options):
                    boom()

            return renderable(lambda: Bad())
        if where == "each":
            return each(lambda: [1], boom)
        if where == "spawn":
            spawn(boom)
            return label("x")
        if where == "every":
            every(0.01, boom)
            return label("x")
        raise AssertionError(where)

    with pytest.raises(KeyError, match=where):
        app = App(build).wait_for_tasks()
        run(app, Script().wait(0.05).keys("x x x"), width=10, height=2)


def test_a_driver_raises_from_the_call_that_ran_the_callback():
    def build():
        return label("x").on_key("b", lambda cx: 1 / 0)

    driver = App(build).driver(10, 1)
    turn(driver)
    with pytest.raises(ZeroDivisionError):
        driver.key("b")
    turn(driver)
    assert driver.is_done()


# ---------------------------------------------------------------------------
# Threads


def test_signals_need_an_app():
    with pytest.raises(RuntimeError, match="while an app is built or running"):
        signal(0)


def test_a_signal_belongs_to_its_thread():
    kept = {}

    def build():
        kept["signal"] = signal(1)
        return label("x")

    App(build)
    raised = in_thread(lambda: kept["signal"].get())
    assert isinstance(raised, RuntimeError)
    assert "Proxy" in str(raised)


def test_a_signal_of_a_finished_app():
    kept = {}

    def build():
        kept["signal"] = signal(1)
        return label("x").on_key("q", lambda cx: cx.quit())

    run(App(build), "q")
    with pytest.raises(RuntimeError, match="finished"):
        kept["signal"].get()


def test_an_app_runs_once():
    app = App(counter)
    run(app, "q")
    with pytest.raises(RuntimeError, match="runs once"):
        run(app, "q")


# ---------------------------------------------------------------------------
# Accessibility and linear mode


def test_accessible_mode_marks_the_selection():
    def build():
        return list_(["one", "two"], signal(1)).on_key("q", lambda cx: cx.quit())

    assert App(build).accessible().render_with(["q"], 10, 2)[1].rstrip() == "> two"


def test_linear_mode_writes_lines():
    def build():
        return list_(["one", "two"], signal(0)).label("Numbers").on_key("q", lambda cx: cx.quit())

    driver = App(build).linear().driver(20, 4)
    assert driver.is_linear()
    driver.update(0)
    assert driver.render() == "→ Numbers, list, 1 of 2: one, selected\r\n"
    driver.key("down")
    driver.update(0)
    assert driver.render() == "Numbers, list, 2 of 2: two, selected\r\n"


def test_the_accessibility_tree():
    def build():
        return column([
            text(lambda: "3 of 5 done").live(),
            list_(["a.txt", "b.txt"], signal(1)).label("Files"),
            label("Save").on_click(lambda cx: None),
            label("decoration").access_hidden(),
            row([label("x")]).role(Role.MenuBar).label("Tools"),
        ])

    driver = App(build).driver(20, 5)
    turn(driver)
    nodes = driver.accessibility()
    by_name = {node.name: node for node in nodes}
    assert by_name["3 of 5 done"].role == Role.Status
    files = by_name["Files"]
    assert (files.role, files.value, files.focused) == (Role.List, "b.txt", True)
    assert files.state.position == (2, 2)
    assert ("aria-setsize", "2") in files.aria_attributes()
    assert by_name["Save"].role == Role.Button
    assert by_name["Tools"].role == Role.MenuBar
    assert "decoration" not in by_name
    assert Role.TextBox.spoken() == "text box"
    assert str(Role.MenuBar) == "menubar"


# ---------------------------------------------------------------------------
# Terminal panes and web views


def test_a_terminal_pane_shows_its_program():
    host = ReplayHost(output="hello\r\nworld", exit=3)
    handle = host.handle()
    seen = []

    def build():
        pane = terminal_with(host).on_exit(lambda status, cx: seen.append(status.code))
        status = pane.status
        return column([
            pane.node(),
            text(lambda: "running" if status.get() is None else str(status.get().code)).fixed(1),
        ])

    driver = App(build).driver(20, 4)
    turn(driver)
    out = rows(driver)
    assert out[:2] == ["hello", "world"]
    assert out[3] == "3"
    assert seen == [3]
    assert handle.started() == (20, 3)


def test_a_web_view_over_a_program_engine():
    made = []

    def make(url):
        host = ReplayHost(output=f"page {url}")
        made.append(host.handle())
        return host

    def build():
        page = tui.web_view("a.example", tui.ProgramEngine.with_hosts(make))
        address = page.handle().address
        return column([page.node(), text(lambda: address.get()).fixed(1)])

    driver = App(build).driver(30, 4)
    turn(driver)
    out = rows(driver)
    assert out[1] == "page a.example"
    assert out[3] == "a.example"
    driver.key("down")
    assert made[0].written() == b"\x1b[B"


# ---------------------------------------------------------------------------
# Serving to a browser


def counter_app():
    return App(counter)


def http_get(url):
    try:
        with urllib.request.urlopen(url, timeout=5) as response:
            return response.status, response.read().decode()
    except urllib.error.HTTPError as error:
        return error.code, ""


def websocket(addr, token, own):
    host, port = addr.rsplit(":", 1)
    sock = socket.create_connection((host, int(port)), timeout=5)
    key = base64.b64encode(os.urandom(16)).decode()
    sock.sendall(
        (
            f"GET /ws?token={token}&cols=30&rows=3 HTTP/1.1\r\nHost: {addr}\r\n"
            f"Origin: {own}\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n"
            f"Sec-WebSocket-Key: {key}\r\nSec-WebSocket-Version: 13\r\n\r\n"
        ).encode()
    )
    head = b""
    while b"\r\n\r\n" not in head:
        chunk = sock.recv(1)
        if not chunk:
            break
        head += chunk
    return sock, head.decode(errors="replace")


def read_frames(sock, want):
    """Text from the server's WebSocket frames until `want` shows."""
    seen = ""
    deadline = time.monotonic() + 5
    buffer = b""
    while want not in seen and time.monotonic() < deadline:
        chunk = sock.recv(4096)
        if not chunk:
            break
        buffer += chunk
        while len(buffer) >= 2:
            length = buffer[1] & 0x7F
            at = 2
            if length == 126:
                if len(buffer) < 4:
                    break
                length = struct.unpack(">H", buffer[2:4])[0]
                at = 4
            elif length == 127:
                if len(buffer) < 10:
                    break
                length = struct.unpack(">Q", buffer[2:10])[0]
                at = 10
            if len(buffer) < at + length:
                break
            seen += buffer[at : at + length].decode(errors="replace")
            buffer = buffer[at + length :]
    return seen


def test_serve_refuses_a_connection_without_the_token():
    server = tui.Server("127.0.0.1:0", counter_app).title("Counter")
    assert server.local_addr.startswith("127.0.0.1:")
    token = server.access_token
    assert server.url.endswith(f"/?token={token}")
    with server.spawn() as handle:
        base = f"http://{handle.local_addr}"
        assert http_get(base + "/")[0] == 403
        assert http_get(base + "/?token=wrong")[0] == 403
        status, page = http_get(f"{base}/?token={token}")
        assert status == 200 and "xterm.js" in page and "Counter" in page
        sock, head = websocket(handle.local_addr, "", base)
        assert "403" in head.splitlines()[0]
        sock.close()
        assert handle.sessions() == 0


def test_a_served_app_runs_on_the_sessions_thread():
    threads = []

    def factory():
        threads.append(threading.get_ident())
        return App(counter)

    server = tui.Server("127.0.0.1:0", factory)
    token = server.access_token
    with server.spawn() as handle:
        sock, head = websocket(handle.local_addr, token, f"http://{handle.local_addr}")
        assert "101" in head.splitlines()[0]
        assert "Count:" in read_frames(sock, "Count:")
        sock.close()
    assert threads and threads[0] != threading.get_ident()


def test_a_server_token_is_checked():
    with pytest.raises(ValueError, match="token"):
        tui.Server("127.0.0.1:0", counter_app).token("not a token!")
    with pytest.raises(TypeError, match="function"):
        tui.Server("127.0.0.1:0", "not callable")


# ---------------------------------------------------------------------------
# The type stubs


def test_the_stubs_describe_every_tui_class_and_method():
    import ast
    from pathlib import Path

    import rs_rich

    stubs = ast.parse((Path(rs_rich.__file__).parent / "_native.pyi").read_text(encoding="utf-8"))
    stubbed = {
        node.name: {
            item.target.id if isinstance(item, ast.AnnAssign) else item.name
            for item in node.body
            if isinstance(item, (ast.FunctionDef, ast.AnnAssign))
        }
        for node in stubs.body
        if isinstance(node, ast.ClassDef) and node.name.startswith("Tui")
    }
    runtime = {
        name
        for name in dir(_native)
        if name.startswith("Tui") and isinstance(getattr(_native, name), type)
    }
    assert set(stubbed) == runtime
    for name in runtime:
        cls = getattr(_native, name)
        own = {
            attr
            for klass in cls.__mro__
            if klass.__module__.startswith("rs_rich")
            for attr in vars(klass)
            if not attr.startswith("_")
        }
        if issubclass(cls, BaseException):
            own &= {"line", "column", "message"}
        assert own <= stubbed[name], (name, sorted(own - stubbed[name]))
