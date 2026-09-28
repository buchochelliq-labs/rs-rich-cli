"""rs_rich.interact: the rs-rich-interact components, headless.

Rich has no interactive components, so these compare with the Rust crate:
the frames and answers are the ones `crates/rich-interact/tests` expects for
the same scripts. The Python layer adds only values (any Python object per
item), outcomes as values and exceptions, and Python components.
"""

from __future__ import annotations

import os
import subprocess
import sys

import pytest

import rs_rich.interact as interact
from rs_rich import _native
from rs_rich.interact import (
    Action,
    Cancel,
    Cancelled,
    Choice,
    Confirm,
    Done,
    Form,
    Input,
    InteractError,
    Item,
    MultiSelect,
    NotInteractive,
    Pager,
    Script,
    Select,
    degrade,
    fuzzy,
    headless,
    rank,
    run,
)
from rs_rich.panel import Panel
from rs_rich.table import Table
from rs_rich.text import Text

FILES = ["src/lib.rs", "src/main.rs", "docs/maintenance.md", "Cargo.toml", "README.md"]


def before_answer(record):
    """The last view before the component collapsed to its answer."""
    return record.frames[-2]


# ---------------------------------------------------------------------------
# The module


def test_the_module_exports_native_objects():
    for name in interact.__all__:
        value = getattr(interact, name)
        assert any(getattr(_native, n) is value for n in dir(_native)), name


def test_classes_say_where_they_live():
    for name in ("Item", "Action", "Select", "MultiSelect", "Input", "Choice", "Confirm", "Form", "Pager",
                 "Event", "Done", "Cancel", "Script", "Outcome", "Record", "Match"):
        cls = getattr(interact, name)
        assert cls.__module__ == "rs_rich.interact", name
        assert cls.__name__ == name


def test_errors_share_a_base():
    assert issubclass(Cancelled, InteractError)
    assert issubclass(NotInteractive, InteractError)


# ---------------------------------------------------------------------------
# Select


def test_arrows_and_enter_pick():
    record = Select("Open", FILES).headless("down down enter", width=60, height=12)
    assert record.value == "docs/maintenance.md"
    assert record.outcome.kind == "done" and record.outcome.done
    first = record.frames[0]
    assert first.startswith("? Open › \n❯ src/lib.rs\n  src/main.rs")
    assert "5/5 · ↑↓ move · enter pick · esc cancel" in first
    assert record.last_frame == "? Open › docs/maintenance.md"


def test_values_are_python_objects():
    marker = object()
    values = [{"id": 1}, marker, ("t", 3)]
    record = Select("Pick", values).headless("down enter")
    assert record.value is marker
    record = Select("Pick", values).headless("enter")
    assert record.value is values[0]


def test_items_carry_labels_descriptions_and_values():
    items = [Item(10, "ten", description="a round number"), Item(11, "eleven")]
    record = Select("Number", items).headless(Script().text("elev").keys("enter"))
    assert record.value == 11
    assert Item(3).label == "3"
    item = Item([1], "one", metadata={"size": "2 KB"}, keywords=["uno"])
    assert (item.value, item.label, item.metadata, item.keywords) == ([1], "one", [("size", "2 KB")], ["uno"])


def test_keywords_match_without_showing():
    items = [Item("a", "short", keywords=["src/very/long/path.rs"]), Item("b", "other")]
    record = Select("Open", items).headless(Script().text("path").keys("enter"))
    assert record.value == "a"


def test_typing_filters_and_ranks():
    record = Select("Open", FILES).headless(Script().text("main").keys("enter"), width=60, height=12)
    assert record.value == "src/main.rs"
    filtered = before_answer(record)
    assert filtered.startswith("? Open › main\n❯ src/main.rs\n  docs/maintenance.md\n")
    assert "2/5" in filtered
    assert record.last_frame == "? Open › src/main.rs"


def test_highlights_matched_characters():
    record = Select("Open", FILES).headless(Script().text("mr").keys("enter"), width=60, height=12)
    assert "\x1b[1;35mm\x1b[0m" in record.output


