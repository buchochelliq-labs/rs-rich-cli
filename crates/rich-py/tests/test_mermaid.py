"""``rs_rich.mermaid``: the ``rich-mermaid`` crate from Python.

Rich has no Mermaid support, so each case in ``CASES`` is compared with what
the Rust crate renders for the same source: ``EXPECTED`` was produced by the
same Rust oracle as ``test_art.py`` (``crates/rich-py/oracles/art-oracle``), reading
these cases as JSON.
"""

from __future__ import annotations

import hashlib
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
    """Long strings as their hash and length; everything else as is."""
    if isinstance(value, str) and len(value) > 1500:
        return {"sha256": hashlib.sha256(value.encode("utf-8")).hexdigest(), "len": len(value)}
    if isinstance(value, dict):
        return {key: digest(item) for key, item in value.items()}
    if isinstance(value, list):
        return [digest(item) for item in value]
    return value


@pytest.mark.parametrize("case", CASES, ids=[c["name"] for c in CASES])
def test_matches_the_rust_crate(case):
    if case["name"] == "render_mmdc_fallback" and mermaid.HAS_MMDC:
        pytest.skip("the oracle is built without the mmdc backend")
    assert digest(run(case)) == EXPECTED[case["name"]]


def test_every_case_has_an_expectation():
    assert sorted(EXPECTED) == sorted(c["name"] for c in CASES)


def test_a_simple_flowchart():
    console = Console(file=io.StringIO(), width=40, color_system=None)
    console.print(mermaid.Mermaid("graph LR\n  A --> B"))
    # The crate ends the diagram's last line itself, so print leaves a blank line.
    assert console.file.getvalue() == "┌───┐  ┌───┐\n│ A ├─►│ B │\n└───┘  └───┘\n\n"
    assert console.file.getvalue() == EXPECTED["render_lr"]["out"]


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
    line = console.file.getvalue().splitlines()[2]
    assert line.startswith("│ │ A ├─►│ B │") and line.endswith("│") and len(line) == 40


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
                                   '\x1b[0m\x1b[38;2;192;197;206;48;2;43;48;59msequenceDiagram\x1b[0m\x1b[48;2;43;48;59m                                           '
                                   '\x1b[0m\x1b[48;2;43;48;59m \x1b[0m\n'
                                   '\x1b[48;2;43;48;59m \x1b[0m\x1b[38;2;192;197;206;48;2;43;48;59m  '
                                   'Alice->>Bob: '
                                   'Hi\x1b[0m\x1b[48;2;43;48;59m                                         '
                                   '\x1b[0m\x1b[48;2;43;48;59m \x1b[0m\n'
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
 'render_too_wide': {'out': {'len': 16483,
                             'sha256': '65d71ef9713a15272b6f8bc6a8b6f3af0a82cab1399d42c9c6dcc7f0341acf21'}}}
# fmt: on
