"""The pictures on docs/python/interact.md: run it, or scripts/capture_python_docs.py.

Each picture is a frame a headless run painted, replayed in colour from the
bytes the run wrote; what the example prints is on the page as text.
"""

from _shots import Shots, painted

from rs_rich.interact import (
    Action, Choice, Column, Component, Confirm, Done, Form, Ignored, Input, Item, Keymap, Label,
    Map, MultiSelect, Pager, Script, Select, Split, TextArea,
)
from rs_rich.panel import Panel
from rs_rich.text import Text


def select(console):
    # --8<-- [start:select]
    record = Select("Open", ["src/lib.rs", "src/main.rs", "README.md"]).headless("down enter")
    print(record.frames[0])
    print(record.last_frame)
    print(repr(record.value))
    # --8<-- [end:select]
    console.print(painted(record, 0))


def items(console):
    # --8<-- [start:items]
    servers = [
        Item({"host": "db1", "port": 5432}, "db1", description="primary",
             preview=Panel("postgres 16", title="db1"), keywords=["database"]),
        Item({"host": "web1", "port": 443}, "web1",
             actions=[Action("ssh", "Open a shell", "ctrl+s")]),
    ]
    record = Select("Server", servers).headless("", width=80, height=8)
    print(record.last_frame)
    # --8<-- [end:items]
    console.print(painted(record))


def filtered(console):
    # --8<-- [start:filtered]
    files = ["src/lib.rs", "src/main.rs", "docs/maintenance.md", "Cargo.toml"]
    record = Select("Open", files).headless(Script().text("main").keys("enter"))
    print(record.frames[-2])
    print(MultiSelect("Stage", files).headless("tab down tab enter").value)
    # --8<-- [end:filtered]
    console.print(painted(record, -2))


def text_input(console):
    # --8<-- [start:input]
    def port(text):
        if not text.isdigit():
            return "a number, please"

    record = Input("Port", validate=port).headless(Script().text("http").keys("enter"))
    print(record.last_frame)
    # --8<-- [end:input]
    console.print(painted(record))


def confirm(console):
    # --8<-- [start:confirm]
    confirm = Confirm("Apply 2 changes?", warnings=["this rewrites history"],
                      choices=[Choice("apply", "Apply", "a"), Choice("skip", "Skip", "s")])
    print(confirm.headless("s").frames[0])
    # --8<-- [end:confirm]
    console.print(painted(confirm.headless("s"), 0))


def form(console):
    # --8<-- [start:form]
    form = (Form("New project")
            .text("name", "Name", placeholder="my-app")
            .choice("license", "License", ["MIT", "Apache-2.0"])
            .toggle("git", "Git repository", on=True))
    record = form.headless(Script().text("demo").keys("enter right enter enter"))
    print(record.value)
    print(record.last_frame)
    # --8<-- [end:form]
    console.print(painted(record))


def pager(console):
    # --8<-- [start:pager]
    log = Text("\n".join(f"step {i}" + (" failed" if i == 7 else "") for i in range(20)))
    record = Pager(log, search="failed").headless("q", width=30, height=5)
    print(record.last_frame)
    # --8<-- [end:pager]
    console.print(painted(record, width=30))


def text_area(console):
    # --8<-- [start:textarea]
    record = TextArea("Notes", line_numbers=True).headless(
        Script().text("first").keys("enter").text("second").keys("ctrl+d"), width=48, height=8
    )
    print(record.frames[-2])
    print(repr(record.value))
    # --8<-- [end:textarea]
    console.print(painted(record, -2, width=48))


# --8<-- [start:counter]
class Counter(Component):
    def __init__(self):
        self.count = 0
        self.keys = Keymap("counter").bind("up", "up k", "count up").bind("done", "enter", "answer")

    def handle(self, event):
        action = self.keys.action(event)
        if action == "up":
            self.count += 1
        elif action == "done":
            return Done(self.count)
        else:
            return Ignored()

    def render(self, context):
        return f"count: [bold]{self.count}[/]"

    def keymap(self):
        return self.keys
# --8<-- [end:counter]


def composed(console):
    # --8<-- [start:compose]
    answers = {}
    form = Column(
        Label("[bold]New file[/]"),
        Map(Input("Name"), lambda name: answers.update(name=name)),
        Split(Counter(), Select("Kind", ["text", "code"]), ratio=40),
    )
    record = form.headless(Script().text("notes").keys("enter tab down enter"), width=60, height=8)
    print(answers, record.value)
    # --8<-- [end:compose]
    console.print(painted(record, -2, width=60))


shots = Shots("interact")
shots.shot("select", select, width=48, stdout=False)
shots.shot("items", items, width=80, stdout=False)
shots.shot("filtered", filtered, width=48, stdout=False)
shots.shot("input", text_input, width=48, stdout=False)
shots.shot("confirm", confirm, width=48, stdout=False)
shots.shot("form", form, width=48, stdout=False)
shots.shot("pager", pager, width=30, stdout=False)
shots.shot("textarea", text_area, width=48, stdout=False)
shots.shot("compose", composed, width=60, stdout=False)
shots.finish()
