# Text

```python
from rs_rich.text import Text
```

A `Text` is a string with styled spans. It corresponds to `rich.text.Text`.
Offsets are Python character offsets, as in Rich, so `stylize("bold", 0, 2)`
covers two characters whatever their encoding.

## Constructor

```text
Text(text="", style="", *, justify=None, overflow=None, no_wrap=None,
     end="\n", tab_size=None, spans=None)
```

| Argument | Meaning |
|---|---|
| `text` | The plain string. It is not parsed as markup; see [`from_markup`](#from_markup). |
| `style` | A style for the whole text: a style string (`"bold red"`), a theme name, or a [`Style`](style.md). |
| `justify` | `"left"`, `"center"`, `"right"`, `"full"` or `"default"`. |
| `overflow` | What a line too long for its space does: `"fold"`, `"crop"`, `"ellipsis"` or `"ignore"`. |
| `no_wrap` | `True` to keep each line whole, then apply `overflow`. |
| `end` | What ends the text when it renders on its own (`Console.render`, a `Lines`). `print` ends lines with its own `end`, as in Rich. |
| `tab_size` | Spaces per tab for `expand_tabs` and `wrap`, or `None` for 8. |
| `spans` | Initial `Span(start, end, style)`s. |

An invalid `justify` or `overflow` raises `ValueError`, and a `style` that is
neither a string nor a `Style` raises `TypeError`.

`justify`, `overflow` and `no_wrap` apply where the text is laid out in a
space of its own, such as a panel or a table cell. A `Text` printed by itself
takes the console's settings, as in Rich:

```python
from rs_rich.console import Console
from rs_rich.panel import Panel
from rs_rich.text import Text

console = Console(width=12)
text = Text("abcdefghijklmnop", overflow="ellipsis", no_wrap=True)
console.print(text)
console.print(Panel(text))
console.print(Panel(Text("right", justify="right")))
```

```text
abcdefghijkl
mnop
╭──────────╮
│ abcdefg… │
╰──────────╯
╭──────────╮
│    right │
╰──────────╯
```

## from_markup

```text
Text.from_markup(text, *, style="", emoji=True, emoji_variant=None,
                 justify=None, overflow=None, end="\n") -> Text
```

This parses console markup into a `Text`. `style` is a base style under the
markup's own. `emoji` replaces codes such as `:smiley:` (before the markup is
parsed, as `Console.print` does), and `emoji_variant` (`"emoji"` or `"text"`)
picks a presentation for codes that name none. Markup that does not parse
raises `rs_rich.errors.MarkupError`.

```python
from rs_rich.text import Text

text = Text.from_markup("[bold]bold[/] and [italic]italic[/]")
print(text.plain)
```

```text
bold and italic
```

## Methods and properties

| Member | Meaning |
|---|---|
| `plain` | The string without styles. |
| `len(text)` | The number of characters. |
| `str(text)` | The same as `plain`. |
| `append(text, style=None)` | Append a `str` (with an optional style) or another `Text` (keeping its spans; a `style` is then a `ValueError`). A text can append itself: `t.append(t)`. Returns the text itself, so calls chain. Anything else is a `TypeError`. |
| `stylize(style, start=0, end=None)` | Apply a style to characters `start` up to `end`. Negative offsets count from the end, `end=None` means the end, and offsets out of range are clamped, however large. |

```python
import io
from rs_rich.console import Console
from rs_rich.text import Text

text = Text("Hello")
text.append(", World", style="bold").append("!")
text.stylize("italic", 0, 5)
text.stylize("underline", -1)
print(text.plain, len(text))

out = io.StringIO()
Console(file=out, force_terminal=True, color_system="truecolor").print(text)
print(repr(out.getvalue()))
```

```text
Hello, World! 13
'\x1b[3mHello\x1b[0m\x1b[1m, World\x1b[0m\x1b[4m!\x1b[0m\n'
```

Offsets count characters, not bytes:

```python
from rs_rich.text import Text

text = Text("日本語 text")
text.stylize("bold", 0, 2)
print(len(text))
```

```text
8
```

## Other constructors

| Class method | Meaning |
|---|---|
| `Text.from_ansi(text, *, style="", justify=None, overflow=None, no_wrap=None, end="\n", tab_size=8)` | Decode ANSI escape codes (SGR styles and OSC 8 links) into spans. |
| `Text.styled(text, style="", *, justify=None, overflow=None)` | A text with `style` as a span over all of it (so justification padding stays unstyled). |
| `Text.assemble(*parts, style="", justify=None, overflow=None, no_wrap=None, end="\n", tab_size=8)` | Join strings, `Text`s and `(string, style)` pairs. |

```python
from rs_rich.text import Text

text = Text.assemble("plain ", ("bold", "bold"), (" red", "red"), style="italic")
print(repr(text))
print(repr(Text.from_ansi("\x1b[1mbold\x1b[0m text")))
print(text.markup)
```

```text
<text 'plain bold red' [Span(6, 10, 'bold'), Span(10, 14, 'red')] 'italic'>
<text 'bold text' [Span(0, 4, Style(bold=True))] ''>
[italic]plain [bold]bold[red][/bold] red[/red][/italic]
```

## Spans and styles

`text.spans` is a list of `Span(start, end, style)` named tuples, in
character offsets; `style` is a style string (resolved by the console's
theme when printed) or a `Style`. Rich returns its internal list; rs_rich
returns a new list each time, so change spans by assigning `text.spans`, or
with the methods below.

