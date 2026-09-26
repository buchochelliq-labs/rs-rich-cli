"""``rich_art::imagediff``: perceptual image diffs."""

from .._native import DiffRegion, DiffReport, DiffSettings, ImageDiffError, image_diff

#: ``rich_art::diff`` under its Rust name.
diff = image_diff

__all__ = ["DiffRegion", "DiffReport", "DiffSettings", "ImageDiffError", "image_diff"]
