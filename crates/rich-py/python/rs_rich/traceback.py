"""``rich.traceback``: ``Traceback``, its data classes and ``install``.

``install`` is ``_native.traceback_install`` (``rich.pretty`` has an
``install`` too, so the compiled module names them apart).
"""

from ._native import Frame, Stack, Trace, Traceback, _SyntaxError, traceback_install as install

__all__ = ["Frame", "Stack", "Trace", "Traceback"]