| Member | Meaning |
|---|---|
| `style`, `justify`, `overflow`, `no_wrap`, `end`, `tab_size` | Settable, as the constructor's arguments. `style` is `""` when unset. |
| `plain` | Settable; a shorter string trims the spans past its end. |
| `stylize(style, start=0, end=None)`, `stylize_before(...)` | Add a span on top of (or under) the others. |
| `highlight_regex(pattern, style=None, *, style_prefix="")` | Style each match of a Python regular expression (a string or compiled pattern); `style` may be a callable taking the matched text. Named groups get the style `style_prefix + name`. Returns the number of matches. |
| `highlight_words(words, style, *, case_sensitive=True)` | Style every occurrence of each word. |
| `copy_styles(text)` | Add another text's spans. |
| `get_style_at_offset(console, offset)` | The `Style` of one character, resolved by `console`. |
| `markup` | Console markup that renders this text. |
| `render(console, end="")` | The text as `Segment`s, without wrapping. |

```python
from rs_rich.text import Text

text = Text("foo 123 bar 45")
count = text.highlight_regex(r"(?P<word>[a-z]+) (?P<number>\d+)", style_prefix="repr.")
text.highlight_words(["bar"], "bold")
print(count)
print(text.spans)
print(text.markup)
```

```text
2
[Span(0, 3, 'repr.word'), Span(4, 7, 'repr.number'), Span(8, 11, 'repr.word'), Span(12, 14, 'repr.number'), Span(8, 11, 'bold')]
[repr.word]foo[/repr.word] [repr.number]123[/repr.number] [repr.word][bold]bar[/repr.word][/bold] [repr.number]45[/repr.number]
```

## Editing

| Method | Meaning |
|---|---|
| `append(text, style=None)`, `append_text(text)`, `append_tokens(tokens)` | Add to the end; each returns the text. |
| `copy()`, `blank_copy(plain="")` | A copy, or an empty text with the same settings. |
| `+`, `==`, `in`, `text[i]`, `text[a:b]` | As in Rich: `+` copies, `==` compares plain and spans, indexing and slicing (step 1) give new texts with the spans over them. |
| `rstrip()`, `rstrip_end(size)`, `remove_suffix(suffix)`, `right_crop(amount=1)` | Remove from the end. |
| `pad(count, character=" ")`, `pad_left`, `pad_right`, `set_length(length)`, `extend_style(spaces)` | Add to the ends. |
| `truncate(max_width, *, overflow=None, pad=False)`, `align(align, width, character=" ")` | Fit a width in cells. |
| `expand_tabs(tab_size=None)` | Replace tabs with spaces. |
| `detect_indentation()`, `with_indent_guides(indent_size=None, *, character="│", style="dim green")` | Indentation of code, and a copy with guide lines. |

```python
from rs_rich.text import Text

text = Text("Hello") + Text(" World", style="bold")
print(repr(text[6:]))
text.align("center", 15, "·")
print(text.plain)
code = Text("if x:\n    if y:\n        z()\n")
print(code.with_indent_guides().plain)
```

```text
<text 'World' [Span(0, 5, 'bold')] ''>
··Hello World··
if x:
│   if y:
│   │   z()

```

## Lines: split, divide, wrap and fit

These return a `Lines`, a list of `Text`s (`rich.containers.Lines`, also
`rs_rich.text.Lines`) that prints one text per line and has Rich's
`justify(console, width, justify="left", overflow="fold")`.

| Method | Meaning |
|---|---|
| `split(separator="\n", *, include_separator=False, allow_blank=False)` | Split at a separator. |
| `divide(offsets)` | Cut at character offsets. |
| `wrap(console, width, *, justify=None, overflow=None, tab_size=8, no_wrap=None)` | Word-wrap to `width` cells, as printing does. |
| `fit(width)` | Split into lines, each padded or cut to `width` characters. |
| `join(lines)` | Join texts with this text between them. |

```python
from rs_rich.console import Console
from rs_rich.text import Text

console = Console(width=40)
lines = Text("The quick brown fox jumps over the lazy dog").wrap(console, 16, justify="full")
for line in lines:
    print(repr(line.plain))
print(Text(", ").join(Text(word) for word in ["a", "b", "c"]))
```

```text
'The quick  brown'
'fox  jumps  over'
'the lazy dog'
a, b, c
```

## Markup and emoji

`rs_rich.markup` has Rich's `escape`, `render(markup, style="", emoji=True,
emoji_variant=None)` (the function behind `Text.from_markup`), `Tag` and
`MarkupError`. `rs_rich.emoji` has `Emoji(name, style="none", variant=None)`,
a renderable single emoji, `Emoji.replace(text)`, and `NoEmoji`, raised for
an unknown name.

```python
from rs_rich.emoji import Emoji, NoEmoji
from rs_rich.markup import Tag, render

print(repr(render("[bold]hi[/] :smiley:")))
print(Tag("link", "https://example.com").markup)
print(Emoji.replace("Launch :rocket:"), repr(Emoji("rocket")))
try:
    Emoji("no_such_emoji")
except NoEmoji as error:
    print(error)
```

```text
<text 'hi 😃' [Span(0, 2, 'bold')] ''>
[link=https://example.com]
Launch 🚀 <emoji 'rocket'>
No emoji called 'no_such_emoji'
```

## Differences from Rich

- `text.spans` returns a new list; assign `text.spans` to change the spans.
- Spans cannot hold meta data: `apply_meta`, `on` and `assemble(meta=...)`
  raise `NotImplementedError`. Core spans hold a `Style` without meta
  (Textual's event handlers), so the data would be lost.
- A zero-width span is dropped by the methods that rebuild the spans
  (`spans =`, `plain =`, `stylize_before`, `extend_style`); Rich keeps it.
  It styles nothing either way.
- Emoji codes inside a markup tag are replaced too, as `Console.print` does.
