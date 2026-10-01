"""rs_rich.interact composition: `Component` subclasses, the containers, and
keymaps (0.0.14). The containers are rs-rich-interact's; these check the
Python side: a component written in Python composes with the built-ins, and
exceptions come out of the run.
"""

from __future__ import annotations

import pytest

import rs_rich.interact as interact
from rs_rich.interact import (
    Binding,
    Cancel,
    Column,
    Component,
    Confirm,
    Context,
    Done,
    Ignored,
    Input,
    Keymap,
    Label,
    Layer,
    Layers,
    Map,
    Row,
    Script,
    Select,
    Split,
    Stack,
    Tabs,
    headless,
)


class Counter(Component):
    """Counts Up presses; Enter answers with the count, Escape cancels.
    Every other key is left to the container."""

    def __init__(self, label="count"):
        self.label = label
        self.count = 0
        self.sizes = []
        self.keys = Keymap("counter").bind("up", "up k", "count up").bind("done", "enter", "answer")

    def handle(self, event):
        action = self.keys.action(event)
        if action == "up":
            self.count += 1
        elif action == "done":
            return Done(self.count)
        elif event.key == "escape":
            return Cancel()
        else:
            return Ignored()
        return None

    def render(self, context):
        self.sizes.append((context.width, context.height))
        return f"{self.label} [bold]{self.count}[/]"

    def keymap(self):
        return self.keys


FILES = ["a.rs", "b.rs", "c.rs"]


# ---------------------------------------------------------------------------
# The classes


def test_the_classes_say_where_they_live():
    for name in ("Component", "Context", "Ignored", "Keymap", "Binding", "Label", "Map", "Container",
                 "Stack", "Column", "Row", "Split", "Tabs", "Layer", "Layers"):
        cls = getattr(interact, name)
        assert cls.__module__ == "rs_rich.interact", name
        assert cls.__name__ == name
    assert issubclass(Column, Stack) and issubclass(Row, Stack)
    assert issubclass(Stack, interact.Container) and issubclass(Split, interact.Container)


def test_a_component_subclass_runs_on_its_own():
    counter = Counter()
    record = counter.headless("up k enter", width=30, height=4)
    assert record.value == 2
    assert record.last_frame == "count 2"
    # render gets the context: the space it has.
    assert counter.sizes[0] == (30, 4)
    assert isinstance(Context, type)
    assert headless(Counter(), "escape").outcome.cancelled


def test_a_component_needs_render():
    class Nothing(Component):
        pass

    with pytest.raises(NotImplementedError, match="Nothing must define render"):
        headless(Nothing(), "x")


def test_the_defaults():
    component = Counter()
    assert Component.handle(component, None).__class__ is Ignored
    assert component.focusable() is True
    assert component.mouse() is False
    assert component.tick() is None
    assert component.start(None) is None
    assert component.default_value() is None
    assert Component.keymap(component) is None


# ---------------------------------------------------------------------------
# Composing with the built-ins


def app():
    counter = Counter()
    picker = Select("File", FILES)
    return counter, Split(counter, picker, ratio=40)


def test_a_python_component_composes_with_a_built_in_in_a_split():
    # Keys go to the focused pane, the Python one first.
    counter, split = app()
    record = split.headless("up up enter", width=60, height=6)
    assert record.value == 2
    frame = record.frames[-1]
    assert "count 2" in frame and "File" in frame
    # It rendered at its pane's width, not the whole terminal's.
    assert counter.sizes[-1][0] < 30

    # Tab is Ignored by the Python component, bubbles up, and moves focus
    # to the Select, which answers with its item's value.
    counter, split = app()
    record = split.headless("up tab down enter", width=60, height=6)
    assert record.value == "b.rs"
    assert counter.count == 1
    assert any("count 1" in frame for frame in record.frames)


