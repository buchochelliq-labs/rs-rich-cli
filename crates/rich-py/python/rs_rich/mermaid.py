"""``rs_rich.mermaid``: ``rich-mermaid`` diagrams (no Rich counterpart).

``Mermaid`` draws flowcharts, sequence diagrams and ER diagrams as text (and,
in a wheel built with the ``mmdc`` feature and ``backend="mmdc"``, every
diagram type through Mermaid's CLI); what it cannot draw shows as source under
a note. The parsers (``parse_flowchart``, ``parse_sequence``, ``parse_er``, and
``parse``, which picks one by the diagram's header) and the drawings
(``draw_flowchart``, ``draw_sequence``, ``draw_er`` and ``draw``) raise
instead. ``MermaidFences`` renders ````mermaid`` fences in Markdown;
``MermaidPlugin`` registers them with a plugin host.
"""

from . import _native
from ._native import (
    MERMAID_HAS_MMDC,
    MERMAID_MAX_EDGES,
    MERMAID_MAX_LINK_LENGTH,
    MERMAID_MAX_NODES,
    MERMAID_MAX_SOURCE,
    EntityDiagram,
    ErAttribute,
    ErEntity,
    ErRelationship,
    Flowchart,
    FlowchartEdge,
    FlowchartNode,
    Mermaid,
    MermaidDiagram,
    MermaidError,
    MermaidFences,
    MermaidLayoutError,
    MermaidParseError,
    MmdcError,
    MmdcOptions,
    SequenceDiagram,
    SequenceEvent,
    SequenceMessage,
    SequenceParticipant,
    draw_diagram,
    draw_er,
    draw_flowchart,
    draw_sequence,
    mermaid_clean_label,
    mermaid_is_er,
    mermaid_is_sequence,
    mmdc_render_png,
    parse_diagram,
    parse_er,
    parse_flowchart,
    parse_sequence,
)

#: The crate's names for the same things. ``parse`` and ``draw`` take any
#: diagram drawn as text, as ``Mermaid`` does.
parse = parse_diagram
draw = draw_diagram
is_sequence = mermaid_is_sequence
is_er = mermaid_is_er
Sequence = SequenceDiagram
clean_label = mermaid_clean_label
render_png = mmdc_render_png
HAS_MMDC = MERMAID_HAS_MMDC
MAX_SOURCE = MERMAID_MAX_SOURCE
MAX_NODES = MERMAID_MAX_NODES
MAX_EDGES = MERMAID_MAX_EDGES
MAX_LINK_LENGTH = MERMAID_MAX_LINK_LENGTH

__all__ = [
    "EntityDiagram",
    "ErAttribute",
    "ErEntity",
    "ErRelationship",
    "Flowchart",
    "FlowchartEdge",
    "FlowchartNode",
    "MERMAID_HAS_MMDC",
    "MERMAID_MAX_EDGES",
    "MERMAID_MAX_LINK_LENGTH",
    "MERMAID_MAX_NODES",
    "MERMAID_MAX_SOURCE",
    "Mermaid",
    "MermaidDiagram",
    "MermaidError",
    "MermaidFences",
    "MermaidLayoutError",
    "MermaidParseError",
    "MmdcError",
    "MmdcOptions",
    "SequenceDiagram",
    "SequenceEvent",
    "SequenceMessage",
    "SequenceParticipant",
    "draw_diagram",
    "draw_er",
    "draw_flowchart",
    "draw_sequence",
    "mermaid_clean_label",
    "mermaid_is_er",
    "mermaid_is_sequence",
    "mmdc_render_png",
    "parse_diagram",
    "parse_er",
    "parse_flowchart",
    "parse_sequence",
]

# The plugin is the plugins area's class (``rs_rich.plugins.MermaidPlugin``).
if hasattr(_native, "MermaidPlugin"):
    MermaidPlugin = _native.MermaidPlugin
    __all__.append("MermaidPlugin")