def test_escape_cancels_and_ctrl_c_interrupts():
    record = Select("Open", FILES).headless("escape")
    assert record.outcome.kind == "cancelled" and record.outcome.cancelled
    assert record.value is None
    assert record.last_frame == "? Open › cancelled"
    assert record.output.endswith("\r\n\x1b[J\x1b[?25h")
    with pytest.raises(Cancelled):
        record.outcome.unwrap()
    record = Select("Open", FILES).headless("ctrl+c")
    assert record.outcome.interrupted
    with pytest.raises(KeyboardInterrupt):
        record.outcome.unwrap()


def test_a_script_that_runs_out_leaves_the_outcome_unset():
    record = Select("Open", FILES).headless("down")
    assert record.outcome is None and not record.finished and record.value is None
    assert "❯ src/main.rs" in record.last_frame


def test_default_focuses_first():
    record = Select("Open", FILES, default=3).headless("enter")
    assert record.value == "Cargo.toml"
    with pytest.raises(ValueError):
        Select("Open", FILES, default=9)


def test_query_starts_filtered_and_height_scrolls():
    assert Select("Open", FILES, query="readme").headless("enter").value == "README.md"
    record = Select("Number", list(range(1, 51)), height=5).headless("pagedown pagedown down enter", width=40)
    assert record.value == 12
    assert "❯ 12" in before_answer(record)


def test_actions_pick_and_are_reported():
    items = [Item("a.txt", actions=[Action("edit", "Open in $EDITOR", "ctrl+e")]), Item("b.txt")]
    record = Select("File", items).headless("ctrl+e")
    assert record.value == "a.txt"
    assert record.outcome.action == "edit"
    assert Select("File", items).headless("enter").outcome.action is None
    with pytest.raises(ValueError):
        Action("x", "X", "ctrl+nope")


def test_preview_renders_markup_and_python_renderables():
    class Custom:
        def __rich_console__(self, console, options):
            yield "from __rich_console__"

    items = [
        Item(1, "one", preview=Custom()),
        Item(2, "two", preview="[bold]second[/]"),
        Item(3, "three", preview=Panel("boxed", width=12)),
    ]
    record = Select("Pick", items).headless("down down", width=90, height=10)
    frames = "\n".join(record.frames)
    assert "│ from __rich_console__" in record.frames[0]
    assert "second" in frames
    assert "╭──────────╮" in record.last_frame
    record = Select("Pick", items, preview="hidden").headless("", width=90, height=10)
    assert "from __rich_console__" not in record.last_frame


def test_items_must_not_be_a_string():
    with pytest.raises(TypeError):
        Select("Pick", "abc")
    with pytest.raises(ValueError):
        Select("Pick", FILES, preview="left")


# ---------------------------------------------------------------------------
# MultiSelect


def test_tab_marks_and_enter_returns_marked_in_list_order():
    # Tab marks the focused item and moves down.
    record = MultiSelect("Pick", FILES).headless("down down tab up up up tab enter")
    assert record.value == ["src/lib.rs", "docs/maintenance.md"]
    assert record.last_frame == "? Pick › src/lib.rs, docs/maintenance.md"


def test_enter_without_marks_returns_the_focused_item():
    assert MultiSelect("Pick", [1, 2, 3]).headless("down enter").value == [2]


def test_marked_start_marked_and_are_the_default():
    assert MultiSelect("Pick", [1, 2, 3], marked=[0, 2]).headless("enter").value == [1, 3]
    assert degrade(MultiSelect("Pick", [1, 2, 3], marked=[1]), fallback="default").value == [2]
    with pytest.raises(ValueError):
        MultiSelect("Pick", [1], marked=[4])


# ---------------------------------------------------------------------------
# Input


def test_typing_and_enter():
    record = Input("Name", placeholder="Ada Lovelace").headless(Script().text("Bob").keys("enter"))
    assert record.value == "Bob"
    assert record.frames[0] == "? Name › Ada Lovelace"
    assert record.last_frame == "? Name › Bob"


def test_editing_keys():
    script = Script().text("hello world").keys("ctrl+w").text("there").keys("home").text(">").keys("enter")
    assert Input("Say").headless(script).value == ">hello there"


def test_default_answers_an_empty_line():
    assert Input("Name", default="zed").headless("enter").value == "zed"


