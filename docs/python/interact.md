# Interactive components

```python
from rs_rich.interact import Select, MultiSelect, Input, Confirm, Choice, Form, Pager, Item
```

`rs_rich.interact` is the port's `rs-rich-interact` crate from Python: fuzzy
pickers, text input and multi-line text, confirmations, forms, a pager, and
file, colour and asset pickers, between printing and a full TUI. Rich has none of these, so the reference is the Rust crate: the same
component, keys and terminal paint the same frames.

Each component is a configuration. `ask()` runs it on the terminal and
returns the answer; `headless(keys)` runs it with scripted keys and returns
what it painted, which is how you test code that uses them.

```python
from rs_rich.interact import Select

record = Select("Open", ["src/lib.rs", "src/main.rs", "README.md"]).headless("down enter")
print(record.frames[0])
print(record.last_frame)
print(repr(record.value))
```

```text
? Open › 
❯ src/lib.rs
  src/main.rs
  README.md
  3/3 · ↑↓ move · enter pick · esc cancel
? Open › src/main.rs
'src/main.rs'
```

## Running on the terminal

```text
component.ask(*, fallback="prompt", interactive=None, output="stdout",
              no_color=None, height=None, transient=False, tty_keys=False,
              alternate_screen=False, mouse=False)
```

`ask` (also `rs_rich.interact.ask(component, ...)`) takes the terminal, runs
the component until it finishes and gives the terminal back, restoring it
on every way out. What comes back:

| The component | `ask` |
|---|---|
| finished | returns the answer |
| was cancelled (Escape) | raises `Cancelled` |
| was interrupted (Ctrl+C) | raises `KeyboardInterrupt`, as `input()` does |
| had no terminal and no answer (see below) | raises `NotInteractive` |

`run(component, ...)` takes the same options and returns an `Outcome`
instead of raising for a cancel or Ctrl+C: its `kind` is `"done"`,
`"cancelled"` or `"interrupted"`, `value` the answer when done, `action` the
item action that picked it, and `unwrap()` is what `ask` returns.

`output="stderr"` paints on standard error, so standard output keeps only
what the program prints; `tty_keys=True` reads keys from the terminal even
when standard input is a pipe. The GIL is released while `ask` waits for
keys, so other Python threads keep running. `height` is how many rows to
paint, at most the terminal's (`None`: all of them); it must be at least 1.

### Exceptions from your code

Python code a component calls during a run (a validator, a preview or
confirmation body that renders, a component of your own) may raise. Any
exception but a validator's `ValueError` ends the run at once, and `ask`,
`run`, `headless` or `degrade` raises it, `KeyboardInterrupt` and
`SystemExit` included. A render that raises (a preview, a body) is noticed
when it is painted, so on the terminal the run ends at the next key.

A validator or component may start another run (asking a follow-up
question, say). Runs nest on the thread's native stack: past what it holds,
or 200 deep, the inner run raises `RecursionError` rather than crashing.

### Without a terminal

With standard input or the output not a terminal, under CI, with
`TERM=dumb`, or with `interactive=False`, no terminal session starts and
`fallback` decides:

- `"prompt"` (the default): ask line by line, prompts on standard error and
  answers from standard input (a password without echo). When input ends
  before an answer, the component's default answers; without one,
  `NotInteractive` is raised;
- `"default"`: return the component's default (`Select(default=...)`,
  `MultiSelect(marked=...)`, `Input(default=...)`, `Confirm(default=...)`),
  or raise `NotInteractive` when there is none;
- `"error"`: raise `NotInteractive`.

`NotInteractive.reason` names why: `"stdin_not_terminal"`,
`"stdout_not_terminal"`, `"stderr_not_terminal"`, `"no_terminal"`, `"ci"`,
`"dumb_terminal"` or `"requested"`. `degrade(component, answers, fallback=...)`
runs this path with scripted answers:

```python
from rs_rich.interact import Confirm, Input, NotInteractive, Select, degrade

record = degrade(Select("Pick", ["red", "green"]), ["2"])
print(repr(record.value))
print(record.prompts)
print(degrade(Input("Name", default="Ada"), fallback="default").value)
try:
    degrade(Confirm("Deploy?"), fallback="error", reason="ci")
except NotInteractive as error:
    print(error, "/", error.reason)
```

```text
'green'
Pick
  1) red
  2) green
Number or name: 
Ada
not interactive: running under CI / ci
```

## Items and values

A picker's items are `Item`s, or any objects (labelled with `str`). An
item's value is any Python object, returned as it is when the item is
chosen:

