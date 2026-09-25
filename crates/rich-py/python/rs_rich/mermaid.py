"""``rs_rich.mermaid``: ``rich-mermaid`` diagrams (no Rich counterpart).

``Mermaid`` draws flowcharts as text (and, in a wheel built with the ``mmdc``
feature and ``backend="mmdc"``, every diagram type through Mermaid's CLI);
what it cannot draw shows as source under a note. ``parse_flowchart`` and
``draw_flowchart`` raise instead. ``MermaidFences`` renders ````mermaid``
fences in Markdown; ``MermaidPlugin`` registers them with a plugin host.
"""

from . import _native
from ._native import (
    MERMAID_HAS_MMDC,
    MERMAID_MAX_EDGES,
    MERMAID_MAX_LINK_LENGTH,
    MERMAID_MAX_NODES,
    MERMAID_MAX_SOURCE,
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
    draw_flowchart,
    mermaid_clean_label,
    mmdc_render_png,
    parse_flowchart,
)

#: The crate's names for the same things.
parse = parse_flowchart
draw = draw_flowchart
clean_label = mermaid_clean_label
render_png = mmdc_render_png
HAS_MMDC = MERMAID_HAS_MMDC
MAX_SOURCE = MERMAID_MAX_SOURCE
MAX_NODES = MERMAID_MAX_NODES
MAX_EDGES = MERMAID_MAX_EDGES
MAX_LINK_LENGTH = MERMAID_MAX_LINK_LENGTH

__all__ = [
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
    "draw_flowchart",
    "mermaid_clean_label",
    "mmdc_render_png",
    "parse_flowchart",
]

# The plugin is the plugins area's class (``rs_rich.plugins.MermaidPlugin``).
if hasattr(_native, "MermaidPlugin"):
    MermaidPlugin = _native.MermaidPlugin
    __all__.append("MermaidPlugin")