def test_value_starts_typed():
    assert Input("Name", value="abc").headless(Script().keys("backspace enter")).value == "ab"


def test_password_masks():
    record = Input("Token", password=True).headless(Script().text("secret").keys("enter"))
    assert record.value == "secret"
    assert all("secret" not in frame for frame in record.frames)
    assert "••••••" in record.last_frame
    assert Input("Pin", mask="*").headless(Script().text("12").keys("enter")).last_frame == "? Pin › **"
    assert Input("Token", password=True).password
    with pytest.raises(ValueError):
        Input("Pin", mask="**")


def test_validation_shows_the_message_and_waits():
    def digits(text):
        if not text.isdigit():
            return "digits only"

    record = Input("Age", validate=digits).headless(Script().text("ab").keys("enter"))
    assert not record.finished
    assert record.last_frame == "? Age › ab\n  ✗ digits only"
    script = Script().text("ab").keys("enter backspace backspace").text("42").keys("enter")
    assert Input("Age", validate=digits).headless(script).value == "42"


def test_validators_may_raise_value_error_or_return_false():
    def raises(text):
        raise ValueError(f"no {text}")

    assert "✗ no x" in Input("A", validate=raises).headless(Script().text("x").keys("enter")).last_frame
    record = Input("A", validate=lambda text: len(text) > 1).headless(Script().text("x").keys("enter"))
    assert "✗ invalid value" in record.last_frame
    assert Input("A", validate=lambda text: True).headless(Script().text("x").keys("enter")).value == "x"


def test_other_exceptions_in_a_validator_are_raised():
    def broken(text):
        raise KeyError("bug")

    with pytest.raises(KeyError):
        Input("A", validate=broken).headless(Script().text("x").keys("enter"))
    with pytest.raises(TypeError):
        Input("A", validate=3)


def test_history_and_suggestions():
    assert Input("Cmd", history=["ls", "pwd"]).headless("up enter").value == "pwd"
    assert Input("Cmd", history=["ls", "pwd"]).headless("up up enter").value == "ls"
    record = Input("Lang", suggestions=["python", ("rust", "systems")]).headless(Script().text("ru").keys("tab enter"))
    assert record.value == "rust"
    assert any("systems" in frame for frame in record.frames)


# ---------------------------------------------------------------------------
# Confirm


def test_yes_and_no_keys():
    record = Confirm("Deploy?").headless("y")
    assert record.value == "yes"
    assert record.frames[0] == "? Deploy?\n   Yes   No  \n  y/n · ←→ move · enter choose · esc cancel"
    assert record.last_frame == "? Deploy? › Yes"
    assert Confirm("Deploy?").headless("n").value == "no"
    assert Confirm("Deploy?").headless("right enter").value == "no"
    assert Confirm("Deploy?", default="no").headless("enter").value == "no"


def test_choices_body_and_warnings():
    table = Table("file", "change")
    table.add_row("a.py", "+3")
    confirm = Confirm(
        "Apply?",
        body=[Text("2 files change"), table],
        warnings=["this cannot be undone"],
        choices=[Choice("apply", "Apply", "a"), ("skip", "Skip", "s"), Choice("cancel", "Cancel", "c")],
        default="skip",
    )
    record = confirm.headless("", width=50, height=15)
    frame = record.last_frame
    assert "2 files change" in frame and "a.py" in frame
    assert "this cannot be undone" in frame
    assert "Apply" in frame and "Skip" in frame
    assert confirm.headless("a").value == "apply"
    assert confirm.headless("enter").value == "skip"
    assert [c.id for c in confirm.choices] == ["apply", "skip", "cancel"]
    assert [c.key for c in Confirm().choices] == ["y", "n"]
    with pytest.raises(ValueError):
        Confirm("x", default="maybe")
    with pytest.raises(ValueError):
        Confirm("x", choices=[])


# ---------------------------------------------------------------------------
# Form


def form():
    return (
        Form("Sign up")
        .text("name", "Name", placeholder="Ada")
        .masked("password", "Password")
        .choice("plan", "Plan", ["free", "pro"])
        .toggle("news", "Newsletter")
    )


