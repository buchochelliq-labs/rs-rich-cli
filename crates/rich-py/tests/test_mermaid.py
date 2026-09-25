"""``rs_rich.mermaid``: the ``rich-mermaid`` crate from Python.

Rich has no Mermaid support, so each case in ``CASES`` is compared with what
the Rust crate renders for the same source: ``EXPECTED`` was produced by the
same Rust oracle as ``test_art.py`` (``target/rich-py/art-oracle``), reading
these cases as JSON.
"""

from __future__ import annotations

import io

import pytest

from rs_rich import mermaid
from rs_rich.console import Console

PLAIN = {"width": 60}
COLOR = {"width": 60, "color": True}

SHAPES = """flowchart TD
  a[rect] --> b(round) --> c([stadium]) --> d[[subroutine]]
  d --> e[(cylinder)] --> f((circle)) --> g(((double)))
  g --> h>asymmetric] --> i{rhombus} --> j{{hexagon}}
  j --> k[/para/] --> l[\\alt\\] --> m[/trap\\] --> n[\\trapalt/]
"""

EDGES = """graph LR
  A -- label --> B
  B -.-> C
  C ==> D
  D --o E
  E --x F
  F <--> G
  G ---> H
  H ~~~ I
  I & J --> K
"""

SUBGRAPH = """flowchart TB
  subgraph one [First]
    a1 --> a2
  end
  a2 --> b1
  style a1 fill:#f9f
  classDef big font-size:20px
"""

TOO_WIDE = "graph TD\n" + "\n".join(f"n{i} --> n{i + 1}" for i in range(199)) + "\n" + "\n".join(
    f"n0 --> n{i}" for i in range(2, 200)
)
TOO_MANY = "graph TD\n" + "\n".join(f"a{i} --> b{i}" for i in range(501))


def case(name, kind, opts, console=PLAIN):
    return {"name": name, "kind": kind, "opts": opts, "console": console, "image": None}


CASES = [
    case("render_lr", "mermaid", {"source": "graph LR\n  A --> B"}, {"width": 40}),
    case("render_td_labels", "mermaid", {"source": "graph TD\n  A[Start] --> B{Is it?}\n  B -->|Yes| C[OK]\n  B -->|No| D[End]"}),
    case("render_shapes", "mermaid", {"source": SHAPES}, {"width": 100}),
    case("render_edges", "mermaid", {"source": EDGES}, {"width": 120}),
    case("render_bt", "mermaid", {"source": "graph BT\n  A --> B --> C"}),
    case("render_rl", "mermaid", {"source": "graph RL\n  A --> B --> C"}),
    case("render_ascii", "mermaid", {"source": "graph LR\n  A --> B --> C", "ascii": True}),
    case("render_subgraph_note", "mermaid", {"source": SUBGRAPH}, COLOR),
    case("render_cropped", "mermaid", {"source": "graph LR\n  Alpha --> Beta --> Gamma --> Delta"}, {"width": 20}),
    case("render_sequence_source", "mermaid", {"source": "sequenceDiagram\n  Alice->>Bob: Hi"}, COLOR),
    case("render_syntax_error", "mermaid", {"source": "graph TD\n  A --> "}),
    case("render_empty", "mermaid", {"source": "%% nothing\n"}),
    case("render_no_nodes", "mermaid", {"source": "graph TD\n"}),
    case("render_too_wide", "mermaid", {"source": TOO_WIDE}, {"width": 40}),
    case("render_mmdc_fallback", "mermaid", {"source": "graph LR\n  A --> B", "backend": "mmdc"}),
    case("parse_edges", "mermaid_parse", {"source": EDGES}),
    case("parse_shapes", "mermaid_parse", {"source": SHAPES}),
    case("parse_subgraph", "mermaid_parse", {"source": SUBGRAPH}),
    case("parse_empty", "mermaid_parse", {"source": ""}),
    case("parse_unsupported", "mermaid_parse", {"source": "pie\n  \"a\": 1"}),
    case("parse_syntax", "mermaid_parse", {"source": "graph TD\n  A --> B\n  B -->"}),
    case("parse_too_many", "mermaid_parse", {"source": TOO_MANY}),
    case("draw_td", "mermaid_draw", {"source": "graph TD\n  A --> B & C"}),
    case("draw_ascii", "mermaid_draw", {"source": "graph LR\n  A((x)) -.-> B", "ascii": True}),
    case("draw_too_wide", "mermaid_draw", {"source": TOO_WIDE}),
    case("clean_label", "clean_label", {"text": "a<br>b &amp; c &lt;d&gt; <br/>e"}),
]


def camel(name: str) -> str:
    return "".join(part.capitalize() for part in name.split("_"))


def parse_error(error: mermaid.MermaidParseError) -> dict:
    return {"error": str(error), "kind": error.kind, "line": error.line}


