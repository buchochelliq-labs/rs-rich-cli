"""``rich.color``: ``Color``, ``ColorTriplet``, ``ColorSystem`` and ``ColorType``."""

from ._native import (
    Color,
    ColorParseError,
    ColorSystem,
    ColorTriplet,
    ColorType,
    blend_rgb,
    parse_rgb_hex,
)

__all__ = [
    "Color",
    "ColorParseError",
    "ColorSystem",
    "ColorTriplet",
    "ColorType",
    "blend_rgb",
    "parse_rgb_hex",
]