def test_form_answers():
    script = Script().text("Ada").keys("enter").text("pw").keys("enter right enter space enter")
    record = form().headless(script)
    assert record.value == {"name": "Ada", "password": "pw", "plan": "pro", "news": True}
    assert list(record.value) == ["name", "password", "plan", "news"]
    assert record.last_frame == "? Sign up\n  Name        Ada\n  Password    ••\n  Plan        pro\n  Newsletter  yes"
    assert form().fields == ["name", "password", "plan", "news"]


def test_form_validation_focuses_the_failing_field():
    f = Form("Account").text("user", "User", validate=lambda text: None if text else "required").toggle("admin", "Admin")
    record = f.headless("enter enter")
    assert not record.finished
    assert "required" in record.last_frame
    assert f.headless(Script().keys("enter enter").text("root").keys("enter enter")).value == {"user": "root", "admin": False}


def test_form_fields_from_inputs_and_unique_names():
    f = Form("F").input("n", Input("Number", default="7"))
    assert f.headless("enter").value == {"n": "7"}
    with pytest.raises(ValueError):
        Form("F").toggle("a", "A").toggle("a", "B")
    assert Form("Empty").headless("").value == {}


# ---------------------------------------------------------------------------
# Pager


def test_pager_scrolls_searches_and_quits():
    lines = "\n".join(f"line {i} needle" if i % 10 == 0 else f"line {i}" for i in range(40))
    record = Pager(Text(lines)).headless("", width=30, height=6)
    assert record.last_frame.startswith("line 0 needle\nline 1\n")
    record = Pager(Text(lines)).headless(Script().keys("/").text("needle").keys("enter n q"), width=30, height=6)
    assert record.outcome.done and record.value is None
    # Closing paints nothing more: the last frame is the view after `n`.
    assert record.last_frame.startswith("line 6\n")
    assert "line 10 needle" in record.last_frame and "match 2/4" in record.last_frame
    record = Pager(Text(lines), search="needle").headless("n n q", width=30, height=6)
    assert "match 3/4" in record.last_frame


def test_pager_pages_any_renderable():
    record = Pager(Panel("inside a panel")).headless("q", width=30, height=6)
    assert "│ inside a panel" in record.frames[0]


# ---------------------------------------------------------------------------
# Python components


class Counter:
    """Counts Up presses; Enter returns the count, Escape cancels."""

    def __init__(self):
        self.count = 0
        self.events = []

    def handle(self, event):
        self.events.append((event.kind, event.key))
        if event.key == "up":
            self.count += 1
        elif event.key == "enter":
            return Done(self.count)
        elif event.key == "escape":
            return Cancel()
        return None

    def render(self, width, height):
        return f"count: [bold]{self.count}[/]"

    def default_value(self):
        return -1


def test_a_python_component_runs_headless():
    counter = Counter()
    record = headless(counter, "up up enter", width=40, height=5)
    assert record.value == 2
    assert record.last_frame == "count: 2"
    assert counter.events == [("key", "up"), ("key", "up"), ("key", "enter")]
    assert headless(Counter(), "escape").outcome.cancelled
    assert degrade(Counter(), fallback="default").value == -1
    assert degrade(Counter(), fallback="prompt").value == -1


def test_a_python_component_sees_pastes_and_resizes():
    counter = Counter()
    headless(counter, Script().paste("hi").resize(30, 4).keys("enter"))
    assert counter.events[:2] == [("paste", None), ("resize", None)]


def test_errors_in_a_python_component_are_raised():
    class Broken(Counter):
        def handle(self, event):
            raise RuntimeError("boom")

    with pytest.raises(RuntimeError, match="boom"):
        headless(Broken(), "up")

    class Bad(Counter):
        def handle(self, event):
            return 3

    with pytest.raises(TypeError, match="Done"):
        headless(Bad(), "up")

    class Unrenderable(Counter):
        def render(self, width, height):
            raise LookupError("no view")

    # A failed render ends the run at the next event; the error is raised.
    with pytest.raises(LookupError, match="no view"):
        headless(Unrenderable(), "up up up")
    with pytest.raises(TypeError):
        headless(object(), "")


# ---------------------------------------------------------------------------
# Scripts


