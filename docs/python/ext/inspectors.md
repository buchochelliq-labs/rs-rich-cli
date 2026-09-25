# Inspectors

Modules: `rs_rich.ext.source_view`, `hex`, `unicode_inspect`, `env_inspect`
(Rust: `rich_ext::source_view` and friends). They are the views behind the
`rich` binary's inspection commands.

## Source

`SourceView` shows code with line numbers, visible tabs and search matches.

```python
from rs_rich.console import Console
from rs_rich.ext import source_view

console = Console(width=60)
code = 'fn main() {\n\tlet greeting = "Hello";\n}\n'
view = source_view.SourceView(code, "text", search="hello", start_line=10)
console.print(view)
print(view.matches())
```

```text
10 │ fn main() {                                            
11 │     let greeting = "Hello";                            
12 │ }                                                      
[(11, 1)]
```

## Bytes

`HexView` is a hex dump with an ASCII column, byte classes told apart by
style, runs of identical lines collapsed, and highlighted needles. A needle
is `bytes`, or a string in the CLI's syntax: hex bytes (`"48 65"`) or a
quoted string (`'"Hello"'`).

```python
from rs_rich.ext import hex

data = b"\x00\x01\x02 Hello, world!\x7f\xff"
console.print(hex.HexView(data, bytes_per_line=8, highlight=b"Hello"))
print(hex.find_all(data, '"l"'), hex.byte_class(0x7f))
```

```text
00000000  00 01 02 20 48 65 6c 6c  │... Hell│
00000008  6f 2c 20 77 6f 72 6c 64  │o, world│
00000010  21 7f ff                 │!..│
00000013
[6, 7, 14] control
```

## Unicode

`UnicodeView` lists a string's grapheme clusters with code points, width and
kind (combining, emoji, invisible, control, bidi); for `bytes` it also shows
invalid UTF-8.

```python
from rs_rich.ext import unicode_inspect

view = unicode_inspect.UnicodeView("e\u0301👍🏽\t")
Console(width=70).print(view)
print(view.summary_text())
```

```text
┏━━━━━━━━┳━━━━━━┳━━━━━━━━━━━━━━━━━━━━━━━━━┓
┃ Offset ┃ Char ┃ Details                 ┃
┡━━━━━━━━╇━━━━━━╇━━━━━━━━━━━━━━━━━━━━━━━━━┩
│      0 │ é    │ combining, 1 cell       │
│        │      │ U+0065 U+0301           │
│        │      │ 65 cc 81                │
│        │      │ e\u{301}                │
│      3 │ 👍🏽   │ emoji, 2 cells          │
│        │      │ U+1F44D U+1F3FD         │
│        │      │ f0 9f 91 8d f0 9f 8f bd │
│        │      │ \u{1f44d}\u{1f3fd}      │
│     11 │ ␉    │ control, 0 cells        │
│        │      │ U+0009                  │
│        │      │ 09                      │
│        │      │ \u{9}                   │
└────────┴──────┴─────────────────────────┘
12 bytes, 5 code points, 3 graphemes, 3 cells, 0 invalid sequences
12 bytes, 5 code points, 3 graphemes, 3 cells, 0 invalid sequences
```

## Environment

`EnvView` shows environment variables with secrets redacted and
`PATH`-like values split; `PathView` checks every entry of a search path.
`probe` replaces the file-system check, here so the example does not depend
on this machine.

```python
from rs_rich.ext import env_inspect

console.print(env_inspect.EnvView({"HOME": "/home/ada", "API_TOKEN": "sk-live-abc"}))
kinds = {"/usr/bin": "directory", "/etc/passwd": "file"}
path = env_inspect.PathView("PATH", "/usr/bin:/opt/none:/etc/passwd:/usr/bin", separator=":",
                            probe=lambda entry: kinds.get(entry, "missing"))
print(path.problems())
```

```text
┏━━━━━━━━━━━┳━━━━━━━━━━━━━━━━━━━┓
┃ Name      ┃ Value             ┃
┡━━━━━━━━━━━╇━━━━━━━━━━━━━━━━━━━┩
│ API_TOKEN │ •••••• (11 chars) │
│ HOME      │ /home/ada         │
└───────────┴───────────────────┘
[(2, '/opt/none', 'missing'), (3, '/etc/passwd', 'not a directory'), (4, '/usr/bin', 'duplicate of #1')]
```
