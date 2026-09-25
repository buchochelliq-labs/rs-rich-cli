# Markdown

```python
from rs_rich.markdown import Markdown
```

`Markdown` renders a CommonMark document (with tables and strikethrough), as
`rich.markdown.Markdown` does. Rendering is core's port of Rich's.

## Constructor

```text
Markdown(markup, code_theme="monokai", justify=None, style="none",
         hyperlinks=True, inline_code_lexer=None, inline_code_theme=None,
         *, highlighter=None)
```

| Argument | Meaning |
|---|---|
| `markup` | The document. |
| `code_theme` | The theme for code blocks, by the code highlighter's name for it (see [Syntax](syntax.md#themes)). An unknown name uses the highlighter's default theme. |
| `justify` | `"left"`, `"center"`, `"right"` or `"full"` for paragraphs. |
| `style` | The style the document is drawn in: a style, a definition or a theme name. |
| `hyperlinks` | `True` draws `[text](url)` as a terminal hyperlink; `False` writes the URL after the text. |
| `inline_code_lexer` | Highlight inline code as this language. |
| `inline_code_theme` | The theme for highlighted inline code (default: `code_theme`). |
| `highlighter` | Not in Rich: the [code highlighter](syntax.md#code-highlighters) for code blocks, by name (`"syntect"`, or `"lumis"` in a lumis build). `None` is the console's default. |

The arguments are readable as attributes of the same names.

```python
from rs_rich.console import Console
from rs_rich.markdown import Markdown

document = """# Shopping

* **apples**, the *green* ones
* `pears`

1. go
2. come back

| item | count |
|---|--:|
| apples | 3 |

See [the list](https://example.com).
"""
Console(width=40).print(Markdown(document, hyperlinks=False))
```

```text
                Shopping                

 • apples, the green ones               
 • pears                                

 1 go                                   
 2 come back                            

               
 item    count 
 ───────────── 
 apples      3 
               

See the list (https://example.com).     
```

A code block is highlighted, with a one-cell margin and the theme's
background:

```python
Console(width=30).print(Markdown("```python\ndef add(a, b):\n    return a + b\n```"))
```

```text
                              
 def add(a, b):               
     return a + b             
                              
```

## Differences from Rich

Output is Rich's byte for byte, in colour and without, except where code is
highlighted: code block and inline-code colours come from the port's code
highlighter, not Pygments (see [Syntax](syntax.md#colours)). With
`inline_code_lexer` set, inline code keeps the lexer's colours but not Rich's
`markdown.code` style under them.