def run(case: dict) -> dict:
    kind, opts = case["kind"], case["opts"]
    if kind == "mermaid":
        spec = case["console"]
        color = spec.get("color", False)
        console = Console(
            file=io.StringIO(), width=spec["width"], force_terminal=color, color_system="truecolor" if color else None
        )
        options = {key: opts[key] for key in ("backend", "ascii") if key in opts}
        console.print(mermaid.Mermaid(opts["source"], **options))
        return {"out": console.file.getvalue()}
    if kind == "mermaid_parse":
        try:
            chart = mermaid.parse_flowchart(opts["source"])
        except mermaid.MermaidParseError as error:
            return parse_error(error)
        return {
            "out": {
                "direction": chart.direction,
                "nodes": [[node.id, node.label, camel(node.shape)] for node in chart.nodes],
                "edges": [
                    {
                        "source": edge.source,
                        "target": edge.target,
                        "label": edge.label,
                        "stroke": edge.stroke,
                        "start": edge.start,
                        "end": edge.end,
                        "length": edge.length,
                    }
                    for edge in chart.edges
                ],
                "notes": chart.notes,
            }
        }
    if kind == "mermaid_draw":
        try:
            diagram = mermaid.draw_flowchart(opts["source"], ascii=opts.get("ascii", False))
        except mermaid.MermaidParseError as error:
            return parse_error(error)
        except mermaid.MermaidLayoutError as error:
            return {"error": str(error), "kind": error.kind}
        return {"lines": diagram.lines, "width": diagram.width}
    if kind == "clean_label":
        return {"out": mermaid.mermaid_clean_label(opts["text"])}
    raise AssertionError(kind)


def digest(value):
    return value


@pytest.mark.parametrize("case", CASES, ids=[c["name"] for c in CASES])
def test_matches_the_rust_crate(case):
    assert run(case) == EXPECTED[case["name"]]


def test_every_case_has_an_expectation():
    assert sorted(EXPECTED) == sorted(c["name"] for c in CASES)


def test_a_simple_flowchart():
    console = Console(file=io.StringIO(), width=40, color_system=None)
    console.print(mermaid.Mermaid("graph LR\n  A --> B"))
    assert console.file.getvalue() == "┌───┐  ┌───┐\n│ A ├─►│ B │\n└───┘  └───┘\n"


def test_parse_errors_carry_kind_and_line():
    with pytest.raises(mermaid.MermaidParseError) as raised:
        mermaid.parse("graph TD\n  A --> B\n  B -->")
    assert (raised.value.kind, raised.value.line) == ("syntax", 3)
    with pytest.raises(mermaid.MermaidParseError) as raised:
        mermaid.parse("sequenceDiagram\n  A->>B: hi")
    assert (raised.value.kind, raised.value.line) == ("unsupported", None)
    assert str(raised.value) == "sequence diagrams are not drawn as text"
    assert issubclass(mermaid.MermaidParseError, mermaid.MermaidError)
    assert issubclass(mermaid.MermaidLayoutError, mermaid.MermaidError)


def test_flowchart_objects():
    chart = mermaid.parse("graph LR\n  A[Start] -->|go| B((End))")
    assert repr(chart) == "<Flowchart LR nodes=2 edges=1>"
    assert repr(chart.nodes[1]) == "FlowchartNode(id='B', label='End', shape='circle')"
    assert repr(chart.edges[0]) == (
        "FlowchartEdge(source=0, target=1, label='go', stroke='solid', start=None, end='arrow', length=1)"
    )
    diagram = mermaid.draw(chart)
    assert str(diagram) == "\n".join(diagram.lines)
    assert mermaid.draw(chart).lines == mermaid.draw_flowchart("graph LR\n  A[Start] -->|go| B((End))").lines
    with pytest.raises(TypeError):
        mermaid.draw_flowchart(42)


def test_limits():
    assert (mermaid.MAX_SOURCE, mermaid.MAX_NODES, mermaid.MAX_EDGES, mermaid.MAX_LINK_LENGTH) == (65536, 500, 2000, 10)
    with pytest.raises(mermaid.MermaidParseError) as raised:
        mermaid.parse("graph TD\n" + "%%" * mermaid.MAX_SOURCE)
    assert raised.value.kind == "too_large"


def test_fences():
    fences = mermaid.MermaidFences(ascii=True)
    assert fences.accepts("Mermaid") and not fences.accepts("python")
    assert fences.render_fence("python", "print()") is None
    diagram = fences.render_fence("mermaid", "graph LR\n  A --> B")
    assert (diagram.source, diagram.ascii, diagram.backend) == ("graph LR\n  A --> B", True, "text")
    # The rs_rich.plugins signature works too.
    assert fences.render_fence("mermaid", "graph LR\n  A --> B", None, None).source == diagram.source


def test_mmdc_options():
    options = mermaid.MmdcOptions(program="/opt/mmdc", timeout=5.0, puppeteer_config="p.json")
    assert (options.program, options.timeout, options.background) == ("/opt/mmdc", 5.0, "white")
    assert (options.max_input, options.max_output) == (64 * 1024, 16 * 1024 * 1024)
    assert repr(options).startswith("MmdcOptions(program='/opt/mmdc', timeout=5.0,")
    assert mermaid.Mermaid("graph LR\n  A", backend="mmdc", mmdc=options).mmdc.program == "/opt/mmdc"
    with pytest.raises(ValueError, match="timeout"):
        mermaid.MmdcOptions(timeout=0)
    with pytest.raises(ValueError, match="invalid backend"):
        mermaid.Mermaid("graph LR", backend="kroki")


@pytest.mark.skipif(mermaid.HAS_MMDC, reason="this build has the mmdc backend")
def test_render_png_needs_an_mmdc_build():
    with pytest.raises(NotImplementedError, match="mmdc"):
        mermaid.render_png("graph LR\n  A --> B")


@pytest.mark.skipif(not mermaid.HAS_MMDC, reason="this build has no mmdc backend")
def test_render_png_reports_a_missing_program():
    with pytest.raises(mermaid.MmdcError) as raised:
        mermaid.render_png("graph LR\n  A --> B", mermaid.MmdcOptions(program="/nonexistent/mmdc"))
    assert raised.value.kind == "not_found"