```text
Item(value, label=None, *, description=None, preview=None, metadata=None,
     keywords=(), actions=())
```

`description` is a dimmer note after the label; `metadata` (a dict or pairs)
and `keywords` are searched without being shown; `preview` (console markup,
or any renderable) shows beside the list on a wide terminal and below it on
a narrow one; `actions` are `Action(id, label, key)`s, keys that pick the
item and tell the caller what to do with it (`outcome.action`).

```python
from rs_rich.interact import Action, Item, Script, Select
from rs_rich.panel import Panel

servers = [
    Item({"host": "db1", "port": 5432}, "db1", description="primary",
         preview=Panel("postgres 16", title="db1"), keywords=["database"]),
    Item({"host": "web1", "port": 443}, "web1",
         actions=[Action("ssh", "Open a shell", "ctrl+s")]),
]
record = Select("Server", servers).headless("", width=80, height=8)
print(record.last_frame)
print(Select("Server", servers).headless(Script().text("database").keys("enter")).value)
print(Select("Server", servers).headless("down ctrl+s").outcome.action)
```

```text
? Server › 
❯ db1  primary                       │ ╭───────────────── db1 ─────────────────╮
  web1                               │ │ postgres 16                           │
  2/2 · ↑↓ move · enter pick · esc cancel
{'host': 'db1', 'port': 5432}
ssh
```

## Select and MultiSelect

```text
Select(prompt, items, *, default=None, query="", height=10, preview="auto",
       preview_height=10)
MultiSelect(prompt, items, *, marked=(), query="", height=10, preview="auto")
```

Typing filters the items with the fuzzy matcher (below) and highlights what
matched; the arrows, PageUp/PageDown, Home and End move; Enter picks.
`MultiSelect` marks with Tab (Shift+Tab marks and moves up, Ctrl+A marks
every match) and returns the marked values in list order, or the focused
one when none is marked. `preview` is `"auto"`, `"right"`, `"below"` or
`"hidden"`.

```python
from rs_rich.interact import MultiSelect, Script, Select

files = ["src/lib.rs", "src/main.rs", "docs/maintenance.md", "Cargo.toml"]
record = Select("Open", files).headless(Script().text("main").keys("enter"))
print(record.frames[-2])
print(MultiSelect("Stage", files).headless("tab down tab enter").value)
```

```text
? Open › main
❯ src/main.rs
  docs/maintenance.md


  2/4 · ↑↓ move · enter pick · esc cancel
['src/lib.rs', 'docs/maintenance.md']
```

## Input

```text
Input(prompt, *, value="", placeholder=None, default=None, help=None,
      password=False, mask=None, validate=None, history=(), suggestions=None,
      limit=5)
```

One line, edited as in a shell (the arrows, Home/End, Ctrl+A/E/U/W,
Backspace, Delete). `password=True` shows `•` for each character (`mask`
picks another). Up and Down walk `history`; `suggestions` (`str`s or
`(value, description)` pairs) are filtered as you type and Tab accepts one.
`validate(text)` returns `None` or `True` to accept, and a message, `False`,
or a raised `ValueError` to refuse: the message shows under the line and
Enter waits. Any other exception from it ends the run at once and is raised
from `ask`.

```python
from rs_rich.interact import Input, Script

def port(text):
    if not text.isdigit():
        return "a number, please"

record = Input("Port", validate=port).headless(Script().text("http").keys("enter"))
print(record.last_frame)
script = Script().text("http").keys("ctrl+u").text("8080").keys("enter")
print(repr(Input("Port", validate=port).headless(script).value))
print(Input("Token", password=True).headless(Script().text("s3cret").keys("enter")).last_frame)
```

```text
? Port › http
  ✗ a number, please
'8080'
? Token › ••••••
```

## Confirm

```text
Confirm(title="Are you sure?", *, body=None, warnings=(), choices=None,
        default=None, body_height=None)
```

A question, with an optional body (a renderable or a list of them, in a
scrollable viewport), warnings under it, and choices: `y`/`n` answering
`"yes"`/`"no"` by default, or your own `Choice(id, label, key)`s. The answer
is the chosen id; a choice's key picks it, and Left, Right, Tab and Enter
choose too.

```python
from rs_rich.interact import Choice, Confirm

confirm = Confirm("Apply 2 changes?", warnings=["this rewrites history"],
                  choices=[Choice("apply", "Apply", "a"), Choice("skip", "Skip", "s")])
print(confirm.headless("s").frames[0])
print(confirm.headless("s").value)
print(Confirm("Deploy?").headless("y").value)
```