def test_scripts():
    script = Script("down").text("ab").keys("enter").wait(0.5).paste("x").resize(10, 5)
    assert len(script) == 7
    with pytest.raises(ValueError):
        Script("nosuchkey")
    with pytest.raises(ValueError):
        Script().keys("ctrl+")
    with pytest.raises(TypeError):
        headless(Select("x", [1]), 3)


# ---------------------------------------------------------------------------
# Without a terminal


def test_fallback_default_returns_the_default():
    assert degrade(Select("Pick", ["a", "b"], default=1), fallback="default").value == "b"
    assert degrade(Input("Name", default="zed"), fallback="default").value == "zed"
    assert degrade(Confirm("Go?", default="yes"), fallback="default").value == "yes"
    assert degrade(Pager("x"), fallback="default").value is None


def test_fallback_default_without_a_default_raises():
    with pytest.raises(NotInteractive) as raised:
        degrade(Select("Pick", ["a", "b"]), fallback="default", reason="ci")
    assert raised.value.reason == "ci"
    assert "running under CI" in str(raised.value)


def test_fallback_error_raises():
    with pytest.raises(NotInteractive) as raised:
        degrade(Input("Name", default="x"), fallback="error", reason="stdin_not_terminal")
    assert raised.value.reason == "stdin_not_terminal"
    with pytest.raises(ValueError):
        degrade(Input("Name"), reason="nope")


def test_fallback_prompt_asks_line_by_line():
    record = degrade(Select("Pick", ["a", "b"]), ["2"])
    assert record.value == "b"
    assert record.prompts == "Pick\n  1) a\n  2) b\nNumber or name: "
    record = degrade(Input("Token", password=True), ["s3cret"])
    assert (record.value, record.secrets, record.prompts) == ("s3cret", 1, "Token: ")
    assert degrade(Confirm("Go?"), ["y"]).value == "yes"
    assert degrade(Confirm("Go?"), ["n"]).value == "no"
    assert degrade(MultiSelect("Pick", [1, 2, 3]), ["1,3"]).value == [1, 3]
    # The end of input takes the default; without one it is no answer.
    assert degrade(Input("Name", default="d"), []).value == "d"
    with pytest.raises(NotInteractive):
        degrade(Input("Name"), [])


def test_form_prompts_each_field():
    record = degrade(form(), ["Ada", "pw", "pro", "y"])
    assert record.value == {"name": "Ada", "password": "pw", "plan": "pro", "news": True}
    assert record.secrets == 1
    assert "Plan: free / pro [free]: " in record.prompts


def test_run_without_a_terminal_follows_the_fallback():
    # pytest's stdin is not a terminal, so no session starts.
    assert run(Select("Pick", ["a", "b"], default=0), fallback="default").value == "a"
    assert Select("Pick", ["a", "b"], default=1).ask(fallback="default") == "b"
    assert interact.ask(Input("x", default="d"), interactive=False, fallback="default") == "d"
    with pytest.raises(NotInteractive) as raised:
        run(Select("Pick", ["a"]), fallback="error", interactive=False)
    assert raised.value.reason == "requested"
    with pytest.raises(ValueError):
        run(Select("Pick", ["a"]), fallback="nope")
    with pytest.raises(ValueError):
        run(Select("Pick", ["a"]), output="file")


def test_ask_prompts_on_stdin_without_a_terminal():
    program = (
        "from rs_rich.interact import Select, Confirm, Input, NotInteractive\n"
        "print(repr(Select('Pick', [10, 20, 30]).ask()))\n"
        "print(Confirm('Sure?').ask())\n"
        "try:\n"
        "    Input('Name').ask()\n"
        "except NotInteractive:\n"
        "    print('no answer')\n"
    )
    env = {k: v for k, v in os.environ.items() if k != "CI"}
    result = subprocess.run(
        [sys.executable, "-c", program], input=b"3\nn\n", capture_output=True, env=env, timeout=60
    )
    assert result.returncode == 0, result.stderr
    assert result.stdout == b"30\nno\nno answer\n"
    assert b"Number or name: " in result.stderr and b"Sure? [y=Yes, n=No]: " in result.stderr


