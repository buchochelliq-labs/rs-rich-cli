"""``rich_art::figlet``: FIGlet banners."""

from .._native import STANDARD_FONT, Figlet, FigletFont, FigletFontError, figlet_render

#: ``rich_art::figlet::render`` under its Rust name.
render = figlet_render

__all__ = ["Figlet", "FigletFont", "FigletFontError", "STANDARD_FONT", "figlet_render"]
