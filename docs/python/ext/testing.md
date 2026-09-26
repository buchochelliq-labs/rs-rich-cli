# Testing and QA

Modules: `rs_rich.ext.testing` (Rust: `rich_ext::testing` and
`rich_ext::diff::assert`) and `rs_rich.ext.qa` (Rust: `rich_ext::qa`). They
check what renders rather than how: snapshots and assertions that fail with a
rendered diff, and tools that render something many ways and report what
went wrong. Rich has no counterpart.

## Assertions and snapshots

`assert_str_eq(left, right, message=None)`, `assert_json_eq(left, right,
message=None)` and `assert_render_eq(renderable, expected, width=80,
message=None)` raise `AssertionError` with rich-ext's rendered diff when the
two sides differ. `assert_json_eq` compares JSON-able values as pretty JSON
(key order does not count); `assert_render_eq` renders plainly at `width`,
ignoring trailing spaces and blank lines. The diff is unified unless
`RICH_ASSERT_LAYOUT=side-by-side`, and coloured only with
`RICH_ASSERT_COLOR=1` (or on a terminal outside CI).

```python
from rs_rich.ext.testing import assert_render_eq, assert_str_eq, render_snapshot
from rs_rich.panel import Panel

assert_render_eq(Panel("hi"), "╭──────╮\n│ hi   │\n╰──────╯", width=8)
try:
    assert_str_eq("a\nb\n", "a\nc\n", "second line")
except AssertionError as error:
    print(error)
```

```text
assertion `left == right` failed: second line
--- left
+++ right
@@ -1,2 +1,2 @@
1 1   a
2   - b
  2 + c
```

`render_snapshot(renderable, *, target=None, width=80)` captures a render
as a dict: the schema version, the size, the plain and ANSI text, and every
segment's text, colours, attributes and link. `target` is a
[`RenderTarget`](terminals.md) (default: an 80-column truecolor capture).

```python
snapshot = render_snapshot("[bold]hi[/] there", width=20)
print(sorted(snapshot))
print(repr(snapshot["plain"]), snapshot["segments"][0])
```

```text
['ansi', 'height', 'plain', 'schema_version', 'segments', 'width']
'hi there' {'text': 'hi', 'control': False, 'foreground': None, 'background': None, 'attributes': ['bold'], 'link': None}
```

`highlighter_conformance(highlighter, *, all_themes=False, scaling=True)`
runs rich-ext's conformance checks on a code highlighter (a name such as
`"syntect"`, a plugin, or a [Python code highlighter](../plugins.md)):
line counts, unknown themes and languages, control characters, and (with
`scaling`) that ten times the input takes about ten times as long. It raises
`AssertionError` listing every failure.

## QA tools

Each tool returns a `QaReport`: `data` (the report as dicts and lists),
`ok`, `kind`, and, when printed, the Rust report.

| Function | What it does |
|---|---|
| `stress(renderable, *, widths=None, heights=None, unicode=True, line_tolerance=1, strict_minimum=False)` | Render at many sizes; report overflow, clipping, unstable wrapping, panics and measure mismatches. |
| `lint(renderable, *, widths=None, color="16", unicode=True, hyperlinks=False, layout=True)` | Render and markup lints (a `str` is linted as markup). |
| `explain(renderable, target=None, *, width=80)` | Why the output looks as it does: wrapping, truncation, colour downgrades, fidelity. |
| `profile(renderable, *, width=80, iterations=20, warmup=2, frame=None)` | Measure and render timings and output size. |
| `fuzz(seed=0, cases=100)` | Seeded random text, tables, columns and trees, checked for panics, overflow, determinism and measurement. |
| `matrix(fixtures, *, profiles=None, width=80)` | Each fixture (`{name: renderable or callable}`) under capability profiles (default: the standard sixteen), checked for colour, links and ASCII. |
| `bench(name, renderable, *, width=80, warmup=0.1, target_time=1.0, min_samples=10, max_samples=10000)` | Time rendering; `data` is the measurement in nanoseconds. |

`screenshot(name, renderable, *, widths=None, color=None, unicode=None,
hyperlinks=False, height=None, approvals=None, approve=None)` renders every
width × colour × unicode combination and returns the shots; with an
`approvals` directory it compares them with the approved files, writes `.new`
files for differences (or approves them with `approve=True` or
`RICH_APPROVE=1`), and raises `AssertionError` when they differ.

```python
from rs_rich.console import Console
from rs_rich.ext import qa

report = qa.stress(Panel("hello world"), widths=[1, 20])
print(report.ok, report.data["renders"], report.data["issues"][0]["kind"])
Console(width=60).print(report)
print(qa.lint("[bold]x[/italic]").data["findings"][0]["rule"])
print([shot["key"] for shot in qa.screenshot("p", Panel("x"), widths=[10], color=["none"])])
```

```text
False 6 overflow
┏━━━━━━━┳━━━━━━━━┳━━━━━━━━━━┳━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━┓
┃ Width ┃ Height ┃ Issue    ┃ Detail                       ┃
┡━━━━━━━╇━━━━━━━━╇━━━━━━━━━━╇━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━┩
│ 1     │ -      │ overflow │ line 1 is 2 cells wide: "╭╮" │
│ 1     │ 5      │ overflow │ line 1 is 2 cells wide: "╭╮" │
│ 1     │ 24     │ overflow │ line 1 is 2 cells wide: "╭╮" │
└───────┴────────┴──────────┴──────────────────────────────┘
6 renders, 3 issues

markup_error
['p@10.none.unicode', 'p@10.none.ascii']
```