def test_a_fallback_render_is_not_a_terminal():
    # Without a terminal the pager writes its content out: renderables that
    # branch on the terminal must see a file, as they would printing to one.
    program = (
        "from rs_rich.interact import Pager, run\n"
        "class Probe:\n"
        "    def __rich_console__(self, console, options):\n"
        "        yield 'terminal' if options.is_terminal else 'file'\n"
        "run(Pager(Probe()), interactive=False)\n"
        "run(Pager(Probe()))\n"
    )
    env = {k: v for k, v in os.environ.items() if k != "CI"}
    result = subprocess.run(
        [sys.executable, "-c", program], capture_output=True, env=env, timeout=60
    )
    assert result.returncode == 0, result.stderr
    # The line fallback writes to stderr.
    assert (result.stdout + result.stderr).split() == [b"file", b"file"]


def test_reference_cycles_through_python_values_are_collected():
    import gc
    import weakref

    class Box:
        pass

    def cycle(make):
        box = Box()
        box.owner = make(box)
        return weakref.ref(box)

    made = [
        lambda box: Item(box),
        lambda box: Item("x", preview=box),
        lambda box: Select("Pick", [Item(box)]),
        lambda box: MultiSelect("Pick", [Item(box)]),
        lambda box: Input("x", validate=lambda text, box=box: None),
        lambda box: Pager(box),
        lambda box: Done(box),
        lambda box: Form("f").input("name", Input("x", validate=lambda text, box=box: None)),
    ]
    refs = [cycle(make) for make in made]
    gc.collect()
    assert [ref() for ref in refs] == [None] * len(made)


# ---------------------------------------------------------------------------
# Fuzzy matching


def test_fuzzy_prefers_word_starts():
    found = fuzzy("fb", "foo_bar")
    assert found.positions == [0, 4] and found.score > 0
    assert fuzzy("xyz", "foo_bar") is None
    assert fuzzy("", "anything").score == 0


def test_fuzzy_smart_case_and_terms():
    assert fuzzy("foo", "FOO") is not None
    assert fuzzy("Foo", "foo") is None
    assert fuzzy("bar foo", "foo_bar") is not None


def test_rank_orders_best_first():
    ranked = rank("main", FILES)
    assert [FILES[index] for index, _ in ranked] == ["src/main.rs", "docs/maintenance.md"]
    ranked = rank("fb", ["foo_bar", "fizzbuzz", "xfb"])
    assert [index for index, _ in ranked] == [0, 2, 1]
    assert [index for index, _ in rank("", ["b", "a"])] == [0, 1]
    assert ranked[0][1] == fuzzy("fb", "foo_bar")


# ---------------------------------------------------------------------------
# Release audit: Python code that raises, nests runs, or passes iterables


def test_a_raising_validator_ends_the_run_at_once():
    calls = []

    def validate(text):
        calls.append(text)
        raise KeyboardInterrupt

    script = Script().text("a").keys("enter").text("b").keys("enter").text("c").keys("enter")
    with pytest.raises(KeyboardInterrupt):
        Input("p", validate=validate).headless(script)
    assert calls == ["a"]


def test_a_raising_form_validator_ends_the_run_at_once():
    calls = []

    def validate(text):
        calls.append(text)
        raise LookupError("bug")

    form = Form("f").text("a", "A", validate=validate)
    with pytest.raises(LookupError):
        form.headless(Script().text("x").keys("enter").text("y").keys("enter"))
    assert calls == ["x"]


def test_a_raising_preview_ends_the_run_at_the_next_key():
    class Preview:
        def __init__(self):
            self.renders = 0

        def __rich_console__(self, console, options):
            self.renders += 1
            raise RuntimeError("preview")
            yield  # pragma: no cover

    preview = Preview()

    select = Select("p", [Item(1, preview=preview), Item(2)])
    with pytest.raises(RuntimeError, match="preview"):
        select.headless("down up down up down up")
    assert preview.renders == 1


def test_a_raising_confirm_body_ends_the_run():
    class Body:
        def __rich_console__(self, console, options):
            raise RuntimeError("body")
            yield  # pragma: no cover

    with pytest.raises(RuntimeError, match="body"):
        Confirm(body=Body()).headless("left right left right")


