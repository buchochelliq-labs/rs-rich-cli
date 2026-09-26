"""``rs_rich.ext.diff``: diffs: the engine, views, git patches and test reports (``rich_ext::diff``).

No Rich counterpart: this is the port's own ``rs-rich-ext`` crate.
"""

from .._native import (
    diff_sequences,
    diff_lines,
    diff_words,
    diff_chars,
    tokenize,
    hunk_header,
    group_hunks,
    Hunk,
    TextDiff,
    DiffView,
    SourceDiff,
    FilePatch,
    Patch,
    parse_patch,
    Annotation,
    TemplateLinks,
    PatchView,
    KeepFiles,
    TestCase,
    TestSuite,
    TestRun,
    TestReport,
    PatchParseError,
    TestParseError,
    DIFF_STYLES,
)

# The Rust names, where the flat native module needed a longer one.
STYLES = DIFF_STYLES
diff_slices = diff_sequences

__all__ = [
    "diff_sequences",
    "diff_lines",
    "diff_words",
    "diff_chars",
    "tokenize",
    "hunk_header",
    "group_hunks",
    "Hunk",
    "TextDiff",
    "DiffView",
    "SourceDiff",
    "FilePatch",
    "Patch",
    "parse_patch",
    "Annotation",
    "TemplateLinks",
    "PatchView",
    "KeepFiles",
    "TestCase",
    "TestSuite",
    "TestRun",
    "TestReport",
    "PatchParseError",
    "TestParseError",
    "DIFF_STYLES",
    "STYLES",
    "diff_slices",
]
