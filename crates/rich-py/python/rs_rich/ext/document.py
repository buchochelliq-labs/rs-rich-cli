"""``rs_rich.ext.document``: Markdown, reStructuredText, Org, AsciiDoc and man pages (``rich_ext::document``).

No Rich counterpart: Rich renders Markdown only. ``render(source, "org")``
(or ``DocumentView``) draws a document in the look of Rich's Markdown;
with no format, ``path`` and the content decide. ``detect`` and ``sniff``
say which format a text is in and how confident that is.
"""

from .._native import (
    DocumentView,
    DocumentDiagnostic,
    DocumentDetection,
    DocumentFormatInfo,
    render_document,
    detect_document,
    sniff_document,
    document_formats,
    DOCUMENT_LIKELY,
    DOCUMENT_CERTAIN,
    DOCUMENT_STYLES,
)

# The Rust names, where the flat native module needed a longer one.
render = render_document
detect = detect_document
sniff = sniff_document
formats = document_formats
Diagnostic = DocumentDiagnostic
Detection = DocumentDetection
FormatInfo = DocumentFormatInfo
LIKELY = DOCUMENT_LIKELY
CERTAIN = DOCUMENT_CERTAIN
STYLES = DOCUMENT_STYLES

__all__ = [
    "DocumentView",
    "DocumentDiagnostic",
    "DocumentDetection",
    "DocumentFormatInfo",
    "render_document",
    "detect_document",
    "sniff_document",
    "document_formats",
    "DOCUMENT_LIKELY",
    "DOCUMENT_CERTAIN",
    "DOCUMENT_STYLES",
    "render",
    "detect",
    "sniff",
    "formats",
    "Diagnostic",
    "Detection",
    "FormatInfo",
    "LIKELY",
    "CERTAIN",
    "STYLES",
]