def test_a_raising_validator_ends_a_degraded_form():
    calls = []

    def validate(text):
        calls.append(text)
        raise LookupError("bug")

    form = Form("f").text("a", "A", validate=validate).text("b", "B")
    with pytest.raises(LookupError):
        degrade(form, ["x", "y", "z"])
    assert calls == ["x"]


def test_nested_runs_raise_recursion_error_instead_of_crashing():
    # Once a native stack overflow (SIGSEGV): run it where a crash cannot
    # take pytest down, on a small thread stack.
    result = subprocess.run(
        [
            sys.executable,
            "-c",
            "import threading\n"
            "from rs_rich import interact as I\n"
            "def validate(text):\n"
            "    I.headless(I.Input('p', validate=validate), 'a enter')\n"
            "def run():\n"
            "    try:\n"
            "        I.headless(I.Input('p', validate=validate), 'a enter')\n"
            "        print('finished')\n"
            "    except RecursionError:\n"
            "        print('RecursionError')\n"
            "threading.stack_size(1024 * 1024)\n"
            "t = threading.Thread(target=run)\n"
            "t.start()\n"
            "t.join()\n",
        ],
        capture_output=True,
        text=True,
        timeout=120,
    )
    assert (result.returncode, result.stdout) == (0, "RecursionError\n"), result.stderr


def test_nested_runs_are_capped_on_a_large_stack():
    result = subprocess.run(
        [
            sys.executable,
            "-c",
            "import sys\n"
            "from rs_rich import interact as I\n"
            "sys.setrecursionlimit(100000)\n"
            "def validate(text):\n"
            "    I.headless(I.Input('p', validate=validate), 'a enter')\n"
            "try:\n"
            "    I.headless(I.Input('p', validate=validate), 'a enter')\n"
            "except RecursionError:\n"
            "    print('RecursionError')\n"
            "# The thread's count is back to zero: a plain run still works.\n"
            "print(I.headless(I.Input('p'), 'a enter').value)\n",
        ],
        capture_output=True,
        text=True,
        timeout=120,
    )
    assert (result.returncode, result.stdout) == (0, "RecursionError\na\n"), result.stderr


def test_iterable_arguments_accept_any_iterable():
    assert degrade(Input("p"), (answer for answer in ["hi"])).value == "hi"
    assert MultiSelect("p", [1, 2, 3], marked={1}).marked == [1]
    assert MultiSelect("p", [1, 2, 3], marked=iter([0, 2])).marked == [0, 2]
    assert Input("p", history=(h for h in ["a", "b"])).history == ["a", "b"]
    item = Item(1, keywords=(k for k in "ab"), actions={Action("x", "X", "ctrl+x")})
    assert item.keywords == ["a", "b"]
    assert [action.id for action in item.actions] == ["x"]
    assert Confirm(warnings=(w for w in ["careful"])).warnings == ["careful"]
    for build in (
        lambda: Input("p", history="abc"),
        lambda: Item(1, keywords="ab"),
        lambda: Confirm(warnings="w"),
        lambda: degrade(Input("p"), "hi"),
    ):
        with pytest.raises(TypeError):
            build()
    with pytest.raises(TypeError):
        Item(1, actions=["not an action"])


def test_not_interactive_has_a_reason_even_when_raised_by_hand():
    assert NotInteractive("x").reason is None
    with pytest.raises(NotInteractive) as raised:
        interact.run(Input("p"), interactive=False, fallback="error")
    assert raised.value.reason == "requested"


def test_run_refuses_a_height_of_zero():
    with pytest.raises(ValueError, match="height"):
        interact.run(Input("p"), interactive=False, height=0)


def test_fuzzy_matching_releases_the_gil():
    import threading
    import time

    # Big enough that ranking takes a while on this build.
    candidates = ["x" * 2000 + "abc"] * 50
    while True:
        started = time.perf_counter()
        rank("abc", candidates)
        took = time.perf_counter() - started
        if took >= 0.1 or len(candidates) >= 50 * 2**8:
            break
        candidates *= 2
    # While another thread ranks, this one keeps running: the longest gap
    # between its samples is far shorter than the ranking.
    switch = sys.getswitchinterval()
    sys.setswitchinterval(0.001)
    try:
        done = threading.Event()
        ranked = []
        thread = threading.Thread(target=lambda: (ranked.append(rank("abc", candidates)), done.set()))
        samples = [time.perf_counter()]
        thread.start()
        while not done.is_set():
            samples.append(time.perf_counter())
        thread.join()
    finally:
        sys.setswitchinterval(switch)
    longest = max(later - earlier for earlier, later in zip(samples, samples[1:]))
    assert len(ranked[0]) == len(candidates)
    assert longest < took / 2, (longest, took)
    assert fuzzy("abc", candidates[0]) is not None


