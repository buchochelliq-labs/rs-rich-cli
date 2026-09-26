"""``rich_art::sixel``: images as Sixel graphics."""

from .._native import SIXEL_DEFAULT_CELL_PX, SIXEL_MAX_PIXELS, SixelArt, sixel_is_probably_supported

#: The crate's names for the same things.
DEFAULT_CELL_PX = SIXEL_DEFAULT_CELL_PX
MAX_PIXELS = SIXEL_MAX_PIXELS
is_probably_supported = sixel_is_probably_supported

__all__ = ["SIXEL_DEFAULT_CELL_PX", "SIXEL_MAX_PIXELS", "SixelArt", "sixel_is_probably_supported"]
