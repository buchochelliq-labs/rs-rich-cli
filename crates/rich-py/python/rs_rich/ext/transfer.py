"""``rs_rich.ext.transfer``: download and upload progress with rate, ETA and retries (``rich_ext::transfer``).

No Rich counterpart: this is the port's own ``rs-rich-ext`` crate.
"""

from .._native import (
    Transfer,
    Transfers,
    TransferReader,
    TransferWriter,
    TransferCancelled,
    TRANSFER_STYLES,
)

# The Rust names, where the flat native module needed a longer one.
STYLES = TRANSFER_STYLES

__all__ = [
    "Transfer",
    "Transfers",
    "TransferReader",
    "TransferWriter",
    "TransferCancelled",
    "TRANSFER_STYLES",
    "STYLES",
]
