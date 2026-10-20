"""The pictures on docs/python/mermaid.md: run it, or scripts/capture_python_docs.py."""

from _shots import Shots

from rs_rich import mermaid
from rs_rich.mermaid import Mermaid


def flowchart(console):
    # --8<-- [start:flowchart]
    console.print(Mermaid("graph LR\n  A[Start] --> B{Ready?}\n  B -->|yes| C((Go))"))
    console.print(Mermaid("graph TD\n  A --> B", ascii=True))
    # --8<-- [end:flowchart]


def sequence_er(console):
    # --8<-- [start:sequence-er]
    console.print(Mermaid("sequenceDiagram\n  Alice->>+Bob: Hi\n  Bob-->>-Alice: Hello"))
    console.print(Mermaid("erDiagram\n  direction LR\n  CUSTOMER ||--o{ ORDER : places"))
    # --8<-- [end:sequence-er]


def fallback(console):
    # --8<-- [start:fallback]
    console.print(Mermaid("classDiagram\n  Animal <|-- Duck"))
    # --8<-- [end:fallback]


def fences(console):
    # --8<-- [start:fences]
    fences = mermaid.MermaidFences()
    print(fences.accepts("mermaid"), fences.accepts("python"))
    console.print(fences.render_fence("mermaid", "graph LR\n  x --> y"))
    # --8<-- [end:fences]


shots = Shots("mermaid")
shots.shot("flowchart", flowchart, width=50)
shots.shot("sequence-er", sequence_er, width=50)
shots.shot("fallback", fallback, width=80)
shots.shot("fences", fences, width=80)
shots.finish()
