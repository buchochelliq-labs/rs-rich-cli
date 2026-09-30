"""``rs_rich.micro``: micro assets (``rs-rich-micro``; no Rich counterpart).

Emoji-sized inline images and animations, written ``:micro:name:``:

- ``MicroRegistry`` holds assets in layers, built-in < user < trusted
  project < inline; ``MicroRegistry()`` has the built-in library
  (``status/success``, ``status/loading``, ``dev/bug``, ``fun/heart``, ...).
- ``MicroAsset`` is one asset: name, size in cells (``"2x1"`` by default),
  kind, alt text, emoji and text fallbacks, licence. It is a renderable that
  always takes exactly its cells.
- ``MicroMarkup(markup)`` is console markup with ``:micro:name:`` tokens; it
  draws images through Kitty, iTerm2, Sixel or half-blocks when the console
  is a terminal that shows them, and each asset's fallback anywhere else.
  ``micro_markup`` returns the parsed ``Text`` and what was left as written.
- ``micro_create_package`` runs an image through the pipeline (fit, contrast,
  sharpen, transparency) and writes a package; ``micro_load_package`` reads
  one. ``micro_mode`` says how this terminal would draw assets.

The Rust names are kept; ``Registry``, ``Asset``, ``Markup``, ``markup``,
``load_package``, ``create_package`` and ``mode`` are shorter aliases.
"""

from ._native import (
    MicroAsset,
    MicroError,
    MicroMarkup,
    MicroRegistry,
    micro_create_package,
    micro_load_package,
    micro_markup,
    micro_mode,
)

Registry = MicroRegistry
Asset = MicroAsset
Markup = MicroMarkup
markup = micro_markup
load_package = micro_load_package
create_package = micro_create_package
mode = micro_mode

__all__ = [
    "MicroAsset",
    "MicroError",
    "MicroMarkup",
    "MicroRegistry",
    "micro_create_package",
    "micro_load_package",
    "micro_markup",
    "micro_mode",
]
