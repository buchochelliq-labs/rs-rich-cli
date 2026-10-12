"""The pictures on docs/python/markdown.md: run it, or scripts/capture_python_docs.py."""

from _shots import Shots

from rs_rich.markdown import Markdown
from rs_rich.mermaid import MermaidFences

# --8<-- [start:document]
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
# --8<-- [end:document]


def shopping(console):
    # --8<-- [start:document-print]
    console.print(Markdown(document, hyperlinks=False))
    # --8<-- [end:document-print]


def code(console):
    # --8<-- [start:code]
    console.print(Markdown("```python\ndef add(a, b):\n    return a + b\n```"))
    # --8<-- [end:code]


def fences(console):
    # --8<-- [start:fences]
    diagram = "```mermaid\nflowchart LR\n    A[Write] --> B[Print]\n```"
    console.print(Markdown(diagram, fences=[MermaidFences()]))
    # --8<-- [end:fences]


shots = Shots("markdown")
shots.shot("document", shopping, width=40)
shots.shot("code", code, width=30)
shots.shot("fences", fences, width=40)
shots.finish()