def test_every_container_takes_python_and_built_in_children():
    def flows(root, keys):
        return root.headless(keys, width=60, height=10).value

    assert flows(Column(Label("[bold]Title[/]"), Counter(), Select("File", FILES)), "up enter") == 1
    assert flows(Row(Counter(), Select("File", FILES)), "tab enter") == "a.rs"
    stack = Stack(axis="horizontal", gap=1).child(Counter(), size=12).child(Select("File", FILES), ratio=1)
    assert flows(stack, "tab down enter") == "b.rs"
    tabs = Tabs([("Count", Counter()), ("Files", Select("File", FILES))])
    assert tabs.titles == ["Count", "Files"]
    assert flows(tabs, "alt+right down down enter") == "c.rs"
    assert flows(Tabs({"Count": Counter()}).tab("More", Counter("more")), "alt+2 up enter") == 1
    assert flows(Split.vertical(Select("File", FILES), Counter()), "tab up enter") == 1


def test_empty_tabs_take_tab_and_shift_tab():
    # 0.0.14 release-test audit B4: Tab and Shift+Tab on Tabs with no tabs
    # (built from data that may be empty) raised PanicException.
    for keys in ("tab", "shift+tab", "tab shift+tab alt+right enter"):
        record = Tabs().headless(keys, width=40, height=5)
        assert record.frames
    record = Column(Tabs([]), Input("Name")).headless(
        Script().keys("tab shift+tab tab").text("ok").keys("enter"), width=40, height=6
    )
    assert record.value == "ok"


def test_map_says_what_an_answer_means():
    seen = []
    name = Map(Input("Name"), lambda value: seen.append(value))
    form = Column(name, Counter())
    # The input's answer carries on; focus moves on to the counter.
    record = form.headless(Script().text("Ada").keys("enter up enter"), width=40, height=6)
    assert seen == ["Ada"]
    assert record.value == 1
    # Done(...) from the callback finishes with its value; a cancel mapped
    # to None carries on.
    mapped = Map(Select("File", FILES), lambda file: Done(file.upper()))
    assert mapped.headless("enter").value == "A.RS"
    carry = Column(Map(Confirm("Sure?"), cancel=lambda: None), Counter())
    assert carry.headless("escape up enter", width=40, height=10).value == 1
    assert Counter().map(lambda count: Done(count * 10)).headless("up enter").value == 10
    with pytest.raises(TypeError, match="callable"):
        Map(Counter(), 3)


def test_a_containers_own_bindings():
    root = Column(Counter()).on("quit", "ctrl+q", "quit", lambda: Done("bye"))
    assert root.headless("up ctrl+q").value == "bye"
    # A shortcut runs before the focused child sees the key.
    root = Column(Counter()).shortcut("steal", "up", "take up", lambda: Done("stolen"))
    assert root.headless("up").value == "stolen"
    # Rebinding focus-next: Tab no longer moves focus, F6 does.
    root = Row(Counter(), Select("File", FILES)).rebind("focus-next", "f6")
    assert root.headless("tab f6 enter").value == "a.rs"
    with pytest.raises(ValueError, match="unknown key name"):
        Column().on("x", "notakey", "x", lambda: None)
    with pytest.raises(TypeError, match="callable"):
        Column().on("x", "x", "x", "not callable")


def test_layers_open_python_and_built_in_dialogs():
    sure = Map(Confirm("Quit?"), lambda answer: Done("quit") if answer == "yes" else None)
    root = Layers(Counter()).open_on("quit", "ctrl+q", "quit", lambda: Layer.modal(sure, title="Quit", size=(30, 7)))
    record = root.headless("up ctrl+q y", width=50, height=12)
    assert record.value == "quit"
    assert any("Quit?" in frame for frame in record.frames)
    # A factory may return a component, shown as a modal; Escape dismisses
    # it and the base carries on.
    root = Layers(Counter()).open_on("more", "f2", "more", lambda: Counter("inner"))
    record = root.headless("f2 escape up enter", width=50, height=12)
    assert record.value == 1
    assert any("inner 0" in frame for frame in record.frames)
    # A layer open from the start takes the keys first.
    root = Layers(Counter()).open(Layer.popover(Counter("pop")))
    assert root.headless("up up enter", width=50, height=12).value == 2
    with pytest.raises(ValueError, match="modal or popover"):
        Layer(Counter(), kind="toast")


