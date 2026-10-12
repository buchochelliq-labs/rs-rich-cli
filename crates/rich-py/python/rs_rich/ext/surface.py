"""``rs_rich.ext.surface``: positioned drawing (``rich_ext::surface``).

No Rich counterpart: this is the port's own ``rs-rich-ext`` crate. A
``Surface`` keeps the terminal's cells between frames; ``Surface.draw(fn)``
calls ``fn`` with a ``Drawing`` that renders anything ``print`` takes into
rectangles of it, then writes only the cells that changed.
"""

from .._native import Drawing, Surface, TuiRect

Rect = TuiRect

__all__ = [
    "Drawing",
    "Rect",
    "Surface",
]
