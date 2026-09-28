# Terminals and accessibility

Modules: `rs_rich.ext.capabilities`, `fidelity`, `a11y`, `ansi_explain`,
`sanitize`, `encoding`, `target`, `frame`, `theme` (Rust: `rich_ext::capabilities` and
friends).

## What the terminal can do

`detect_capabilities(env, terminal=..., size=...)` reads an environment
mapping (default `os.environ`) and reports color, Unicode, hyperlinks,
size, CI and interactivity, with where each answer came from. Keyword
overrides (`color="256"`, `unicode=False`, `width=40`) and the
`RICH_*` variables in `CAPABILITY_OVERRIDE_VARS` take precedence.

```python
from rs_rich.console import Console
from rs_rich.ext import capabilities

console = Console(width=80)
env = {"TERM": "xterm-256color", "TERM_PROGRAM": "WezTerm", "COLORTERM": "truecolor", "LANG": "en_US.UTF-8"}
report = capabilities.detect_capabilities(env, terminal=True, size=(120, 40))
print(report.color, report.hyperlinks, report.width, report.terminal)
ci = capabilities.detect_capabilities({"CI": "true", "GITHUB_ACTIONS": "true", "RICH_WIDTH": "100"})
print(ci.width, ci.interactive, ci.ci)
```

```text
truecolor True 120 WezTerm
100 False GitHub Actions
```

## Fidelity

`select_fidelity` picks the richest output level a terminal supports:
`"rich"`, `"styled"`, `"plain"` or `"ascii"`. `Degrade(renderable,
level=...)` renders anything at a lower level: no links or animation, no
color, no styles, ASCII only.

```python
from rs_rich.ext import fidelity
from rs_rich.panel import Panel
from rs_rich.text import Text

print(fidelity.select_fidelity(unicode=True, color=False, interactive=True, animation=True))
panel = Panel(Text.from_markup("[green]✔ ok[/]  [red]✖ failed[/]  → log"), title="CI", width=30)
console.print(fidelity.Degrade(panel, level="ascii"))
print(fidelity.ascii_text("“quoted” → ok"))
```

```text
styled
+------------ CI ------------+
| v ok  x failed  > log      |
+----------------------------+
"quoted" > ok
```

## Accessibility

`AccessibilityPolicy` collects the preferences that change output: screen
reader, reduced motion, high contrast, monochrome, and how statuses are
marked (symbols, ASCII or words). `from_env` reads `NO_COLOR` and
`RICH_A11Y`; `theme()` is a `Theme` that follows the policy.
`accessible_text` renders a renderable as plain, linear text for screen
readers; the contrast functions check a theme against WCAG ratios.

```python
from rs_rich.ext import a11y
from rs_rich.ext.diagnostic import Diagnostic

policy = a11y.AccessibilityPolicy.from_env({"RICH_A11Y": "screen-reader"})
print(policy.screen_reader, policy.status("ok"), policy.status("error"))
print(a11y.accessible_text(Diagnostic.error("mismatched types", code="E0308", help=["use `u64`"])))
print(round(a11y.contrast_ratio("#777777", "#ffffff"), 2), a11y.suggest_color((120, 120, 120), (255, 255, 255), 4.5))
```

```text
True ok: error:
error[E0308]: mismatched types
help: use `u64`
4.48 (118, 118, 118)
```

## ANSI explained

`explain(text_or_bytes)` breaks captured terminal output into text and
escape sequences, each with its meaning; `ExplanationView` renders that as a
table or inline. Offsets are string indices.

```python
from rs_rich.ext import ansi_explain

explanation = ansi_explain.explain("\x1b[1;32m✔\x1b[0m ok \x1b]8;;https://ci.example\x1b\\log\x1b]8;;\x1b\\")
print(repr(explanation.visible_text))
for token in explanation.tokens:
    print(token.offset, token.kind, token.meaning)
print(ansi_explain.escape_visible("\x1b[0m\x07"))
```

```text
'✔ ok log'
0 SGR bold on, fg green
7 text 1 character
8 SGR reset
12 text 4 characters
16 OSC open hyperlink to https://ci.example (ST-terminated)
41 text 3 characters
44 OSC close hyperlink (ST-terminated)
ESC[0m<BEL>
```

## Sanitizing and decoding

```python
from rs_rich.ext import encoding, sanitize

print(repr(sanitize.sanitize_terminal_controls("a\x1b[31mb\x07c")))
print(repr(sanitize.sanitize_single_line("one\ntwo\r")))
print(encoding.decode_text("héllo".encode("utf-16"), "utf-16"))
```

```text
'a␛[31mb␇c'
'one␊two␍'
héllo
```

`decode_text` raises `EncodingError` on invalid input and `ValueError` for
an encoding it does not know.

## Render targets

`target.RenderTarget` is where output goes (`"terminal"`, `"plain_stream"`,
`"capture"`) and what it supports; `text(renderable)` renders for it, and
`frame(renderable)` renders it to a [frame](#frames). The
live coordinator ([Layout and live output](layout.md)) writes to one.

```python
from rs_rich.ext import target

stream = target.RenderTarget("plain_stream", width=20)
print(repr(stream.text(Text("docs", style="bold red"))))
print(stream.capabilities["color_system"], stream.capabilities["interactive"])
```

```text
'docs'
None False
```

## Frames

`frame.Frame` holds a render as rows of styled runs (`rich_ext::frame`): its
plain text, one cell per terminal column, the bytes the console writes for
it, and a cell-level `diff` against a previous frame, which is what a live
repaint rewrites. Build one from a console's render
(`Frame.from_console(console, renderable)`), from segments
(`Frame.from_segments`), or with `RenderTarget.frame(renderable)`. Control
segments are dropped: a frame is content.

```python
import io
from rs_rich.console import Console
from rs_rich.ext.frame import Frame

console = Console(file=io.StringIO(), width=30, force_terminal=True, color_system="truecolor")
before = Frame.from_console(console, "[bold]build[/] 3 crates\nwaiting")
after = Frame.from_console(console, "[bold]build[/] 4 crates\ndone")
print(after.plain(), end="")
print(after.height, after.width)
print(after.diff(before))
print(repr(after.encode_span(0, 6, 7)))
print(repr(before.to_ansi(console)))
print(after.cells(0)[:2])
```

```text
build 4 crates
done
2 14
[(0, range(6, 7)), (1, range(0, 7))]
'\x1b[1;36m4\x1b[0m'
'\x1b[1mbuild\x1b[0m \x1b[1;36m3\x1b[0m crates\nwaiting\n'
[('b', 1, Style(bold=True)), ('u', 1, Style(bold=True))]
```

`to_ansi(console)` writes exactly what `console.print` writes for the same
render; `to_ansi_merged` merges adjacent runs of one style first (fewer
bytes, not Rich's), and `encode(color_system, no_color)` picks the colour
system directly.

`theme.extended_theme()` is the default theme with the styles the extension
renderables use (`theme.EXTRA_STYLES`).