```text
? Apply 2 changes?
  ⚠ this rewrites history
   Apply   Skip  
  a/s · ←→ move · enter choose · esc cancel
skip
yes
```

## Form

`Form(title)` asks several named fields together; `text`, `masked`, `input`
(a configured `Input`), `choice` and `toggle` add fields and return the
form. Tab and the arrows move; Enter moves on and, on the last field,
submits once every field validates. The answer is a `dict`: `str` for text
and choices, `bool` for toggles.

```python
from rs_rich.interact import Form, Script

form = (Form("New project")
        .text("name", "Name", placeholder="my-app")
        .choice("license", "License", ["MIT", "Apache-2.0"])
        .toggle("git", "Git repository", on=True))
record = form.headless(Script().text("demo").keys("enter right enter enter"))
print(record.value)
print(record.last_frame)
```

```text
{'name': 'demo', 'license': 'Apache-2.0', 'git': True}
? New project
  Name            demo
  License         Apache-2.0
  Git repository  yes
```

## Pager

`Pager(renderable, *, search=None)` pages any renderable at the terminal's
width: the arrows, Space and PageUp/PageDown scroll, `/` searches, `n` and
`N` move between matches, and `q` or Escape closes (the answer is `None`
either way: closing a pager is not a cancel). Without a terminal it writes
the content out.

```python
from rs_rich.interact import Pager
from rs_rich.text import Text

log = Text("\n".join(f"step {i}" + (" failed" if i == 7 else "") for i in range(20)))
record = Pager(log, search="failed").headless("q", width=30, height=5)
print(record.last_frame)
```

```text
step 4
step 5
step 6
step 7 failed
lines 5–8 of 20 · match 1/1 · 
```

## TextArea

`TextArea(prompt="Write", *, value="", placeholder=None, char_limit=None,
height=5, line_numbers=False, submit="ctrl+d")` reads several lines: Enter
starts a line, `submit` (a key name) finishes, Escape cancels. Long lines
wrap and the text scrolls to keep the caret in view; `char_limit` counts
line breaks too. The answer is the text, lines joined with `"\n"`. Without
a terminal, every line of input up to its end is the text.

```python
from rs_rich.interact import Script, TextArea

record = TextArea("Notes", line_numbers=True).headless(
    Script().text("first").keys("enter").text("second").keys("ctrl+d"), width=48, height=8
)
print(record.frames[-2])
print(repr(record.value))
```

```text
? Notes › 
1 │ first
2 │ second
  ~
  ~
  ~
  enter new line · ctrl+d submit · esc cancel
'first\nsecond'
```

## FilePicker

`FilePicker(root=".", *, prompt="File", mode="file", extensions=(),
hidden=False, jail=False, default=None, query="", height=10, mouse=False)`
browses from `root`: typing filters, Enter opens a directory or picks a
file, Right opens, Left (or Backspace with nothing typed) goes up, and
Ctrl+T shows hidden files. `mode` is what may be picked: `"file"`,
`"directory"` (files are not listed) or `"both"`. `extensions` keeps only
files with those extensions. With `jail=True` nothing outside `root` is
listed, opened or previewed, symbolic links out of it included. The focused
text file is previewed beside the list. The answer is a `pathlib.Path`;
without a terminal, `default`.

```python
import pathlib
import tempfile

from rs_rich.interact import FilePicker, Script

root = pathlib.Path(tempfile.mkdtemp())
(root / "src").mkdir()
(root / "src" / "main.py").write_text("print('hi')\n")
(root / "README.md").write_text("# Demo\n")
picked = FilePicker(root).headless(Script().keys("enter").text("main").keys("enter")).value
print(picked.relative_to(root).as_posix())
print([p.name for p in [FilePicker(root, mode="directory").headless("enter").value]])
```

```text
src/main.py
['src']
```

## ColorPicker

`ColorPicker(prompt="Colour", *, format="hex", value="", default=None,
height=10, palette=False, mouse=False)` picks a colour: typing filters
rich's named colours, text that is a colour (`#ff8800`, `rgb(255,136,0)`,
`color(208)`) is offered first, and Tab switches to the 256-colour palette,
moved with the arrows. A swatch shows the focused colour. The answer is a
colour string rich parses: `#rrggbb` for `format="hex"`, a name for
`"name"` (or `color(N)`, or hex, for a colour without one), `rgb(r,g,b)`
for `"rgb"`.

