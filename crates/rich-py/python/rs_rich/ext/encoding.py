"""``rs_rich.ext.encoding``: strict, explicit text decoding (``rich_ext::encoding``).

No Rich counterpart: this is the port's own ``rs-rich-ext`` crate.
"""

from .._native import (
    decode_text,
    has_utf16_bom,
    EncodingError,
)

# The Rust names, where the flat native module needed a longer one.
decode = decode_text

__all__ = [
    "decode_text",
    "has_utf16_bom",
    "EncodingError",
    "decode",
]
