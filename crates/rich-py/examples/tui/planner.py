"""A project planner, in Python: projects and their tasks in a tree, a
task's due date on a calendar, and a long activity log, under a menu bar.

    python crates/rich-py/examples/tui/planner.py

Tab moves between the tree, the calendar and the log · Space marks the
selected task done · n adds a task to its project · the calendar's arrows
move the due date · F10 opens the menus · ? shows the keys · Ctrl+P finds a
command · q quits. The divider between the tree and the rest, and the one
above the log, drag.

A port of crates/rich-intuituive/examples/planner.rs: a `menu_bar`, a
`tree_with` of the projects, `hsplit` and `vsplit` panes, a `calendar_with`
for the due date, and a `virtual_list` of a hundred thousand log rows that
draws only the rows in view. The help and the command palette are made from
the bindings' descriptions, and changes raise toasts.
"""

import copy
import datetime
from dataclasses import dataclass, field

from rs_rich.markup import escape
from rs_rich.tui import (
    App,
    Menu,
    MenuItem,
    TreeItem,
    calendar_with,
    column,
    hsplit,
    menu_bar,
    row,
    signal,
    text,
    tree_with,
    virtual_list,
    vsplit,
    watch,
)


@dataclass
class Task:
    title: str
    due: datetime.date
    done: bool = False


@dataclass
class Project:
    name: str
    tasks: list = field(default_factory=list)


def task(title, month, day, done):
    return Task(title, datetime.date(2026, month, day), done)


def sample_projects():
    """The projects the planner starts with."""
    return [
        Project(
            "intuiTUIve",
            [
                task("Widget trait v2", 10, 6, True),
                task("Owning the loop", 10, 7, True),
                task("Examples", 10, 8, False),
                task("Release 0.0.2", 10, 9, False),
            ],
        ),
        Project("rs-rich", [task("Sync upstream 15.1", 10, 20, False), task("Golden fixtures", 10, 21, False)]),
        Project("Home", [task("Water the plants", 10, 8, False)]),
    ]


# How many rows the log had before the planner opened.
HISTORY = 100_000