def test_a_diagram_inside_a_panel():
    from rs_rich.panel import Panel

    console = Console(file=io.StringIO(), width=40, color_system=None)
    console.print(Panel(mermaid.Mermaid("graph LR\n  A --> B"), expand=False))
    assert console.file.getvalue().splitlines()[2] == "│ │ A ├─►│ B │ │"


# Generated by the Rust oracle; do not edit by hand.
# fmt: off
EXPECTED: dict = {'clean_label': {'out': 'a\nb &amp; c &lt;d&gt;\ne'},
 'draw_ascii': {'lines': ['.---.  +---+', '( x ).>| B |', "'---'  +---+"], 'width': 12},
 'draw_td': {'lines': ['    ┌───┐',
                       '    │ A │',
                       '    └┬─┬┘',
                       '     │ │',
                       '  ┌──┘ └─┐',
                       '  ▼      ▼',
                       '┌───┐  ┌───┐',
                       '│ B │  │ C │',
                       '└───┘  └───┘'],
             'width': 12},
 'draw_too_wide': {'error': 'too large to draw: its edges cross 19901 rank positions, more than 5000',
                   'kind': 'too_large'},
 'parse_edges': {'out': {'direction': 'LR',
                         'edges': [{'end': 'arrow',
                                    'label': 'label',
                                    'length': 1,
                                    'source': 0,
                                    'start': None,
                                    'stroke': 'solid',
                                    'target': 1},
                                   {'end': 'arrow',
                                    'label': None,
                                    'length': 1,
                                    'source': 1,
                                    'start': None,
                                    'stroke': 'dotted',
                                    'target': 2},
                                   {'end': 'arrow',
                                    'label': None,
                                    'length': 1,
                                    'source': 2,
                                    'start': None,
                                    'stroke': 'thick',
                                    'target': 3},
                                   {'end': 'circle',
                                    'label': None,
                                    'length': 1,
                                    'source': 3,
                                    'start': None,
                                    'stroke': 'solid',
                                    'target': 4},
                                   {'end': 'cross',
                                    'label': None,
                                    'length': 1,
                                    'source': 4,
                                    'start': None,
                                    'stroke': 'solid',
                                    'target': 5},
                                   {'end': 'arrow',
                                    'label': None,
                                    'length': 1,
                                    'source': 5,
                                    'start': 'arrow',
                                    'stroke': 'solid',
                                    'target': 6},
                                   {'end': 'arrow',
                                    'label': None,
                                    'length': 2,
                                    'source': 6,
                                    'start': None,
                                    'stroke': 'solid',
                                    'target': 7},
                                   {'end': None,
                                    'label': None,
                                    'length': 1,
                                    'source': 7,
                                    'start': None,
                                    'stroke': 'invisible',
                                    'target': 8},
                                   {'end': 'arrow',
                                    'label': None,
                                    'length': 1,
                                    'source': 8,
                                    'start': None,
                                    'stroke': 'solid',
                                    'target': 10},
                                   {'end': 'arrow',
                                    'label': None,
                                    'length': 1,
                                    'source': 9,
                                    'start': None,
                                    'stroke': 'solid',
                                    'target': 10}],
                         'nodes': [['A', 'A', 'Rect'],
                                   ['B', 'B', 'Rect'],
                                   ['C', 'C', 'Rect'],
                                   ['D', 'D', 'Rect'],
                                   ['E', 'E', 'Rect'],
                                   ['F', 'F', 'Rect'],
                                   ['G', 'G', 'Rect'],
                                   ['H', 'H', 'Rect'],
                                   ['I', 'I', 'Rect'],
                                   ['J', 'J', 'Rect'],
                                   ['K', 'K', 'Rect']],
                         'notes': []}},
 'parse_empty': {'error': 'the diagram is empty', 'kind': 'empty', 'line': None},
 'parse_shapes': {'out': {'direction': 'TD',
                          'edges': [{'end': 'arrow',
                                     'label': None,
                                     'length': 1,
                                     'source': 0,
                                     'start': None,
                                     'stroke': 'solid',
                                     'target': 1},
                                    {'end': 'arrow',
                                     'label': None,
                                     'length': 1,
                                     'source': 1,
                                     'start': None,
                                     'stroke': 'solid',
                                     'target': 2},
                                    {'end': 'arrow',
                                     'label': None,
                                     'length': 1,
                                     'source': 2,
                                     'start': None,
                                     'stroke': 'solid',
                                     'target': 3},
                                    {'end': 'arrow',
                                     'label': None,
                                     'length': 1,
                                     'source': 3,
                                     'start': None,
                                     'stroke': 'solid',
                                     'target': 4},
                                    {'end': 'arrow',
                                     'label': None,
                                     'length': 1,
                                     'source': 4,
                                     'start': None,
                                     'stroke': 'solid',
                                     'target': 5},
                                    {'end': 'arrow',
                                     'label': None,
                                     'length': 1,
                                     'source': 5,
                                     'start': None,
                                     'stroke': 'solid',
                                     'target': 6},
                                    {'end': 'arrow',
                                     'label': None,
                                     'length': 1,
                                     'source': 6,
                                     'start': None,
                                     'stroke': 'solid',
                                     'target': 7},
                                    {'end': 'arrow',
                                     'label': None,
                                     'length': 1,
                                     'source': 7,
                                     'start': None,
                                     'stroke': 'solid',
                                     'target': 8},
                                    {'end': 'arrow',
                                     'label': None,
                                     'length': 1,
                                     'source': 8,
                                     'start': None,
                                     'stroke': 'solid',
                                     'target': 9},
                                    {'end': 'arrow',
                                     'label': None,
                                     'length': 1,
                                     'source': 9,
                                     'start': None,
                                     'stroke': 'solid',
                                     'target': 10},
                                    {'end': 'arrow',
                                     'label': None,
                                     'length': 1,
                                     'source': 10,
                                     'start': None,
                                     'stroke': 'solid',
                                     'target': 11},
                                    {'end': 'arrow',
                                     'label': None,
                                     'length': 1,
                                     'source': 11,
                                     'start': None,
                                     'stroke': 'solid',
                                     'target': 12},
                                    {'end': 'arrow',
                                     'label': None,
                                     'length': 1,
                                     'source': 12,
                                     'start': None,
                                     'stroke': 'solid',
                                     'target': 13}],
                          'nodes': [['a', 'rect', 'Rect'],
                                    ['b', 'round', 'Round'],
                                    ['c', 'stadium', 'Stadium'],
                                    ['d', 'subroutine', 'Subroutine'],
                                    ['e', 'cylinder', 'Cylinder'],
                                    ['f', 'circle', 'Circle'],
                                    ['g', 'double', 'DoubleCircle'],
                                    ['h', 'asymmetric', 'Asymmetric'],
                                    ['i', 'rhombus', 'Rhombus'],
                                    ['j', 'hexagon', 'Hexagon'],
                                    ['k', 'para', 'Parallelogram'],
                                    ['l', 'alt', 'ParallelogramAlt'],
                                    ['m', 'trap', 'Trapezoid'],
                                    ['n', 'trapalt', 'TrapezoidAlt']],
                          'notes': []}},
 'parse_subgraph': {'out': {'direction': 'TD',
                            'edges': [{'end': 'arrow',
                                       'label': None,
                                       'length': 1,
                                       'source': 0,
                                       'start': None,
                                       'stroke': 'solid',
                                       'target': 1},
                                      {'end': 'arrow',
                                       'label': None,
                                       'length': 1,
                                       'source': 1,
                                       'start': None,
                                       'stroke': 'solid',
                                       'target': 2}],
                            'nodes': [['a1', 'a1', 'Rect'], ['a2', 'a2', 'Rect'], ['b1', 'b1', 'Rect']],
                            'notes': ['subgraphs are drawn without their frames']}},
 'parse_syntax': {'error': 'line 3: an arrow needs a node after it', 'kind': 'syntax', 'line': 3},
 'parse_too_many': {'error': 'the diagram is too large: more than 500 nodes',
                    'kind': 'too_large',
                    'line': None},
 'parse_unsupported': {'error': 'pie chart diagrams are not drawn as text',
                       'kind': 'unsupported',
                       'line': None},
 'render_ascii': {'out': '+---+  +---+  +---+\n| A +->| B +->| C |\n+---+  +---+  +---+\n\n'},
 'render_bt': {'out': '┌───┐\n'
                      '│ C │\n'
                      '└───┘\n'
                      '  ▲\n'
                      '  │\n'
                      '┌─┴─┐\n'
                      '│ B │\n'
                      '└───┘\n'
                      '  ▲\n'
                      '  │\n'
                      '┌─┴─┐\n'
                      '│ A │\n'
                      '└───┘\n'
                      '\n'},
 'render_cropped': {'out': '┌───────┐  ┌──────┐\n'
                           '│ Alpha ├─►│ Beta ├─\n'
                           '└───────┘  └──────┘\n'
                           'Mermaid: cropped to \n'
                           '20 of 41 columns\n'
                           '\n'},
 'render_edges': {'out': '┌───┐         ┌───┐  ┌───┐  ┌───┐  ┌───┐  ┌───┐  ┌───┐     ┌───┐  ┌───┐\n'
                         '│ A ├──label─►│ B ├┄►│ C ├━►│ D ├─●│ E ├─×│ F │◄►│ G ├────►│ H │  │ I ├─┐ ┌───┐\n'
                         '└───┘         └───┘  └───┘  └───┘  └───┘  └───┘  └───┘     └───┘  └───┘ └►│ K │\n'
                         '                                                                        ┌►│   │\n'
                         '                                                                  ┌───┐ │ └───┘\n'
                         '                                                                  │ J ├─┘\n'
                         '                                                                  └───┘\n'
                         '\n'},
 'render_empty': {'out': 'Mermaid: the diagram is empty\n'
                         '                                                            \n'
                         ' %% nothing                                                 \n'
                         '                                                            \n'},
 'render_lr': {'out': '┌───┐  ┌───┐\n│ A ├─►│ B │\n└───┘  └───┘\n\n'},
 'render_mmdc_fallback': {'out': '┌───┐  ┌───┐\n'
                                 '│ A ├─►│ B │\n'
                                 '└───┘  └───┘\n'
                                 'Mermaid: this build has no mmdc backend; drawn as text\n'
                                 '\n'},
 'render_no_nodes': {'out': 'Mermaid: the flowchart has no nodes\n'
                            '                                                            \n'
                            ' graph TD                                                   \n'
                            '                                                            \n'},
 'render_rl': {'out': '┌───┐  ┌───┐  ┌───┐\n│ C │◄─┤ B │◄─┤ A │\n└───┘  └───┘  └───┘\n\n'},
 'render_sequence_source': {'out': '\x1b[2;3mMermaid: sequence diagrams are not drawn as text\x1b[0m\n'
                                   '\x1b[48;2;43;48;59m                                                            '
                                   '\x1b[0m\n'
                                   '\x1b[48;2;43;48;59m '
                                   '\x1b[0m\x1b[38;2;192;197;206;48;2;43;48;59msequenceDiagram\x1b[0m\x1b[48;2;43;48;59m                                            '
                                   '\x1b[0m\n'
                                   '\x1b[48;2;43;48;59m \x1b[0m\x1b[38;2;192;197;206;48;2;43;48;59m  '
                                   'Alice->>Bob: '
                                   'Hi\x1b[0m\x1b[48;2;43;48;59m                                          '
                                   '\x1b[0m\n'
                                   '\x1b[48;2;43;48;59m                                                            '
                                   '\x1b[0m\n'},
 'render_shapes': {'out': '    ┌───────┐\n'
                          '    │ rect  │\n'
                          '    └───┬───┘\n'
                          '        │\n'
                          '        ▼\n'
                          '    ╭───────╮\n'
                          '    │ round │\n'
                          '    ╰───┬───╯\n'
                          '        │\n'
                          '        ▼\n'
                          '   ╭─────────╮\n'
                          '   ( stadium )\n'
                          '   ╰────┬────╯\n'
                          '        │\n'
                          '        ▼\n'
                          '┌┬─────────────┬┐\n'
                          '││ subroutine  ││\n'
                          '└┴──────┬──────┴┘\n'
                          '        │\n'
                          '        ▼\n'
                          '  ╭───────────╮\n'
                          '  │ cylinder  │\n'
                          '  ╰─────┬─────╯\n'
                          '        │\n'
                          '        ▼\n'
                          '   ╭─────────╮\n'
                          '   ( circle  )\n'
                          '   ╰────┬────╯\n'
                          '        │\n'
                          '        ▼\n'
                          '   ╭─────────╮\n'
                          '   ( double  )\n'
                          '   ╰────┬────╯\n'
                          '        │\n'
                          '        ▼\n'
                          ' ┌─────────────┐\n'
                          ' > asymmetric  │\n'
                          ' └──────┬──────┘\n'
                          '        │\n'
                          '        ▼\n'
                          '   ╱─────────╲\n'
                          '   < rhombus >\n'
                          '   ╲────┬────╱\n'
                          '        │\n'
                          '        ▼\n'
                          '   ╱─────────╲\n'
                          '   │ hexagon │\n'
                          '   ╲────┬────╱\n'
                          '        │\n'
                          '        ▼\n'
                          '    ┌───────┐\n'
                          '    / para  /\n'
                          '    └───┬───┘\n'
                          '        │\n'
                          '        ▼\n'
                          '     ┌─────┐\n'
                          '     \\ alt \\\n'
                          '     └──┬──┘\n'
                          '        │\n'
                          '        ▼\n'
                          '    ┌───────┐\n'
                          '    / trap  \\\n'
                          '    └───┬───┘\n'
                          '        │\n'
                          '        ▼\n'
                          '   ┌─────────┐\n'
                          '   \\ trapalt /\n'
                          '   └─────────┘\n'
                          '\n'},
 'render_subgraph_note': {'out': '┌─────┐\n'
                                 '│ a1  │\n'
                                 '└──┬──┘\n'
                                 '   │\n'
                                 '   ▼\n'
                                 '┌─────┐\n'
                                 '│ a2  │\n'
                                 '└──┬──┘\n'
                                 '   │\n'
                                 '   ▼\n'
                                 '┌─────┐\n'
                                 '│ b1  │\n'
                                 '└─────┘\n'
                                 '\x1b[2;3mMermaid: subgraphs are drawn without their frames\x1b[0m\n'
                                 '\n'},
 'render_syntax_error': {'out': 'Mermaid: line 2: an arrow needs a node after it\n'
                                '                                                            \n'
                                ' graph TD                                                   \n'
                                '   A -->                                                    \n'
                                '                                                            \n'},
 'render_td_labels': {'out': '    ┌───────┐\n'
                             '    │ Start │\n'
                             '    └───┬───┘\n'
                             '        │\n'
                             '        ▼\n'
                             '   ╱─────────╲\n'
                             '   < Is it?  >\n'
                             '   ╲───┬─┬───╱\n'
                             '       │ │\n'
                             '   ┌───┘ └──┐\n'
                             '  Yes      No\n'
                             '   ▼        ▼\n'
                             '┌─────┐  ┌─────┐\n'
                             '│ OK  │  │ End │\n'
                             '└─────┘  └─────┘\n'
                             '\n'},
 'render_too_wide': {'out': 'Mermaid: too large to draw: its edges \n'
                            'cross 19901 rank positions, more than \n'
                            '5000\n'
                            '                                        \n'
                            ' graph TD                               \n'
                            ' n0 --> n1                              \n'
                            ' n1 --> n2                              \n'
                            ' n2 --> n3                              \n'
                            ' n3 --> n4                              \n'
                            ' n4 --> n5                              \n'
                            ' n5 --> n6                              \n'
                            ' n6 --> n7                              \n'
                            ' n7 --> n8                              \n'
                            ' n8 --> n9                              \n'
                            ' n9 --> n10                             \n'
                            ' n10 --> n11                            \n'
                            ' n11 --> n12                            \n'
                            ' n12 --> n13                            \n'
                            ' n13 --> n14                            \n'
                            ' n14 --> n15                            \n'
                            ' n15 --> n16                            \n'
                            ' n16 --> n17                            \n'
                            ' n17 --> n18                            \n'
                            ' n18 --> n19                            \n'
                            ' n19 --> n20                            \n'
                            ' n20 --> n21                            \n'
                            ' n21 --> n22                            \n'
                            ' n22 --> n23                            \n'
                            ' n23 --> n24                            \n'
                            ' n24 --> n25                            \n'
                            ' n25 --> n26                            \n'
                            ' n26 --> n27                            \n'
                            ' n27 --> n28                            \n'
                            ' n28 --> n29                            \n'
                            ' n29 --> n30                            \n'
                            ' n30 --> n31                            \n'
                            ' n31 --> n32                            \n'
                            ' n32 --> n33                            \n'
                            ' n33 --> n34                            \n'
                            ' n34 --> n35                            \n'
                            ' n35 --> n36                            \n'
                            ' n36 --> n37                            \n'
                            ' n37 --> n38                            \n'
                            ' n38 --> n39                            \n'
                            ' n39 --> n40                            \n'
                            ' n40 --> n41                            \n'
                            ' n41 --> n42                            \n'
                            ' n42 --> n43                            \n'
                            ' n43 --> n44                            \n'
                            ' n44 --> n45                            \n'
                            ' n45 --> n46                            \n'
                            ' n46 --> n47                            \n'
                            ' n47 --> n48                            \n'
                            ' n48 --> n49                            \n'
                            ' n49 --> n50                            \n'
                            ' n50 --> n51                            \n'
                            ' n51 --> n52                            \n'
                            ' n52 --> n53                            \n'
                            ' n53 --> n54                            \n'
                            ' n54 --> n55                            \n'
                            ' n55 --> n56                            \n'
                            ' n56 --> n57                            \n'
                            ' n57 --> n58                            \n'
                            ' n58 --> n59                            \n'
                            ' n59 --> n60                            \n'
                            ' n60 --> n61                            \n'
                            ' n61 --> n62                            \n'
                            ' n62 --> n63                            \n'
                            ' n63 --> n64                            \n'
                            ' n64 --> n65                            \n'
                            ' n65 --> n66                            \n'
                            ' n66 --> n67                            \n'
                            ' n67 --> n68                            \n'
                            ' n68 --> n69                            \n'
                            ' n69 --> n70                            \n'
                            ' n70 --> n71                            \n'
                            ' n71 --> n72                            \n'
                            ' n72 --> n73                            \n'
                            ' n73 --> n74                            \n'
                            ' n74 --> n75                            \n'
                            ' n75 --> n76                            \n'
                            ' n76 --> n77                            \n'
                            ' n77 --> n78                            \n'
                            ' n78 --> n79                            \n'
                            ' n79 --> n80                            \n'
                            ' n80 --> n81                            \n'
                            ' n81 --> n82                            \n'
                            ' n82 --> n83                            \n'
                            ' n83 --> n84                            \n'
                            ' n84 --> n85                            \n'
                            ' n85 --> n86                            \n'
                            ' n86 --> n87                            \n'
                            ' n87 --> n88                            \n'
                            ' n88 --> n89                            \n'
                            ' n89 --> n90                            \n'
                            ' n90 --> n91                            \n'
                            ' n91 --> n92                            \n'
                            ' n92 --> n93                            \n'
                            ' n93 --> n94                            \n'
                            ' n94 --> n95                            \n'
                            ' n95 --> n96                            \n'
                            ' n96 --> n97                            \n'
                            ' n97 --> n98                            \n'
                            ' n98 --> n99                            \n'
                            ' n99 --> n100                           \n'
                            ' n100 --> n101                          \n'
                            ' n101 --> n102                          \n'
                            ' n102 --> n103                          \n'
                            ' n103 --> n104                          \n'
                            ' n104 --> n105                          \n'
                            ' n105 --> n106                          \n'
                            ' n106 --> n107                          \n'
                            ' n107 --> n108                          \n'
                            ' n108 --> n109                          \n'
                            ' n109 --> n110                          \n'
                            ' n110 --> n111                          \n'
                            ' n111 --> n112                          \n'
                            ' n112 --> n113                          \n'
                            ' n113 --> n114                          \n'
                            ' n114 --> n115                          \n'
                            ' n115 --> n116                          \n'
                            ' n116 --> n117                          \n'
                            ' n117 --> n118                          \n'
                            ' n118 --> n119                          \n'
                            ' n119 --> n120                          \n'
                            ' n120 --> n121                          \n'
                            ' n121 --> n122                          \n'
                            ' n122 --> n123                          \n'
                            ' n123 --> n124                          \n'
                            ' n124 --> n125                          \n'
                            ' n125 --> n126                          \n'
                            ' n126 --> n127                          \n'
                            ' n127 --> n128                          \n'
                            ' n128 --> n129                          \n'
                            ' n129 --> n130                          \n'
                            ' n130 --> n131                          \n'
                            ' n131 --> n132                          \n'
                            ' n132 --> n133                          \n'
                            ' n133 --> n134                          \n'
                            ' n134 --> n135                          \n'
                            ' n135 --> n136                          \n'
                            ' n136 --> n137                          \n'
                            ' n137 --> n138                          \n'
                            ' n138 --> n139                          \n'
                            ' n139 --> n140                          \n'
                            ' n140 --> n141                          \n'
                            ' n141 --> n142                          \n'
                            ' n142 --> n143                          \n'
                            ' n143 --> n144                          \n'
                            ' n144 --> n145                          \n'
                            ' n145 --> n146                          \n'
                            ' n146 --> n147                          \n'
                            ' n147 --> n148                          \n'
                            ' n148 --> n149                          \n'
                            ' n149 --> n150                          \n'
                            ' n150 --> n151                          \n'
                            ' n151 --> n152                          \n'
                            ' n152 --> n153                          \n'
                            ' n153 --> n154                          \n'
                            ' n154 --> n155                          \n'
                            ' n155 --> n156                          \n'
                            ' n156 --> n157                          \n'
                            ' n157 --> n158                          \n'
                            ' n158 --> n159                          \n'
                            ' n159 --> n160                          \n'
                            ' n160 --> n161                          \n'
                            ' n161 --> n162                          \n'
                            ' n162 --> n163                          \n'
                            ' n163 --> n164                          \n'
                            ' n164 --> n165                          \n'
                            ' n165 --> n166                          \n'
                            ' n166 --> n167                          \n'
                            ' n167 --> n168                          \n'
                            ' n168 --> n169                          \n'
                            ' n169 --> n170                          \n'
                            ' n170 --> n171                          \n'
                            ' n171 --> n172                          \n'
                            ' n172 --> n173                          \n'
                            ' n173 --> n174                          \n'
                            ' n174 --> n175                          \n'
                            ' n175 --> n176                          \n'
                            ' n176 --> n177                          \n'
                            ' n177 --> n178                          \n'
                            ' n178 --> n179                          \n'
                            ' n179 --> n180                          \n'
                            ' n180 --> n181                          \n'
                            ' n181 --> n182                          \n'
                            ' n182 --> n183                          \n'
                            ' n183 --> n184                          \n'
                            ' n184 --> n185                          \n'
                            ' n185 --> n186                          \n'
                            ' n186 --> n187                          \n'
                            ' n187 --> n188                          \n'
                            ' n188 --> n189                          \n'
                            ' n189 --> n190                          \n'
                            ' n190 --> n191                          \n'
                            ' n191 --> n192                          \n'
                            ' n192 --> n193                          \n'
                            ' n193 --> n194                          \n'
                            ' n194 --> n195                          \n'
                            ' n195 --> n196                          \n'
                            ' n196 --> n197                          \n'
                            ' n197 --> n198                          \n'
                            ' n198 --> n199                          \n'
                            ' n0 --> n2                              \n'
                            ' n0 --> n3                              \n'
                            ' n0 --> n4                              \n'
                            ' n0 --> n5                              \n'
                            ' n0 --> n6                              \n'
                            ' n0 --> n7                              \n'
                            ' n0 --> n8                              \n'
                            ' n0 --> n9                              \n'
                            ' n0 --> n10                             \n'
                            ' n0 --> n11                             \n'
                            ' n0 --> n12                             \n'
                            ' n0 --> n13                             \n'
                            ' n0 --> n14                             \n'
                            ' n0 --> n15                             \n'
                            ' n0 --> n16                             \n'
                            ' n0 --> n17                             \n'
                            ' n0 --> n18                             \n'
                            ' n0 --> n19                             \n'
                            ' n0 --> n20                             \n'
                            ' n0 --> n21                             \n'
                            ' n0 --> n22                             \n'
                            ' n0 --> n23                             \n'
                            ' n0 --> n24                             \n'
                            ' n0 --> n25                             \n'
                            ' n0 --> n26                             \n'
                            ' n0 --> n27                             \n'
                            ' n0 --> n28                             \n'
                            ' n0 --> n29                             \n'
                            ' n0 --> n30                             \n'
                            ' n0 --> n31                             \n'
                            ' n0 --> n32                             \n'
                            ' n0 --> n33                             \n'
                            ' n0 --> n34                             \n'
                            ' n0 --> n35                             \n'
                            ' n0 --> n36                             \n'
                            ' n0 --> n37                             \n'
                            ' n0 --> n38                             \n'
                            ' n0 --> n39                             \n'
                            ' n0 --> n40                             \n'
                            ' n0 --> n41                             \n'
                            ' n0 --> n42                             \n'
                            ' n0 --> n43                             \n'
                            ' n0 --> n44                             \n'
                            ' n0 --> n45                             \n'
                            ' n0 --> n46                             \n'
                            ' n0 --> n47                             \n'
                            ' n0 --> n48                             \n'
                            ' n0 --> n49                             \n'
                            ' n0 --> n50                             \n'
                            ' n0 --> n51                             \n'
                            ' n0 --> n52                             \n'
                            ' n0 --> n53                             \n'
                            ' n0 --> n54                             \n'
                            ' n0 --> n55                             \n'
                            ' n0 --> n56                             \n'
                            ' n0 --> n57                             \n'
                            ' n0 --> n58                             \n'
                            ' n0 --> n59                             \n'
                            ' n0 --> n60                             \n'
                            ' n0 --> n61                             \n'
                            ' n0 --> n62                             \n'
                            ' n0 --> n63                             \n'
                            ' n0 --> n64                             \n'
                            ' n0 --> n65                             \n'
                            ' n0 --> n66                             \n'
                            ' n0 --> n67                             \n'
                            ' n0 --> n68                             \n'
                            ' n0 --> n69                             \n'
                            ' n0 --> n70                             \n'
                            ' n0 --> n71                             \n'
                            ' n0 --> n72                             \n'
                            ' n0 --> n73                             \n'
                            ' n0 --> n74                             \n'
                            ' n0 --> n75                             \n'
                            ' n0 --> n76                             \n'
                            ' n0 --> n77                             \n'
                            ' n0 --> n78                             \n'
                            ' n0 --> n79                             \n'
                            ' n0 --> n80                             \n'
                            ' n0 --> n81                             \n'
                            ' n0 --> n82                             \n'
                            ' n0 --> n83                             \n'
                            ' n0 --> n84                             \n'
                            ' n0 --> n85                             \n'
                            ' n0 --> n86                             \n'
                            ' n0 --> n87                             \n'
                            ' n0 --> n88                             \n'
                            ' n0 --> n89                             \n'
                            ' n0 --> n90                             \n'
                            ' n0 --> n91                             \n'
                            ' n0 --> n92                             \n'
                            ' n0 --> n93                             \n'
                            ' n0 --> n94                             \n'
                            ' n0 --> n95                             \n'
                            ' n0 --> n96                             \n'
                            ' n0 --> n97                             \n'
                            ' n0 --> n98                             \n'
                            ' n0 --> n99                             \n'
                            ' n0 --> n100                            \n'
                            ' n0 --> n101                            \n'
                            ' n0 --> n102                            \n'
                            ' n0 --> n103                            \n'
                            ' n0 --> n104                            \n'
                            ' n0 --> n105                            \n'
                            ' n0 --> n106                            \n'
                            ' n0 --> n107                            \n'
                            ' n0 --> n108                            \n'
                            ' n0 --> n109                            \n'
                            ' n0 --> n110                            \n'
                            ' n0 --> n111                            \n'
                            ' n0 --> n112                            \n'
                            ' n0 --> n113                            \n'
                            ' n0 --> n114                            \n'
                            ' n0 --> n115                            \n'
                            ' n0 --> n116                            \n'
                            ' n0 --> n117                            \n'
                            ' n0 --> n118                            \n'
                            ' n0 --> n119                            \n'
                            ' n0 --> n120                            \n'
                            ' n0 --> n121                            \n'
                            ' n0 --> n122                            \n'
                            ' n0 --> n123                            \n'
                            ' n0 --> n124                            \n'
                            ' n0 --> n125                            \n'
                            ' n0 --> n126                            \n'
                            ' n0 --> n127                            \n'
                            ' n0 --> n128                            \n'
                            ' n0 --> n129                            \n'
                            ' n0 --> n130                            \n'
                            ' n0 --> n131                            \n'
                            ' n0 --> n132                            \n'
                            ' n0 --> n133                            \n'
                            ' n0 --> n134                            \n'
                            ' n0 --> n135                            \n'
                            ' n0 --> n136                            \n'
                            ' n0 --> n137                            \n'
                            ' n0 --> n138                            \n'
                            ' n0 --> n139                            \n'
                            ' n0 --> n140                            \n'
                            ' n0 --> n141                            \n'
                            ' n0 --> n142                            \n'
                            ' n0 --> n143                            \n'
                            ' n0 --> n144                            \n'
                            ' n0 --> n145                            \n'
                            ' n0 --> n146                            \n'
                            ' n0 --> n147                            \n'
                            ' n0 --> n148                            \n'
                            ' n0 --> n149                            \n'
                            ' n0 --> n150                            \n'
                            ' n0 --> n151                            \n'
                            ' n0 --> n152                            \n'
                            ' n0 --> n153                            \n'
                            ' n0 --> n154                            \n'
                            ' n0 --> n155                            \n'
                            ' n0 --> n156                            \n'
                            ' n0 --> n157                            \n'
                            ' n0 --> n158                            \n'
                            ' n0 --> n159                            \n'
                            ' n0 --> n160                            \n'
                            ' n0 --> n161                            \n'
                            ' n0 --> n162                            \n'
                            ' n0 --> n163                            \n'
                            ' n0 --> n164                            \n'
                            ' n0 --> n165                            \n'
                            ' n0 --> n166                            \n'
                            ' n0 --> n167                            \n'
                            ' n0 --> n168                            \n'
                            ' n0 --> n169                            \n'
                            ' n0 --> n170                            \n'
                            ' n0 --> n171                            \n'
                            ' n0 --> n172                            \n'
                            ' n0 --> n173                            \n'
                            ' n0 --> n174                            \n'
                            ' n0 --> n175                            \n'
                            ' n0 --> n176                            \n'
                            ' n0 --> n177                            \n'
                            ' n0 --> n178                            \n'
                            ' n0 --> n179                            \n'
                            ' n0 --> n180                            \n'
                            ' n0 --> n181                            \n'
                            ' n0 --> n182                            \n'
                            ' n0 --> n183                            \n'
                            ' n0 --> n184                            \n'
                            ' n0 --> n185                            \n'
                            ' n0 --> n186                            \n'
                            ' n0 --> n187                            \n'
                            ' n0 --> n188                            \n'
                            ' n0 --> n189                            \n'
                            ' n0 --> n190                            \n'
                            ' n0 --> n191                            \n'
                            ' n0 --> n192                            \n'
                            ' n0 --> n193                            \n'
                            ' n0 --> n194                            \n'
                            ' n0 --> n195                            \n'
                            ' n0 --> n196                            \n'
                            ' n0 --> n197                            \n'
                            ' n0 --> n198                            \n'
                            ' n0 --> n199                            \n'
                            '                                        \n'}}
# fmt: on