# ---------------------------------------------------------------------------
# TextArea, FilePicker, ColorPicker and AssetPicker (#493)


def test_text_area_takes_several_lines():
    from rs_rich.interact import TextArea

    record = TextArea("Notes").headless(Script().text("one").keys("enter").text("two").keys("ctrl+d"))
    assert record.value == "one\ntwo"
    assert "│ one\n│ two" in record.frames[-2]
    area = TextArea("Bio", placeholder="Say something", char_limit=4, submit="ctrl+s")
    assert area.submit == "ctrl+s" and area.char_limit == 4
    assert TextArea("Bio", char_limit=4).headless(Script().text("abcdef").keys("ctrl+d")).value == "abcd"
    assert degrade(TextArea("Notes"), ["a", "b"]).value == "a\nb"
    with pytest.raises(ValueError):
        TextArea(submit="hyper+x")
    with pytest.raises(ValueError):
        TextArea(char_limit=0)


def test_file_picker_returns_a_path(tmp_path):
    import pathlib

    from rs_rich.interact import FilePicker

    (tmp_path / "notes.md").write_text("# notes\n")
    (tmp_path / "sub").mkdir()
    (tmp_path / ".hidden").write_text("x")
    picked = FilePicker(tmp_path).headless(Script().text("notes").keys("enter")).value
    assert picked == pathlib.Path(tmp_path) / "notes.md"
    assert FilePicker(tmp_path, mode="directory").headless("enter").value == tmp_path / "sub"
    shown = FilePicker(tmp_path, hidden=True).headless("esc").frames[0]
    assert ".hidden" in shown
    assert degrade(FilePicker(tmp_path, default="x.txt"), fallback="default").value == pathlib.Path("x.txt")
    with pytest.raises(NotInteractive):
        degrade(FilePicker(tmp_path))
    with pytest.raises(ValueError):
        FilePicker(tmp_path, mode="socket")


def test_color_picker_formats_the_colour():
    from rs_rich.interact import ColorPicker

    assert ColorPicker(format="name").headless(Script().text("dark_orange").keys("enter")).value == "dark_orange"
    assert ColorPicker(format="rgb").headless(Script().text("#ff8800").keys("enter")).value == "rgb(255,136,0)"
    # Tab to the palette: right twice and down once is color(18).
    assert ColorPicker(format="name").headless("tab right right down enter").value == "dark_blue"
    assert degrade(ColorPicker(), ["rgb(1,2,3)"]).value == "#010203"
    with pytest.raises(ValueError):
        ColorPicker(format="hsl")


def test_asset_picker_picks_emoji_boxes_and_spinners():
    from rs_rich.interact import AssetPicker

    assert AssetPicker().headless(Script().text("thumbs_up").keys("enter")).value == "👍"
    assert AssetPicker("box").headless(Script().text("double_edge").keys("enter")).value == "double_edge"
    assert AssetPicker("spinner", default="dots").headless("enter").value == "dots"
    assert degrade(AssetPicker(), [":rocket:"]).value == "🚀"
    with pytest.raises(ValueError):
        AssetPicker("icon")


def test_scripts_click_and_python_components_see_mouse_events():
    events = []

    class Clicks:
        def handle(self, event):
            if event.kind == "key":
                return Done(list(events))
            events.append((event.kind, event.mouse, event.column, event.row))
            return None

        def render(self, width, height):
            return "click me"

    record = headless(Clicks(), Script().click(3, 0).scroll(1, 0).keys("q"))
    assert record.value == [
        ("mouse", "down", 3, 0),
        ("mouse", "up", 3, 0),
        ("mouse", "scroll_down", 1, 0),
    ]
    assert len(Script().drag((0, 0), (3, 0))) == 5
