"""``rs_rich.ext.markdown_export``: GitHub-flavoured Markdown from renderables (``rich_ext::markdown_export``).

No Rich counterpart: this is the port's own ``rs-rich-ext`` crate. ``to_gfm``
writes a ``Table`` as a GFM table, a ``Tree`` as nested lists, a ``Panel`` as a
blockquote or ``<details>`` block, a ``Rule`` as ``---``, ``Text`` and markup
as inline Markdown, a ``Syntax`` as a fenced block and a ``Markdown`` as its
source; ``GfmDocument`` builds a document block by block, and
``fit_github_comment`` cuts it to fit a GitHub comment.
"""

from .._native import (
    GITHUB_COMMENT_LIMIT,
    GfmDocument,
    GfmExporter,
    GfmOptions,
    PanelStyle,
    fit_github_comment,
    to_gfm,
    truncate_markdown,
)

__all__ = [
    "GITHUB_COMMENT_LIMIT",
    "GfmDocument",
    "GfmExporter",
    "GfmOptions",
    "PanelStyle",
    "fit_github_comment",
    "to_gfm",
    "truncate_markdown",
]
