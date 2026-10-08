"""The to-do app the intuiTUIve tutorial builds, in Python.

    python crates/rich-py/examples/tui/todo.py

Type a to-do and press Enter to add it. Tab moves to the list, where Space
ticks a to-do off and d deletes it (after asking). Ctrl+Q quits.

A port of crates/rich-intuituive/examples/todo.rs, line for line: the
state is one signal holding the to-dos, each row reads its own to-do
through a memo, so ticking one redraws that row and the footer only.
"""

from dataclasses import dataclass, replace

from rs_rich.interact import Input
from rs_rich.markup import escape
from rs_rich.tui import App, Size, column, each, label, memo, repeating, signal, text


@dataclass(frozen=True)
class Todo:
    """One to-do."""

    id: int
    title: str
    done: bool = False


def todo_app(start):
    """The app, starting with the to-dos in `start`."""

    def build():
        todos = signal(list(start))
        left = memo(lambda: sum(1 for todo in todos.get() if not todo.done))

        def add(title, cx):
            title = title.strip()
            if title:
                todos.update(
                    lambda ts: ts + [Todo(max((t.id for t in ts), default=0) + 1, title)]
                )

        entry = repeating(lambda: Input("Add"), add)
        rows = each(
            lambda: [todo.id for todo in todos.get()],
            lambda id: todo_row(todos, id),
        )
        return column([
            entry.panel("New").fixed(3),
            rows.panel("To do"),
            text(lambda: f"[muted]{left.get()} left · space ticks · d deletes · ctrl+q quits").auto(),
        ]).on_key("ctrl+q", lambda cx: cx.quit())

    return App(build)


def todo_row(todos, id):
    """One row of the list: the to-do with `id`."""
    todo = memo(lambda: next((t for t in todos.get() if t.id == id), None))

    def show():
        current = todo.get()
        if current is None:
            return ""
        if current.done:
            return f"[good]✓[/] [muted strike]{escape(current.title)}[/]"
        return f"[accent]•[/] {escape(current.title)}"

    def tick(cx):
        todos.update(lambda ts: [replace(t, done=not t.done) if t.id == id else t for t in ts])

    return (
        text(show)
        .focus_style("reverse")
        .on_key("space", tick)
        .on_key("d", lambda cx: cx.modal(Size.Auto, Size.Auto, lambda: confirm_delete(todos, id)))
    )


def confirm_delete(todos, id):
    """Ask before deleting the to-do with `id`."""
    title = next((t.title for t in todos.get_untracked() if t.id == id), "")

    def delete(cx):
        todos.update(lambda ts: [t for t in ts if t.id != id])
        cx.pop()

    return (
        label(f"Delete “{escape(title)}”? [b]y[/] / [b]n[/]")
        .padding(0, 1)
        .panel("Delete")
        .on_key("y", delete)
        .on_key("n esc", lambda cx: cx.pop())
    )


if __name__ == "__main__":
    todo_app([Todo(1, "Read the intuiTUIve tutorial"), Todo(2, "Build an app")]).run()