```python
from rs_rich.interact import ColorPicker, Script

print(ColorPicker(format="name").headless(Script().text("dark_orange").keys("enter")).value)
print(ColorPicker(format="rgb").headless(Script().text("#ff8800").keys("enter")).value)
print(ColorPicker(format="name").headless("tab right right down enter").value)
```

```text
dark_orange
rgb(255,136,0)
dark_blue
```

## AssetPicker

`AssetPicker(kind="emoji", *, prompt=None, query="", default=None,
height=10, mouse=False)` picks an emoji by its shortcode name, a box style
(`kind="box"`) or a spinner (`kind="spinner"`), each with a preview. The
answer is the emoji itself, or the style's or spinner's name as rich takes
it.

```python
from rs_rich.interact import AssetPicker, Script

print(AssetPicker().headless(Script().text("thumbs_up").keys("enter")).value)
print(AssetPicker("box").headless(Script().text("double").keys("enter")).value)
```

```text
👍
double
```

## Headless runs

`headless(component, script=None, *, width=80, height=24)` (or
`component.headless(...)`) runs a component against a scripted keyboard at
a fixed size, with virtual time. The script is a string of key names, or a
`Script` built by chaining: `keys("down tab")`, `text("typed")`,
`paste("pasted")`, `resize(columns, rows)`, `wait(seconds)`, and the mouse:
`click(column, row)`, `drag((column, row), (column, row))` and
`scroll(column, row, down=True)`, at cells of the component's own view
(the mouse reaches the components that take `mouse=True`). Key names are
`enter`, `tab`, `shift+tab`, `backspace`, `delete`, `escape` (`esc`),
`space`, the arrows, `home`, `end`, `pageup`, `pagedown`, `f1`–`f12`, one
character, and `ctrl+`/`alt+`/`shift+` combinations.

The `Record` it returns has the `outcome` (`None` if the script ran out
before the component finished), `value`, `frames` (the plain text of every
paint that changed something), `last_frame`, and `output` (the exact bytes
written, escape sequences included).

## Your own components

Any object with `render(width, height)` and `handle(event)` is a component:
`render` returns a renderable, and `handle` returns `None` to go on,
`Done(value)` to finish or `Cancel()` to cancel. `event.kind` is `"key"`
(with `event.key` the key's name), `"paste"` (`event.text`), `"resize"`
(`event.columns`, `event.rows`), `"mouse"` (`event.mouse` is `"down"`,
`"up"`, `"drag"`, `"moved"`, `"scroll_up"` or `"scroll_down"`, at
`event.column` and `event.row` of the component's view), `"link"` (a click
on a hyperlink, its URL in `event.text`) or `"tick"`. An optional
`default_value()` is the answer without a terminal.

```python
from rs_rich.interact import Done, headless

class Counter:
    def __init__(self):
        self.count = 0
    def handle(self, event):
        if event.key == "up":
            self.count += 1
        elif event.key == "enter":
            return Done(self.count)
    def render(self, width, height):
        return f"count: [bold]{self.count}[/]"

record = headless(Counter(), "up up enter", width=20, height=3)
print(record.frames, record.value)
```

```text
['count: 0', 'count: 1', 'count: 2'] 2
```

## Fuzzy matching

`fuzzy(pattern, candidate)` returns a `Match` (`score`, `positions`) or
`None`; `rank(pattern, candidates)` returns the matching `(index, Match)`
pairs, best first. A pattern's characters match in order anywhere in the
candidate, scoring more at word starts and in runs; case is ignored unless
the pattern has an upper-case letter, and space-separated terms must all
match.

```python
from rs_rich.interact import fuzzy, rank

print(fuzzy("fb", "foo_bar"))
print([index for index, _ in rank("main", ["src/lib.rs", "docs/maintenance.md", "src/main.rs"])])
```

```text
Match(score=43, positions=[0, 4])
[2, 1]
```

## The command line

The wheel's `python -m rs_rich` has the `rich` binary's interactive
commands: `choose`, `filter`, `input`, `confirm`, `pager`, `write`, `file`,
`color` and `asset` (see [The command line](cli.md)).

## Not yet from Python

- `Theme` (the components' styles and symbols): the default theme only.
- `TableSelect`, `TreeSelect`, view-wide `Actions` and the action menu, and
  mouse support in `Select`, `Confirm`, `Form` and `Pager`.
- `Input`'s background suggestion `provider`: fixed `suggestions` only.
- The multi-component `EventLoop` with timers, and hand-offs to another
  program (`Flow::Handoff`) from Python components.
