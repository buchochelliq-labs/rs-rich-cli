"""``rs_rich.ext.qa``: screenshots, stress, lint, explain, profile, fuzz, matrix and benchmarks (``rich_ext::qa``; needs the ``testing`` feature).

No Rich counterpart: this is the port's own ``rs-rich-ext`` crate.
"""

from .._native import (
    qa_screenshot,
    qa_stress,
    qa_lint,
    qa_explain,
    qa_profile,
    qa_fuzz,
    qa_matrix,
    qa_bench,
)

# The Rust names, where the flat native module needed a longer one.
screenshot = qa_screenshot
stress = qa_stress
lint = qa_lint
explain = qa_explain
profile = qa_profile
fuzz = qa_fuzz
matrix = qa_matrix
bench = qa_bench

__all__ = [
    "qa_screenshot",
    "qa_stress",
    "qa_lint",
    "qa_explain",
    "qa_profile",
    "qa_fuzz",
    "qa_matrix",
    "qa_bench",
    "screenshot",
    "stress",
    "lint",
    "explain",
    "profile",
    "fuzz",
    "matrix",
    "bench",
]
