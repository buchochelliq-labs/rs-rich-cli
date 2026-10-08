"""The intuiTUIve examples ported to Python (crates/rich-py/examples/tui),
driven like a person would, against what the Rust examples' tests expect
(crates/rich-intuituive/tests/todo.rs and planner.rs)."""

from __future__ import annotations

import importlib.util
import sys
from pathlib import Path

import pytest

from rs_rich.tui import Script, run

EXAMPLES = Path(__file__).resolve().parent.parent / "examples" / "tui"


def example(name):
    sys.path.insert(0, str(EXAMPLES))
    try:
        spec = importlib.util.spec_from_file_location(f"tui_example_{name}", EXAMPLES / f"{name}.py")
        module = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(module)
        return module
    finally:
        sys.path.remove(str(EXAMPLES))


def screen(ran):
    return [line.rstrip() for line in ran.last_frame.splitlines()]


def row_of(rows, needle):
    return next((i for i, line in enumerate(rows) if needle in line), None)


# ---------------------------------------------------------------------------
# The counter


def test_the_counter_counts():
    counter = example("counter")
    rows = screen(run(counter.counter_app(), "+ + + - q", width=40, height=6))
    assert row_of(rows, "Count: 2") is not None, rows


def test_the_serve_example_imports():
    # It serves only when run as a program.
    assert example("serve").counter_app


# ---------------------------------------------------------------------------
# The to-do app


@pytest.fixture(scope="module")
def todo():
    return example("todo")


def test_typing_and_enter_adds_a_todo_and_the_entry_clears(todo):
    script = Script().text("milk").keys("enter").text("eggs").keys("enter ctrl+q")
    rows = screen(run(todo.todo_app([]), script, width=50, height=12))
    assert row_of(rows, "• milk") == 4, rows
    assert row_of(rows, "• eggs") == 5, rows
    assert "eggs" not in rows[1], rows
    assert row_of(rows, "2 left") is not None, rows


def test_space_ticks_off_the_focused_todo(todo):
    start = [todo.Todo(1, "milk"), todo.Todo(2, "eggs")]
    rows = screen(run(todo.todo_app(start), "tab tab space ctrl+q", width=50, height=12))
    assert "• milk" in rows[4], rows
    assert "✓ eggs" in rows[5], rows
    assert row_of(rows, "1 left") is not None, rows


def test_d_asks_before_deleting(todo):
    start = [todo.Todo(1, "milk"), todo.Todo(2, "eggs")]
    rows = screen(run(todo.todo_app(start), "tab d n ctrl+q", width=50, height=12))
    assert row_of(rows, "milk") is not None, rows
    rows = screen(run(todo.todo_app(start), "tab d y ctrl+q", width=50, height=12))
    assert row_of(rows, "milk") is None, rows
    assert row_of(rows, "• eggs") == 4, rows


def test_ticking_a_todo_draws_only_its_row_and_the_footer(todo):
    start = [todo.Todo(1, "milk"), todo.Todo(2, "eggs"), todo.Todo(3, "tea")]
    app = todo.todo_app(start).inspector(True)
    rows = screen(run(app, "tab space ctrl+q", width=100, height=14))
    assert "drew 2 of" in "\n".join(rows), rows


# ---------------------------------------------------------------------------
# The planner


@pytest.fixture(scope="module")
def planner():
    module = example("planner")

    def drive(script=""):
        ran = run(module.planner_app(), script, width=90, height=24)
        assert not ran.finished
        return screen(ran)

    return drive


def test_it_shows_the_projects_the_due_date_and_the_end_of_the_log(planner):
    rows = planner()
    assert rows[0].startswith(" File  Task  Help"), rows
    for item in ["▾ intuiTUIve 2", "✓ Widget trait v2", "· Water the plants"]:
        assert row_of(rows, item) is not None, (item, rows)
    assert row_of(rows, "October 2026") is not None, rows
    assert "Examples" in rows[2], rows
    assert row_of(rows, "#99999") is not None, rows
    assert "5 open · 100000 log rows" in rows[23], rows


def test_space_marks_the_task_done_with_a_toast_and_a_log_row(planner):
    rows = planner("space")
    assert "4 open · 100001 log rows" in rows[23], rows
    assert row_of(rows, "✓ Examples") is not None, rows
    assert row_of(rows, "Examples done") is not None, rows


def test_picking_a_day_on_the_calendar_moves_the_due_date(planner):
    rows = planner()
    at = row_of(rows, "12 13 14 15")
    x = len(rows[at][: rows[at].index("15")])
    rows = planner(Script().click(x, at))
    assert row_of(rows, "due date of Examples moved to 15 October") is not None, rows


def test_n_adds_a_task_to_the_project_and_selects_it(planner):
    rows = planner("n")
    assert row_of(rows, "· New task 5") is not None, rows
    assert "6 open" in rows[23], rows
    assert row_of(rows, "Added to intuiTUIve") is not None, rows


def test_f10_opens_the_menus_and_the_help_lists_the_bindings(planner):
    rows = planner("f10 enter")
    assert row_of(rows, "New task") is not None, rows
    assert row_of(rows, "Quit") is not None, rows
    rows = planner("?")
    assert row_of(rows, "add a task to the project") is not None, rows
