# Syntax

```python
from rs_rich.syntax import Syntax
```

`Syntax` highlights source code, as `rich.syntax.Syntax` does: with line
numbers, a highlighted line, a line range, wrapping, indent guides, padding
and a background colour. The highlighting engine is the port's own (see
[Colours](#colours)).

## Constructor

```text
Syntax(code, lexer, *, theme="monokai", dedent=False, line_numbers=False,
       start_line=1, line_range=None, highlight_lines=None, code_width=None,
       tab_size=4, word_wrap=False, background_color=None,
       indent_guides=False, padding=0, highlighter=None)
```

| Argument | Meaning |
|---|---|
| `code` | The source. |
| `lexer` | A language name or file extension (`"python"`, `"rs"`). An unknown one is plain text. |
| `theme` | A theme name of the code highlighter ([Themes](#themes)). |
| `dedent` | Remove common leading whitespace first. |
| `line_numbers`, `start_line` | Number the lines, starting at `start_line`. |
| `line_range` | `(first, last)` lines to show (1-based; either may be `None`). |
| `highlight_lines` | Line numbers to mark with `❱`. |
| `code_width` | The code's width, excluding line numbers (default: all available). |
| `tab_size` | Tab stops, in characters. |
| `word_wrap` | Wrap long lines instead of cropping them. |
| `background_color` | A colour to use instead of the theme's background. |
| `indent_guides` | Draw `│` guides in the indentation. |
| `padding` | `n`, `(vertical, horizontal)` or `(top, right, bottom, left)`, in the theme's background. |
| `highlighter` | Not in Rich: the [code highlighter](#code-highlighters) by name. `None` is the console's default, else syntect. |

```python
from rs_rich.console import Console
from rs_rich.syntax import Syntax

code = '''def fib(n):
    if n < 2:
        return n
    return fib(n - 1) + fib(n - 2)
'''
console = Console(width=44)
console.print(Syntax(code, "python", line_numbers=True, highlight_lines={3}))
console.print(Syntax(code, "python", line_range=(2, 3), indent_guides=True))
```

```text
  1 def fib(n):                             
  2     if n < 2:                           
❱ 3         return n                        
  4     return fib(n - 1) + fib(n - 2)      
  5                                         
│   if n < 2:                               
│   │   return n                            
```

## Methods

| Method | Meaning |
|---|---|
| `Syntax.from_path(path, encoding="utf-8", lexer=None, ...)` | Read a file; the lexer is guessed from its name when not given. Takes the constructor's other arguments. |
| `Syntax.guess_lexer(path, code=None)` | The language for a file name, lower-cased (`"python"`, `"rust"`), or `"default"`. Rich asks Pygments, which also looks at `code`; rs_rich decides by the name. |
| `Syntax.get_theme(name)` | The theme name the default highlighter will use for `name`: `name` itself, or its default theme. Rich returns a `SyntaxTheme` object. |
| `highlight(code, line_range=None)` | The highlighted `Text` (ending with a newline, as Pygments' output does). |
| `stylize_range(style, start, end, style_before=False)` | Style `start` to `end` (`(line, column)`, lines from 1, columns from 0) when the code renders. |

```python
syntax = Syntax("total = price * count", "python")
syntax.stylize_range("reverse", (1, 8), (1, 13))
print(syntax.highlight("total = 1").plain, end="")
print(Syntax.guess_lexer("main.rs"))
```

```text
total = 1
rust
```

## Code highlighters

Rich highlights with Pygments. rs_rich highlights with a Rust *code
highlighter*: `syntect` in every build, and `lumis` (tree-sitter) in the
separate lumis build. Choose one per `Syntax` (or `Markdown`) with
`highlighter=`; `code_highlighters()` lists what the build has, and
`code_themes(name)` a highlighter's themes.

```python
from rs_rich.syntax import code_highlighters, code_themes

print(code_highlighters())
print([theme for theme in code_themes("syntect") if theme.startswith("ansi")])
```

```text
['syntect']
['ansi_dark', 'ansi_light']
```

An unknown name raises `ValueError` listing the known ones.

## Themes

Theme names are the code highlighter's. syntect has its bundled themes
(`base16-ocean.dark`, `InspiredGitHub`, `Solarized (dark)`, ...) plus Rich's
`ansi_dark` and `ansi_light`, which use the terminal's own palette. A name
the highlighter does not have, such as Rich's default `monokai`, uses its
default theme (`base16-ocean.dark` for syntect).

## Colours

Layout is Rich's byte for byte: line numbers, ranges, wrapping, padding,
indent guides and the highlighted-line marker all match rich 15.0.0 in
plain output. Token colours are the engine's:

| Theme | Rich (Pygments) | rs_rich (syntect) |
|---|---|---|
| `monokai` (Rich's default) | background `#272822`, text `#f8f8f2`, keywords `#66d9ef`, functions `#a6e22e`, operators `#ff4689` | no such theme: `base16-ocean.dark`, background `#2b303b`, text `#c0c5ce`, keywords `#b48ead`, functions `#8fa1b3`, operators in the text colour |
| `ansi_dark`, `ansi_light` | Pygments tokens in the terminal's 16 colours | the same colours for the same token classes, except that whitespace between tokens is not coloured `bright_black`, and a comment is drawn as two runs (`#` and the rest) in the same style |

Where the tokens agree, the colours are Rich's:

```python
import io

out = io.StringIO()
Console(file=out, width=20, force_terminal=True, color_system="standard").print(
    Syntax("x=[1,'a']", "python", theme="ansi_dark")
)
print(repr(out.getvalue()))
```

```text
"x=[\x1b[94m1\x1b[0m,\x1b[33m'\x1b[0m\x1b[33ma\x1b[0m\x1b[33m'\x1b[0m]\n"
```