def history_row(i):
    """Row `i` of the log from before the planner opened: the same every run."""
    what = ["synced", "built", "tested", "deployed", "reviewed"][i % 5]
    day = 1 + (i // 3_000) % 28
    time = (i * 37) % 1440
    return f"[muted]2026-09-{day:02} {time // 60:02}:{time % 60:02}[/] {what} [dim]#{i}[/]"


def at(path):
    """The project and task a tree path points at."""
    return (path[0] if path else 0, path[1] if len(path) > 1 else None)


def build():
    projects = signal(sample_projects())
    selected = signal([0, 2])
    expanded = signal({(0,), (1,), (2,)})
    due = signal(datetime.date(2026, 10, 8))
    events = signal([])
    log_selected = signal(HISTORY - 1)

    def log(line):
        """Write `line` to the log and show its newest row."""
        events.update(lambda e: e.append(line))
        log_selected.set(HISTORY + len(events.get_untracked()) - 1)

    def selected_task(path):
        p, t = at(path)
        ps = projects.get_untracked()
        if t is None or p >= len(ps) or t >= len(ps[p].tasks):
            return None
        return ps[p].tasks[t]

    # The calendar follows the selected task; moving the date on the
    # calendar moves the task's due date.
    def task_due():
        p, t = at(selected.get())
        ps = projects.get()
        if t is None or p >= len(ps) or t >= len(ps[p].tasks):
            return None
        return ps[p].tasks[t].due

    def follow(date, cx):
        if date is not None:
            due.set(date)

    watch(task_due, follow)

    def move(date, cx):
        current = selected_task(selected.get_untracked())
        if current is None or current.due == date:
            return
        p, t = at(selected.get_untracked())

        def change(ps):
            ps = copy.deepcopy(ps)
            ps[p].tasks[t].due = date
            return ps

        projects.update(change)
        log(f"due date of [b]{escape(current.title)}[/] moved to {date.day} {date.strftime('%B')}")

    watch(lambda: due.get(), move)

    def toggle(cx):
        """Mark the selected task done or not."""
        p, t = at(selected.get_untracked())
        if t is None:
            return

        def change(ps):
            ps = copy.deepcopy(ps)
            ps[p].tasks[t].done = not ps[p].tasks[t].done
            return ps

        projects.update(change)
        current = projects.get_untracked()[p].tasks[t]
        what = "done" if current.done else "open again"
        log(f"[b]{escape(current.title)}[/] {what}")
        cx.toast(f"[green]✓[/] {escape(current.title)} {what}")

    def add(cx):
        """A task added to the selected project, a week after the date shown."""
        p, _ = at(selected.get_untracked())
        date = due.get_untracked() + datetime.timedelta(days=7)
        count = len(projects.get_untracked()[p].tasks)

        def change(ps):
            ps = copy.deepcopy(ps)
            ps[p].tasks.append(Task(f"New task {count + 1}", date))
            return ps

        projects.update(change)
        expanded.update(lambda e: e | {(p,)})
        selected.set([p, count])
        name = projects.get_untracked()[p].name
        log(f"task added to [b]{escape(name)}[/]")
        cx.toast(f"Added to {escape(name)}")

    bar = menu_bar([
        Menu("File", [MenuItem("New task", add).hint("n"), MenuItem.separator(), MenuItem("Quit", lambda cx: cx.quit()).hint("q")]),
        Menu("Task", [MenuItem("Done / open again", toggle).hint("space")]),
        Menu("Help", [MenuItem("Keys", lambda cx: cx.help()).hint("?"), MenuItem("Commands", lambda cx: cx.palette()).hint("ctrl+p")]),
    ])
    bar_id = bar.id

    def items():
        out = []
        for project in projects.get():
            open_ = sum(1 for t in project.tasks if not t.done)
            out.append(
                TreeItem(f"[b]{escape(project.name)}[/] [muted]{open_}[/]").children(
                    TreeItem(f"{'[green]✓[/]' if t.done else '[dim]·[/]'} {escape(t.title)}")
                    for t in project.tasks
                )
            )
        return out

    # The tree has the focus when the planner opens, not the menu bar above it.
    projects_tree = tree_with(items, selected, expanded).autofocus().panel("Projects")

    def details():
        p, t = at(selected.get())
        ps = projects.get()
        if p >= len(ps):
            return ""
        project = ps[p]
        if t is not None and t < len(project.tasks):
            current = project.tasks[t]
            state = "[green]done[/]" if current.done else "[yellow]open[/]"
            return f"[b]{escape(current.title)}[/]\n[muted]{escape(project.name)}[/]\n\n{state}"
        open_ = sum(1 for t in project.tasks if not t.done)
        return f"[b]{escape(project.name)}[/]\n[muted]{len(project.tasks)} tasks, {open_} open[/]"

    due_panel = row([calendar_with(due, None).fixed(22), text(details).padding(0, 1).flex(1)]).panel("Due")

    def activity_row(i):
        return history_row(i) if i < HISTORY else events.get()[i - HISTORY]

    activity = virtual_list(lambda: HISTORY + len(events.get()), activity_row, log_selected).panel("Activity")

    right = vsplit(due_panel, activity, signal(0.55))
    body = hsplit(projects_tree, right, signal(0.34))

    def status():
        open_ = sum(1 for p in projects.get() for t in p.tasks if not t.done)
        rows = HISTORY + len(events.get())
        return f"[muted]{open_} open · {rows} log rows[/]  [dim]? keys · ctrl+p commands · f10 menu[/]"

    return (
        column([bar.fixed(1), body.flex(1), text(status).fixed(1)])
        .bind("space", "mark the task done or open", toggle)
        .bind("n", "add a task to the project", add)
        .bind("f10", "open the menus", lambda cx: cx.focus(bar_id))
        .bind("q", "quit", lambda cx: cx.quit())
    )


def planner_app():
    return App(build).help_key("?").palette_key("ctrl+p")


if __name__ == "__main__":
    planner_app().run()