def test_children_are_checked_when_added():
    with pytest.raises(TypeError, match="interactive component"):
        Column(object())
    with pytest.raises(TypeError, match="interactive component"):
        Split(Counter(), 3)
    with pytest.raises(ValueError, match="size or ratio"):
        Column().child(Counter(), size=1, ratio=1)
    with pytest.raises(ValueError, match="percentage"):
        Split(Counter(), Counter(), ratio=101)
    with pytest.raises(ValueError, match="horizontal or vertical"):
        Stack(axis="diagonal")


def test_a_container_that_contains_itself_is_refused():
    column = Column(Counter())
    column.child(column)
    with pytest.raises(RecursionError, match="contain itself"):
        column.headless("enter")


# ---------------------------------------------------------------------------
# Exceptions come out of the run


def test_an_exception_in_handle_is_raised_from_the_run():
    class Broken(Counter):
        def handle(self, event):
            raise RuntimeError("boom in handle")

    split = Split(Broken(), Select("File", FILES))
    with pytest.raises(RuntimeError, match="boom in handle"):
        split.headless("up down enter")


def test_an_exception_in_render_is_raised_from_the_run():
    class Unrenderable(Counter):
        def render(self, context):
            raise LookupError("no view")

    with pytest.raises(LookupError, match="no view"):
        Row(Select("File", FILES), Unrenderable()).headless("down down enter")


def test_an_exception_in_a_callback_is_raised_from_the_run():
    def fails(value):
        raise KeyError("in done")

    with pytest.raises(KeyError, match="in done"):
        Column(Map(Select("File", FILES), fails)).headless("enter")
    with pytest.raises(ZeroDivisionError):
        Column(Counter()).on("x", "x", "x", lambda: 1 / 0).headless("x")

    def no_layer():
        raise OSError("no layer")

    with pytest.raises(OSError, match="no layer"):
        Layers(Counter()).open_on("open", "f2", "open", no_layer).headless("f2 up enter")


def test_a_bad_return_value_is_a_type_error():
    class Bad(Counter):
        def handle(self, event):
            return 3

    with pytest.raises(TypeError, match="Ignored"):
        Column(Bad()).headless("up")

    class BadKeys(Counter):
        def keymap(self):
            return ["up"]

    with pytest.raises(TypeError, match="Keymap or None"):
        interact.keymap(Column(BadKeys()))


# ---------------------------------------------------------------------------
# Keymaps


def test_keymaps_declare_and_answer():
    keymap = Keymap("demo").bind("down", "down ctrl+n", "move down").bind("done", ["enter"], "finish")
    assert keymap.context == "demo" and len(keymap) == 2
    assert keymap.action("ctrl+n") == "down"
    assert keymap.action("x") is None
    assert keymap.keys("down") == ["down", "ctrl+n"]
    keymap.rebind("down", "j")
    assert keymap.action("ctrl+n") is None and keymap.action("j") == "down"
    first = keymap.bindings[0]
    assert (first.context, first.action, first.keys, first.description) == ("demo", "down", ["j"], "move down")
    assert first.id == "demo.down" and first.keys_label == "j"
    assert Binding("demo", "down", "j", "move down") == first
    with pytest.raises(ValueError, match="unknown key name"):
        keymap.action("nope+nope")
    with pytest.raises(TypeError):
        keymap.action(3)


def test_a_python_components_keys_are_listed_by_its_container():
    split = Split(Counter(), Select("File", FILES))
    ids = [binding.id for binding in split.keymap().bindings]
    # The focused child's first, then the split's own.
    assert ids[:2] == ["counter.up", "counter.done"]
    assert "split.focus-next" in ids
    assert [binding.id for binding in interact.keymap(Counter()).bindings] == ["counter.up", "counter.done"]
