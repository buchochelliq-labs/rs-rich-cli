"""``rs_rich.art``: the ``rich-art`` crate (no Rich counterpart).

Images in every mode (ASCII, half-blocks, Braille, quadrants, Sixel), fit,
anchor, background, colour mode, dither and adjustment; FIGlet banners;
animated GIFs; and perceptual image diffs. Everything renders in Rust.
Submodules mirror the crate's modules: ``image_art``, ``ascii``, ``block``,
``braille``, ``quadrant``, ``sixel``, ``figlet``, ``gif``, ``stage`` and
``imagediff``.
"""

from .._native import (
    DEFAULT_RAMP,
    SIXEL_DEFAULT_CELL_PX,
    SIXEL_MAX_PIXELS,
    STANDARD_FONT,
    AnimatedArt,
    ArtError,
    ArtImage,
    AsciiArt,
    BlockArt,
    BrailleArt,
    DiffRegion,
    DiffReport,
    DiffSettings,
    Figlet,
    FigletFont,
    FigletFontError,
    GifFrame,
    ImageArt,
    ImageArtError,
    ImageDecodeError,
    ImageDiffError,
    ImageOptions,
    QuadrantArt,
    RenderCapabilities,
    SixelArt,
    Stage,
    figlet_render,
    image_diff,
    show_cursor_sequence,
    sixel_is_probably_supported,
)

#: ``rich_art::diff`` under its Rust name.
diff = image_diff

__all__ = [
    "AnimatedArt",
    "ArtError",
    "ArtImage",
    "AsciiArt",
    "BlockArt",
    "BrailleArt",
    "DEFAULT_RAMP",
    "DiffRegion",
    "DiffReport",
    "DiffSettings",
    "Figlet",
    "FigletFont",
    "FigletFontError",
    "GifFrame",
    "ImageArt",
    "ImageArtError",
    "ImageDecodeError",
    "ImageDiffError",
    "ImageOptions",
    "QuadrantArt",
    "RenderCapabilities",
    "SIXEL_DEFAULT_CELL_PX",
    "SIXEL_MAX_PIXELS",
    "STANDARD_FONT",
    "SixelArt",
    "Stage",
    "figlet_render",
    "image_diff",
    "show_cursor_sequence",
    "sixel_is_probably_supported",
]
