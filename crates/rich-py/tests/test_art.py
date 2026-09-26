"""``rs_rich.art``: the ``rich-art`` crate from Python.

Rich has no image art, so each case in ``CASES`` is compared with what the
Rust crate itself renders for the same input: ``EXPECTED`` was produced by a
small Rust program (``crates/rich-py/oracles/art-oracle``, see its README) that
reads these cases as JSON and renders them with ``rs-rich-art``. Short
outputs are kept verbatim; long ones (Sixel included) as their SHA-256 and
length, so every byte is still compared. Images are generated from a formula
both sides share, so no binary fixture is needed; the GIFs are the crate's
own example assets.
"""

from __future__ import annotations

import hashlib
import io
import struct
from pathlib import Path

import pytest

from rs_rich import art
from rs_rich.console import Console

ASSETS = Path(__file__).resolve().parents[2] / "rich-art" / "examples" / "assets"


# ---------------------------------------------------------------------------
# Inputs shared with the Rust oracle


def pattern_bytes(pattern: str, w: int, h: int) -> bytes:
    """RGBA pixels: the oracle's ``pixel()`` formula."""
    out = bytearray()
    for y in range(h):
        for x in range(w):
            r = x * 255 // (w - 1) if w > 1 else 0
            g = y * 255 // (h - 1) if h > 1 else 0
            b = (x * y * 7 + 13) % 256
            a = 255
            if pattern == "alpha":
                a = (x + y) * 255 // (w + h - 2) if w + h > 2 else 255
            elif pattern == "spot" and w // 4 <= x < w // 2 and h // 4 <= y < h // 2:
                r, g, b = 255, 0, 0
            out += bytes((r, g, b, a))
    return bytes(out)


def make_image(spec: dict) -> art.ArtImage:
    w, h = spec["w"], spec["h"]
    return art.ArtImage.frombytes("RGBA", (w, h), pattern_bytes(spec.get("pattern", "gradient"), w, h))


def make_console(spec: dict) -> Console:
    color = spec.get("color", False)
    return Console(
        file=io.StringIO(),
        width=spec.get("width", 40),
        force_terminal=spec.get("terminal", color),
        color_system="truecolor" if color else None,
    )


def printed(console: Console, renderable) -> str:
    console.file.seek(0)
    console.file.truncate()
    console.print(renderable)
    return console.file.getvalue()


def digest(value):
    """Long strings as their hash and length; everything else as is."""
    if isinstance(value, str) and len(value) > 300:
        return {"sha256": hashlib.sha256(value.encode("utf-8")).hexdigest(), "len": len(value)}
    if isinstance(value, dict):
        return {key: digest(item) for key, item in value.items()}
    if isinstance(value, list):
        return [digest(item) for item in value]
    return value


IMG = {"w": 24, "h": 16, "pattern": "gradient"}
ALPHA = {"w": 24, "h": 16, "pattern": "alpha"}
SMALL = {"w": 12, "h": 8, "pattern": "gradient"}
COLOR = {"width": 30, "color": True}
PLAIN = {"width": 30, "color": False}
TERMINAL = {"width": 30, "color": True, "terminal": True}
FILE = {"width": 30, "color": True, "terminal": False}


def case(name, kind, opts=None, image=IMG, console=COLOR, **extra):
    return {"name": name, "kind": kind, "opts": opts or {}, "image": image, "console": console, **extra}


ANCHORS = ["center", "top", "bottom", "left", "right", "top-left", "top-right", "bottom-left", "bottom-right"]

CASES = [
    # Modes.
    case("mode_ascii_plain", "image_art", {"mode": "ascii"}, console=PLAIN),
    case("mode_ascii_color", "image_art", {"mode": "ascii", "color": True}),
    case("mode_blocks", "image_art", {"mode": "blocks"}),
    case("mode_braille", "image_art", {"mode": "braille"}),
    case("mode_quadrants", "image_art", {"mode": "quadrants"}),
    case("mode_auto_color", "image_art", {"mode": "auto", "width": 16}),
    case("mode_auto_plain", "image_art", {"mode": "auto", "width": 16}, console=PLAIN),
    case("mode_sixel", "image_art", {"mode": "sixel", "width": 6}, console=TERMINAL),
    case("mode_sixel_ansi16", "image_art", {"mode": "sixel", "width": 6, "color_mode": "ansi16", "dither": "atkinson"}, console=TERMINAL),
    case("mode_sixel_grayscale", "image_art", {"mode": "sixel", "width": 4, "color_mode": "grayscale", "background": "default"}, image=ALPHA, console=TERMINAL),
    case("mode_sixel_file", "image_art", {"mode": "sixel"}, console=FILE),
    # Fits and anchors.
    case("fit_contain", "image_art", {"mode": "blocks", "fit": "contain", "width": 20, "height": 4}),
    case("fit_stretch", "image_art", {"mode": "blocks", "fit": "stretch", "width": 20, "height": 4}),
    case("fit_native", "image_art", {"mode": "blocks", "fit": "native"}, image=SMALL),
    case("fit_native_capped", "image_art", {"mode": "quadrants", "fit": "native", "max_width": 3}, image=SMALL),
    case("fit_native_ascii", "image_art", {"mode": "ascii", "fit": "native", "color": True}, image=SMALL),
    case("fit_cover_missing_height", "image_art", {"mode": "blocks", "fit": "cover", "width": 10}),
    *[
        case(f"fit_cover_{anchor}", "image_art", {"mode": "blocks", "fit": "cover", "width": 6, "height": 6, "anchor": anchor})
        for anchor in ANCHORS
    ],
    # Backgrounds.
    case("background_color", "image_art", {"mode": "blocks", "background": "#336699"}, image=ALPHA),
    case("background_default_blocks", "image_art", {"mode": "blocks", "background": "default"}, image=ALPHA),
    case("background_default_ascii", "image_art", {"mode": "ascii", "color": True, "background": "default"}, image=ALPHA),
    case("background_default_braille", "image_art", {"mode": "braille", "background": "default"}, image=ALPHA),
    case("background_default_quadrants", "image_art", {"mode": "quadrants", "background": "default"}, image=ALPHA),
    case("background_checkerboard", "image_art", {"mode": "blocks", "background": "checkerboard"}, image=ALPHA),
    case("background_checkerboard_fit", "image_art", {"mode": "blocks", "background": "checkerboard", "fit": "contain", "width": 20, "height": 5}, image=ALPHA),
    # Colour modes, dithers and distances.
    case("color_ansi256", "image_art", {"mode": "blocks", "color_mode": "ansi256"}),
    case("color_ansi16_quadrants", "image_art", {"mode": "quadrants", "color_mode": "ansi16"}),
    case("color_grayscale_ascii", "image_art", {"mode": "ascii", "color": True, "color_mode": "grayscale"}),
    case("dither_floyd_steinberg", "image_art", {"mode": "blocks", "color_mode": "ansi16", "dither": "floyd-steinberg"}),
    case("dither_bayer4x4", "image_art", {"mode": "blocks", "color_mode": "ansi16", "dither": "bayer4x4"}),
    case("dither_atkinson", "image_art", {"mode": "blocks", "color_mode": "ansi256", "dither": "atkinson"}),
    case("distance_oklab", "image_art", {"mode": "blocks", "color_mode": "ansi16", "color_distance": "oklab"}),
    case("dither_without_palette", "image_art", {"mode": "blocks", "dither": "bayer4x4"}),
    case("braille_with_palette", "image_art", {"mode": "braille", "color_mode": "ansi256"}),
    # Adjustments.
    case("adjust_brightness", "image_art", {"mode": "blocks", "brightness": 1.5}),
    case("adjust_contrast", "image_art", {"mode": "blocks", "contrast": 0.5}),
    case("adjust_gamma", "image_art", {"mode": "blocks", "gamma": 2.2}),
    case("adjust_rotate_90", "image_art", {"mode": "blocks", "rotate": 90}),
    case("adjust_rotate_180", "image_art", {"mode": "blocks", "rotate": 180}),
    case("adjust_rotate_270", "image_art", {"mode": "quadrants", "rotate": 270}),
    case("adjust_flip_horizontal", "image_art", {"mode": "blocks", "flip_horizontal": True}),
    case("adjust_flip_vertical", "image_art", {"mode": "blocks", "flip_vertical": True}),
    case("adjust_grayscale", "image_art", {"mode": "blocks", "grayscale": True}),
    case("adjust_invalid", "image_art", {"mode": "blocks", "brightness": -1.0}),
    # Sizes.
    case("size_width_height", "image_art", {"mode": "ascii", "width": 12, "height": 3}, console=PLAIN),
    case("size_max_width", "image_art", {"mode": "blocks", "max_width": 10}),
    case("size_max_height_ascii", "image_art", {"mode": "ascii", "max_height": 3}, console=PLAIN),
    case("size_max_height_blocks", "image_art", {"mode": "blocks", "max_height": 3}),
    case("native_grid_ascii", "native_grid", {"grid_mode": "ascii", "available": 30}),
    case("native_grid_braille", "native_grid", {"grid_mode": "braille", "available": 30}),
    case("native_grid_sixel", "native_grid", {"grid_mode": "sixel", "available": 2}),
    # The single-backend renderables.
    case("ascii_ramp_invert", "ascii", {"width": 20, "ramp": " .:-=+*#%@", "invert": True}, console=PLAIN),
    case("ascii_color_raw", "ascii", {"width": 20, "color": True, "normalize": False}),
    case("ascii_text", "ascii_text", {"height": 4, "text_width": 16}),
    case("block_height", "block", {"height": 3}),
    case("quadrant", "quadrant", {"width": 10}),
    case("braille", "braille", {"width": 10}),
    case("braille_text", "braille_text", {"text_width": 8}),
    case("sixel_print", "sixel", {"width": 4}, console=TERMINAL),
    case("sixel_encode", "sixel_encode", {"cell_px": [4, 8], "max_colors": 16, "available": 5}),
    # FIGlet.
    case("figlet_plain", "figlet", {"text": "Hi rs"}, console={"width": 40}),
    case("figlet_center", "figlet", {"text": "Hi", "justify": "center"}, console={"width": 30}),
    case("figlet_right_width", "figlet", {"text": "Hi", "justify": "right", "width": 20}, console={"width": 40}),
    case("figlet_style", "figlet", {"text": "Ok", "style": "bold red"}, console={"width": 30, "color": True}),
    case("figlet_wraps", "figlet", {"text": "wrapping words"}, console={"width": 30}),
    case("figlet_text", "figlet_text", {"text": "rs", "text_width": 80}),
    # GIFs.
    case("gif_ball_ascii", "gif", {"asset": "ball.gif", "width": 20}, console=PLAIN),
    case("gif_ball_blocks", "gif", {"asset": "ball.gif", "width": 20, "blocks": True, "color": True}, console=TERMINAL),
    case("gif_cat_ansi16", "gif", {"asset": "cat.gif", "width": 16, "blocks": True, "color": True, "color_mode": "ansi16", "dither": "bayer4x4"}, console=TERMINAL),
    case("gif_capped", "gif", {"asset": "ball.gif", "width": 10, "max_fps": 2.0, "ramp": " .oO@", "invert": True}, console=PLAIN),
    case(
        "stage_two",
        "stage",
        {"arts": [{"asset": "ball.gif", "width": 12}, {"asset": "cat.gif", "width": 14}], "gap": 3},
        console=PLAIN,
    ),
    # Diffs.
    case("diff_spot", "diff", {"blur": 2.0, "threshold": 30.0, "min_region": 20, "open_kernel": 3}, image={"w": 48, "h": 40, "pattern": "gradient"}, image2={"w": 48, "h": 40, "pattern": "spot"}),
    case("diff_defaults", "diff", {}, image={"w": 48, "h": 40, "pattern": "gradient"}, image2={"w": 48, "h": 40, "pattern": "spot"}),
    case("diff_identical", "diff", {}, image={"w": 16, "h": 16, "pattern": "gradient"}, image2={"w": 16, "h": 16, "pattern": "gradient"}),
    case("diff_sizes", "diff", {}, image={"w": 16, "h": 16, "pattern": "gradient"}, image2={"w": 16, "h": 12, "pattern": "gradient"}),
]

IMAGE_KWARGS = [
    "mode", "width", "height", "fit", "anchor", "background", "color", "color_mode", "dither",
    "color_distance", "rotate", "flip_horizontal", "flip_vertical", "grayscale", "brightness",
    "contrast", "gamma", "max_width", "max_height",
]  # fmt: skip


def pick(opts: dict, names) -> dict:
    return {name: opts[name] for name in names if name in opts}


GIF_KWARGS = ["width", "height", "ramp", "invert", "color", "blocks", "color_mode", "dither", "color_distance", "max_fps"]


def gif(opts: dict) -> art.AnimatedArt:
    return art.AnimatedArt(ASSETS / opts["asset"], **pick(opts, GIF_KWARGS))


def run(case: dict) -> dict:
    """Render a case with rs_rich, in the oracle's result shape."""
    kind, opts = case["kind"], case["opts"]
    console = make_console(case["console"])
    if kind == "image_art":
        try:
            image_art = art.ImageArt(make_image(case["image"]), **pick(opts, IMAGE_KWARGS))
            return {"out": printed(console, image_art)}
        except art.ImageArtError as error:
            return {"error": str(error)}
    if kind == "native_grid":
        image_art = art.ImageArt(make_image(case["image"]), **pick(opts, IMAGE_KWARGS))
        return {"out": list(image_art.native_grid(opts["grid_mode"], opts["available"]))}
    image = make_image(case["image"])
    if kind in ("ascii", "ascii_text"):
        ascii_art = art.AsciiArt(image, **pick(opts, ["width", "height", "ramp", "invert", "color", "normalize"]))
        if kind == "ascii":
            return {"out": printed(console, ascii_art)}
        width = opts["text_width"]
        return {"out": ascii_art.to_text(width), "columns": ascii_art.columns(width)}
    if kind == "block":
        return {"out": printed(console, art.BlockArt(image, **pick(opts, ["width", "height"])))}
    if kind == "quadrant":
        return {"out": printed(console, art.QuadrantArt(image, **pick(opts, ["width", "height"])))}
    if kind in ("braille", "braille_text"):
        braille = art.BrailleArt(image, **pick(opts, ["width", "height"]))
        if kind == "braille":
            return {"out": printed(console, braille)}
        return {"out": braille.to_text(opts["text_width"])}
    if kind in ("sixel", "sixel_encode"):
        options = pick(opts, ["width", "height", "max_colors"])
        if "cell_px" in opts:
            options["cell_px"] = tuple(opts["cell_px"])
        sixel = art.SixelArt(image, **options)
        if kind == "sixel":
            return {"out": printed(console, sixel)}
        return {"out": sixel.encode(opts["available"])}
    if kind in ("figlet", "figlet_text"):
        banner = art.Figlet(opts["text"], **pick(opts, ["justify", "style", "width"]))
        if kind == "figlet":
            return {"out": printed(console, banner)}
        return {"out": banner.to_text(opts["text_width"])}
    if kind == "gif":
        animation = gif(opts)
        count = animation.frame_count
        played = Console(file=io.StringIO(), width=console.width, force_terminal=False, color_system=None)
        animation.play(played)
        return {
            "frame_count": count,
            "duration": animation.duration,
            "delays": [animation.frame_delay(i) for i in range(count)],
            "frames": [printed(console, animation.render_frame(i)) for i in range(min(count, 3))],
            "ascii0": printed(console, animation.frame(0)),
            "print": printed(console, animation),
            "played": played.file.getvalue(),
        }
    if kind == "stage":
        stage = art.Stage(*[gif(item) for item in opts["arts"]], gap=opts.get("gap", 2))
        stage.play(console)
        return {"out": console.file.getvalue()}
    if kind == "diff":
        after = make_image(case["image2"])
        try:
            report = art.image_diff(image, after, **pick(opts, ["blur", "threshold", "open_kernel", "min_region", "top"]))
        except art.ImageDiffError as error:
            return {"error": str(error)}
        delta = report.delta_e
        return {
            "width": report.width,
            "height": report.height,
            "changed_fraction": report.changed_fraction,
            "naive_changed_fraction": report.naive_changed_fraction,
            "mean_delta_e": report.mean_delta_e,
            "max_delta_e": report.max_delta_e,
            "regions": [
                [r.x, r.y, r.width, r.height, r.area_px, r.share_of_change, r.mean_delta_e] for r in report.regions
            ],
            "delta_e_hex": struct.pack(f"<{len(delta)}f", *delta).hex(),
            "heatmap_hex": report.heatmap().tobytes().hex(),
            "highlight_hex": report.highlight(after).tobytes().hex(),
        }
    raise AssertionError(f"unknown kind {kind}")


@pytest.mark.parametrize("case", CASES, ids=[c["name"] for c in CASES])
def test_matches_the_rust_crate(case):
    assert digest(run(case)) == EXPECTED[case["name"]]


def test_every_case_has_an_expectation():
    assert sorted(EXPECTED) == sorted(c["name"] for c in CASES)


# ---------------------------------------------------------------------------
# Inputs


def png_bytes(w: int, h: int, pattern: str = "gradient") -> bytes:
    """A PNG written by hand (zlib, no Pillow), RGBA 8-bit."""
    import zlib

    raw = pattern_bytes(pattern, w, h)
    rows = b"".join(b"\x00" + raw[y * w * 4 : (y + 1) * w * 4] for y in range(h))

    def chunk(tag: bytes, data: bytes) -> bytes:
        return struct.pack(">I", len(data)) + tag + data + struct.pack(">I", zlib.crc32(tag + data) & 0xFFFFFFFF)

    header = struct.pack(">IIBBBBB", w, h, 8, 6, 0, 0, 0)
    return b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", header) + chunk(b"IDAT", zlib.compress(rows)) + chunk(b"IEND", b"")


class FakePillowImage:
    """What ImageArt reads from a Pillow image: mode, size, convert, tobytes."""

    def __init__(self, w: int, h: int):
        self.mode, self.size, self._data = "RGBA", (w, h), pattern_bytes("gradient", w, h)

    def convert(self, mode):
        assert mode == "RGBA"
        return self

    def tobytes(self):
        return self._data


def render_blocks(source) -> str:
    return printed(make_console(COLOR), art.ImageArt(source, mode="blocks"))


def test_every_input_kind_renders_the_same_pixels(tmp_path):
    expected = render_blocks(make_image(IMG))
    png = png_bytes(24, 16)
    path = tmp_path / "image.png"
    path.write_bytes(png)
    for source in [png, bytearray(png), memoryview(png), path, str(path), FakePillowImage(24, 16), art.ArtImage(png)]:
        assert render_blocks(source) == expected, type(source)


def test_a_real_pillow_image_when_pillow_is_installed():
    image_module = pytest.importorskip("PIL.Image")
    pil = image_module.frombytes("RGBA", (24, 16), pattern_bytes("gradient", 24, 16))
    assert render_blocks(pil) == render_blocks(make_image(IMG))
    assert render_blocks(pil.convert("RGB")) == render_blocks(make_image(IMG))
    assert art.ArtImage(pil).to_pil().tobytes() == pil.tobytes()


def test_art_image():
    image = art.ArtImage.frombytes("RGB", (2, 1), b"\x01\x02\x03\x04\x05\x06")
    assert (image.width, image.height, image.size, image.mode) == (2, 1, (2, 1), "RGB")
    assert image.tobytes() == b"\x01\x02\x03\x04\x05\x06"
    assert image.to_rgba() == b"\x01\x02\x03\xff\x04\x05\x06\xff"
    assert repr(image) == "<ArtImage 2x1 RGB>"
    decoded = art.ArtImage.from_bytes(image.to_png())
    assert decoded.to_rgba() == image.to_rgba()
    assert art.ArtImage.frombytes("L", (2, 1), b"\x00\xff").mode == "L"
    with pytest.raises(ValueError, match="do not make a 3x1 RGB image"):
        art.ArtImage.frombytes("RGB", (3, 1), b"\x00")
    with pytest.raises(ValueError, match="invalid mode"):
        art.ArtImage.frombytes("CMYK", (1, 1), b"\x00\x00\x00\x00")


def test_art_image_save_and_open(tmp_path):
    image = make_image(SMALL)
    path = tmp_path / "out.png"
    image.save(path)
    assert art.ArtImage.open(path).tobytes() == image.tobytes()
    assert art.ArtImage.open(str(path)).size == (12, 8)


def test_decode_and_file_errors(tmp_path):
    with pytest.raises(art.ImageDecodeError, match="could not decode the image"):
        art.ImageArt(b"not an image")
    with pytest.raises(FileNotFoundError):
        art.ImageArt(tmp_path / "missing.png")
    with pytest.raises(TypeError, match="expected an image"):
        art.ImageArt(42)
    with pytest.raises(art.ImageDecodeError):
        art.AnimatedArt(b"GIF89a broken")
    assert issubclass(art.ImageDecodeError, art.ArtError)


# ---------------------------------------------------------------------------
# ImageArt's arguments and errors


def test_image_art_attributes():
    image_art = art.ImageArt(
        make_image(IMG), mode="half-block", fit="cover", anchor="top_left", background=(1, 2, 3),
        color_mode="greyscale", dither="floyd_steinberg", color_distance="oklab", rotate=-90,
        width=5, height=4, max_width=9, max_height=8,
    )  # fmt: skip
    assert image_art.mode == "blocks"
    assert (image_art.fit, image_art.anchor, image_art.background) == ("cover", "top-left", (1, 2, 3))
    assert (image_art.color_mode, image_art.dither, image_art.color_distance) == ("grayscale", "floyd-steinberg", "oklab")
    assert (image_art.rotate, image_art.width, image_art.height) == (270, 5, 4)
    assert (image_art.max_width, image_art.max_height) == (9, 8)
    assert image_art.image.size == (24, 16)
    assert art.ImageArt(make_image(IMG), background="#102030").background == (16, 32, 48)
    assert art.ImageArt(make_image(IMG), background="default").background == "default"
    assert art.ImageArt(make_image(IMG), background="checkerboard").background == "checkerboard"


def test_image_options_replace_mode_and_size():
    options = art.ImageOptions(mode="braille", width=10, color=True)
    image_art = art.ImageArt(make_image(IMG), mode="ascii", width=3, options=options)
    assert (image_art.mode, image_art.width, image_art.height, image_art.color) == ("braille", 10, None, True)
    assert repr(options) == "ImageOptions(mode='braille', width=10, height=None, color=True)"
    assert printed(make_console(COLOR), image_art) == printed(make_console(COLOR), art.ImageArt(make_image(IMG), mode="braille", width=10))


def test_resolve_mode():
    image_art = art.ImageArt(make_image(IMG))
    assert image_art.resolve_mode(art.RenderCapabilities()) == "ascii"
    assert image_art.resolve_mode(art.RenderCapabilities(color=True)) == "blocks"
    assert image_art.resolve_mode(art.RenderCapabilities(color=True, sixel_supported=True)) == "sixel"
    assert art.ImageArt(make_image(IMG), mode="braille").resolve_mode(art.RenderCapabilities(color=True)) == "braille"


def test_render_capabilities_from_console(monkeypatch):
    monkeypatch.setenv("RICH_GRAPHICS", "sixel")
    caps = art.RenderCapabilities.from_console(make_console(TERMINAL))
    assert (caps.color, caps.sixel_supported) == (True, True)
    assert art.sixel_is_probably_supported() is True
    monkeypatch.setenv("RICH_GRAPHICS", "none")
    caps = art.RenderCapabilities.from_console(make_console(PLAIN))
    assert (caps.color, caps.sixel_supported) == (False, False)
    assert repr(caps) == "RenderCapabilities(color=False, sixel_supported=False)"


@pytest.mark.parametrize(
    "kwargs, message",
    [
        ({"mode": "pixels"}, "invalid image mode 'pixels'"),
        ({"fit": "fill"}, "invalid fit 'fill'"),
        ({"anchor": "middle"}, "invalid anchor 'middle'"),
        ({"color_mode": "ansi8"}, "invalid color mode 'ansi8'"),
        ({"dither": "random"}, "invalid dither 'random'"),
        ({"color_distance": "lab"}, "invalid color distance 'lab'"),
        ({"rotate": 45}, "invalid rotation 45"),
        ({"background": "nope"}, "invalid background 'nope'"),
    ],
)
def test_bad_names_raise_value_error(kwargs, message):
    with pytest.raises(ValueError, match=message):
        art.ImageArt(make_image(IMG), **kwargs)


def test_render_errors_have_a_kind():
    with pytest.raises(art.ImageArtError) as raised:
        art.ImageArt(make_image(IMG), gamma=0.0)
    assert raised.value.kind == "invalid_adjustment"
    with pytest.raises(art.ImageArtError) as raised:
        make_console(FILE).print(art.ImageArt(make_image(IMG), mode="sixel"))
    assert raised.value.kind == "non_terminal_destination"


def test_sixel_raises_inside_a_container_too():
    from rs_rich.panel import Panel

    with pytest.raises(art.ImageArtError, match="terminal destination"):
        make_console(FILE).print(Panel(art.ImageArt(make_image(IMG), mode="sixel")))


def test_image_art_in_a_panel_matches_its_own_render():
    from rs_rich.panel import Panel

    console = make_console(COLOR)
    inner = art.ImageArt(make_image(SMALL), mode="blocks", width=6)
    boxed = printed(console, Panel(inner, width=10)).splitlines()
    alone = printed(console, inner).splitlines()
    assert len(boxed) == len(alone) + 2
    for inside, line in zip(boxed[1:-1], alone):
        assert line in inside


# ---------------------------------------------------------------------------
# FIGlet, GIFs and diffs


def test_figlet_fonts(tmp_path):
    font = art.FigletFont()
    assert (font.height, font.hard_blank) == (6, "$")
    assert art.FigletFont.standard().height == 6
    parsed = art.FigletFont.parse(art.STANDARD_FONT)
    assert art.Figlet("A", font=parsed).to_text() == art.Figlet("A").to_text()
    path = tmp_path / "standard.flf"
    path.write_text(art.STANDARD_FONT, encoding="utf-8")
    assert art.FigletFont.from_path(path).height == 6
    with pytest.raises(art.FigletFontError, match="flf2a"):
        art.FigletFont("not a font")
    assert art.figlet_render("rs", width=80) == art.Figlet("rs").to_text(80)
    assert art.figlet_render("rs", parsed, 80, "right") == art.Figlet("rs", justify="right").to_text(80)
    with pytest.raises(ValueError, match="invalid justify"):
        art.Figlet("x", justify="full")
    banner = art.Figlet("Hi", width=20, justify="center")
    assert (banner.text, banner.width, banner.justify) == ("Hi", 20, "center")


def test_gif_frames_for_a_live_display():
    animation = art.AnimatedArt((ASSETS / "ball.gif").read_bytes(), width=10, repeat="forever")
    frames = animation.frames()
    assert len(frames) == len(animation) == animation.frame_count
    assert [delay for _, delay in frames] == [animation.frame_delay(i) for i in range(len(animation))]
    assert frames[1][0].index == 1
    assert animation.frame_delay(len(animation)) is None and animation.render_frame(len(animation)) is None
    assert animation.repeat == "forever"
    assert art.AnimatedArt(ASSETS / "ball.gif", repeat=3).repeat == 3
    assert art.AnimatedArt.MAX_DECODED_BYTES == 512 * 1024 * 1024


def test_gif_plays_in_place_on_a_terminal():
    animation = art.AnimatedArt(ASSETS / "ball.gif", width=8, max_fps=1000.0)
    console = make_console({"width": 20, "color": False, "terminal": True})
    animation.play(console)
    out = console.file.getvalue()
    assert out.startswith("\x1b[?25l") and out.endswith("\x1b[?25h")
    assert art.show_cursor_sequence() == "\x1b[?25h"


def test_play_stops_when_the_file_fails():
    class Broken(io.StringIO):
        def write(self, text):
            raise OSError("disk full")

    console = Console(file=Broken(), width=20, force_terminal=False, color_system=None)
    with pytest.raises(OSError, match="disk full"):
        art.AnimatedArt(ASSETS / "ball.gif", width=8).play(console)


def test_stage():
    stage = art.Stage(gap=1)
    assert len(stage) == 0
    stage.add(art.AnimatedArt(ASSETS / "ball.gif", width=6)).add(art.AnimatedArt(ASSETS / "cat.gif", width=6))
    assert len(stage) == 2 and stage.gap == 1 and stage.until is None
    stage.until = 0.5
    assert stage.until == 0.5
    with pytest.raises(ValueError, match="until"):
        art.Stage(until=-1.0)


def test_diff_settings_and_report():
    settings = art.DiffSettings(threshold=30.0, min_region=20)
    assert (settings.blur, settings.threshold, settings.open_kernel, settings.min_region, settings.top) == (6.0, 30.0, 11, 20, 3)
    before, after = make_image({"w": 48, "h": 40}), make_image({"w": 48, "h": 40, "pattern": "spot"})
    by_settings = art.image_diff(before, after, art.DiffSettings(blur=2.0, threshold=30.0, min_region=20, open_kernel=3))
    by_keywords = art.image_diff(before, after, blur=2.0, threshold=30.0, min_region=20, open_kernel=3)
    assert by_settings.regions[0].__repr__() == by_keywords.regions[0].__repr__()
    assert by_keywords.heatmap().size == (48, 40)
    assert art.diff is art.image_diff
    with pytest.raises(art.ImageDiffError) as raised:
        art.image_diff(before, make_image(SMALL))
    assert raised.value.kind == "size_mismatch"


def test_the_submodules_mirror_the_crate():
    from rs_rich.art import ascii, block, braille, figlet, gif, image_art, imagediff, quadrant, sixel, stage

    assert image_art.ImageArt is art.ImageArt and ascii.AsciiArt is art.AsciiArt
    assert block.BlockArt is art.BlockArt and braille.BrailleArt is art.BrailleArt
    assert quadrant.QuadrantArt is art.QuadrantArt and stage.Stage is art.Stage
    assert figlet.render is art.figlet_render and gif.GifFrame is art.GifFrame
    assert sixel.MAX_PIXELS == art.SIXEL_MAX_PIXELS == 16 * 1024 * 1024
    assert sixel.DEFAULT_CELL_PX == (8, 16) and imagediff.diff is art.image_diff


# Generated by the Rust oracle; do not edit by hand.
# fmt: off
EXPECTED: dict = {'adjust_brightness': {'out': {'len': 11812,
                               'sha256': 'e11de73e30ae9aa7232f45f3bceb79c3878b19d9a377f2be0324ba897071e0f4'}},
 'adjust_contrast': {'out': {'len': 11798,
                             'sha256': '8b8793872a9518db8f8d7d2fa02a06cfc512cace2a93ab723c23c674ceeb0e03'}},
 'adjust_flip_horizontal': {'out': {'len': 11513,
                                    'sha256': '3ca5500a0f43c225db399ac056a466bfb542fa4db7c5cddb431d71cdda3f4bce'}},
 'adjust_flip_vertical': {'out': {'len': 11513,
                                  'sha256': 'eeea514283a36974b3d7bae6863ed29e474f69b4d919e46ca6623f23845adb39'}},
 'adjust_gamma': {'out': {'len': 12010,
                          'sha256': '29ccaa08a97f5ef38eaf2d99489b376e2a3cc26ace11d4789d61502eae8ca923'}},
 'adjust_grayscale': {'out': {'len': 11689,
                              'sha256': '47f4f503e5036aed0cfb3493c81a310ed97d7e962bdb2dfc2911bd82e393ad2d'}},
 'adjust_invalid': {'error': 'image brightness and contrast must be finite and non-negative, and gamma '
                             'finite and positive'},
 'adjust_rotate_180': {'out': {'len': 11513,
                               'sha256': '852f26b5bb6e9ab4668ff85c30be1fda5b30a7f6940231675d5b1f099627fcb9'}},
 'adjust_rotate_270': {'out': {'len': 26483,
                               'sha256': '3b7fcfdbd1dc07fa05e4e62bf42c6a8de97baa9ebc2ed8e2dc4c5203a99d8ea5'}},
 'adjust_rotate_90': {'out': {'len': 26479,
                              'sha256': 'e23fa53290349058abdf2cacbe776f29d71c3371f2df1b668032764b57d1c4cf'}},
 'ascii_color_raw': {'out': {'len': 3201,
                             'sha256': '7546d0c7bf5da8862e88b14aea0150ba2811107e27a43cf4f9ba38c71bf91174'}},
 'ascii_ramp_invert': {'out': '@@@%%%%%######******\n'
                              '%%%####******++++++=\n'
                              '##******++++++=+====\n'
                              '**++++++=======-----\n'
                              '++========-----:::::\n'
                              '====-----::::::.....\n'
                              '=---::::::......    \n'},
 'ascii_text': {'columns': 16,
                'out': "    .....',,,;::\n..',,;;::clooodd\n:cllooddxkkO000K\ndkxOOO0KKKXXWWWM"},
 'background_checkerboard': {'out': {'len': 11976,
                                     'sha256': '9b0998d808b32eadb3529fb4b52d3b04121babfb76324114acd2bf70579d53ec'}},
 'background_checkerboard_fit': {'out': {'len': 4017,
                                         'sha256': 'ea17006e4ede835f14aa8f912a87d3c363c4d1f6f1b51ede5de58b143381cfeb'}},
 'background_color': {'out': {'len': 11704,
                              'sha256': 'e1df3305fb05562882df1cb13c32c1f2d80e74abb26b686e56d9d23017913117'}},
 'background_default_ascii': {'out': {'len': 3675,
                                      'sha256': '91f7d1eef0574ad79acb9cdeb9a63b7800087e128d7e6c9fb37f23e32a709ac6'}},
 'background_default_blocks': {'out': {'len': 5982,
                                       'sha256': 'bd373059528d48782e147cccf36b7668371507edc69fca29d0a0bedca4c94e3c'}},
 'background_default_braille': {'out': {'len': 310,
                                        'sha256': '7ab77ef3767a9dfe659ca42551044f41780a401d5e8214bebe8333ca356c2db1'}},
 'background_default_quadrants': {'out': {'len': 6192,
                                          'sha256': 'de8a70811082b9aba9d9048d14bd53be01a47ceeb382efbb88ec03cbabe36324'}},
 'block_height': {'out': {'len': 1057,
                          'sha256': '20293c94cd5f1930312f5faa50b4b42a2779542e41d3b6f560caf6deaeb8ff5a'}},
 'braille': {'out': '⠀⠀⠀⠀⠀⠀⠀⠀⠀⢀\n⠀⠀⠀⠀⠀⢀⣠⣄⣶⣾\n⠀⣄⣠⣦⣾⣿⣿⣿⣿⣿\n⣾⣿⣿⣿⣿⣿⣿⣿⣿⣿\n'},
 'braille_text': {'out': '⠀⠀⠀⠀⠀⠀⠀⢀\n⠀⠀⣀⣠⣴⣶⣾⣿\n⣼⣿⣿⣿⣿⣿⣿⣿'},
 'braille_with_palette': {'error': 'Braille images are monochrome, so they take no color mode; dithering and '
                                   'color distance require ansi256, ansi16 or grayscale'},
 'color_ansi16_quadrants': {'out': {'len': 4063,
                                    'sha256': '916fd22451603982ae92f1263cf92c314dabddf05d07b63fcbdc32a60485f877'}},
 'color_ansi256': {'out': {'len': 7251,
                           'sha256': 'bc532b7fc250bd2d3302dce350a511682e3173179b96031f19bdbbfa8eb65a23'}},
 'color_grayscale_ascii': {'out': {'len': 2335,
                                   'sha256': '2a2ac32579247cd67ac5cc094229f5a0c21fff3c5b219c4e167bf02764be8b87'}},
 'diff_defaults': {'changed_fraction': 0.0,
                   'delta_e_hex': {'len': 15360,
                                   'sha256': '0664fa1f09419e7e5ad53dde5397250e1b823640c917a2deae69c139b9a10810'},
                   'heatmap_hex': {'len': 11520,
                                   'sha256': '9d2740629703afb1e12525e4cfee490b502b84d73fb8c09c802a4282d5ed046b'},
                   'height': 40,
                   'highlight_hex': {'len': 11520,
                                     'sha256': 'f62d8401c27dbd7c79b1e9a046f886c4a4a44383d4a8f0c8f530fe8a48290709'},
                   'max_delta_e': 47.931095123291016,
                   'mean_delta_e': 6.5645880699157715,
                   'naive_changed_fraction': 0.0625,
                   'regions': [],
                   'width': 48},
 'diff_identical': {'changed_fraction': 0.0,
                    'delta_e_hex': {'len': 2048,
                                    'sha256': 'e2943f8d0b0e7d5835f9533722a6e25f669acb8980daee378b4edb44da212f51'},
                    'heatmap_hex': {'len': 1536,
                                    'sha256': 'e22f88769a8d88ba1e0258671e08c8d9e32e408d37cafc2336160951d7de32a0'},
                    'height': 16,
                    'highlight_hex': {'len': 1536,
                                      'sha256': 'ffd61e63769daa76f1baf312a256659bbfa9241bb00f125776ca57a75d37365c'},
                    'max_delta_e': 0.0,
                    'mean_delta_e': 0.0,
                    'naive_changed_fraction': 0.0,
                    'regions': [],
                    'width': 16},
 'diff_sizes': {'error': 'images differ in size (16x16 vs 16x12); align them first — a diff of '
                         'differently-sized images is meaningless'},
 'diff_spot': {'changed_fraction': 0.08124999701976776,
               'delta_e_hex': {'len': 15360,
                               'sha256': '735a89a5060374fd8d373610a55abf5a71a3b7ea488bd1cc1f2785378ecfd2fc'},
               'heatmap_hex': {'len': 11520,
                               'sha256': 'c4b3423d827889f751a4ccd5774acfa83b27a50b287b1d7b20ab2ea9877c0ae1'},
               'height': 40,
               'highlight_hex': {'len': 11520,
                                 'sha256': '6eb61151f6578c694fa04fdaf8b2a1f6972225281ca058ec8e4db5faf68e002c'},
               'max_delta_e': 113.0997314453125,
               'mean_delta_e': 7.2309770584106445,
               'naive_changed_fraction': 0.0625,
               'regions': [[11, 9, 14, 12, 156, 1.0, 76.95532989501953]],
               'width': 48},
 'distance_oklab': {'out': {'len': 4061,
                            'sha256': '8cd47f6bf59b1c41af33a6311385d889ac3f3480442dbd09d72837e62fa1c915'}},
 'dither_atkinson': {'out': {'len': 7247,
                             'sha256': 'c02885c3f58a9487ea41577c338a6ea7d559c4a6e24931002c91050687082247'}},
 'dither_bayer4x4': {'out': {'len': 4061,
                             'sha256': '1d20a3850f0bb5c2ead13858709cdc95adea00ecfe6c3dd697f9d53bb5162b86'}},
 'dither_floyd_steinberg': {'out': {'len': 4064,
                                    'sha256': 'fb0945961d69a3cd71fef96c8818ccb0e1bb69a9557a7f8e733e477d88ddd305'}},
 'dither_without_palette': {'error': 'Braille images are monochrome, so they take no color mode; dithering '
                                     'and color distance require ansi256, ansi16 or grayscale'},
 'figlet_center': {'out': '           _   _ _ \n'
                          '          | | | (_)\n'
                          '          | |_| | |\n'
                          '          |  _  | |\n'
                          '          |_| |_|_|\n'
                          '                   \n'},
 'figlet_plain': {'out': ' _   _ _            \n'
                         '| | | (_)  _ __ ___ \n'
                         "| |_| | | | '__/ __|\n"
                         '|  _  | | | |  \\__ \\\n'
                         '|_| |_|_| |_|  |___/\n'
                         '                    \n'},
 'figlet_right_width': {'out': '           _   _ _ \n'
                               '          | | | (_)\n'
                               '          | |_| | |\n'
                               '          |  _  | |\n'
                               '          |_| |_|_|\n'
                               '                   \n'},
 'figlet_style': {'out': '\x1b[1;31m  ___  _    \x1b[0m\n'
                         '\x1b[1;31m / _ \\| | __\x1b[0m\n'
                         '\x1b[1;31m| | | | |/ /\x1b[0m\n'
                         '\x1b[1;31m| |_| |   < \x1b[0m\n'
                         '\x1b[1;31m \\___/|_|\\_\\\x1b[0m\n'
                         '\x1b[1;31m            \x1b[0m\n'},
 'figlet_text': {'out': "          \n _ __ ___ \n| '__/ __|\n| |  \\__ \\\n|_|  |___/\n          \n"},
 'figlet_wraps': {'out': {'len': 474,
                          'sha256': '92e96f66eceb97439e25e8652da30fdb9db0a34b2bff0367dff547f20e439532'}},
 'fit_contain': {'out': {'len': 2792,
                         'sha256': '8de365544184dd2afc7f4f2f6b681c5b6473e10968b188a085b83170cb107ab2'}},
 'fit_cover_bottom': {'out': {'len': 1415,
                              'sha256': 'c9055695eefca60e698cb57ae658523d89a9bd777a095aa02c07ff44f983cc55'}},
 'fit_cover_bottom-left': {'out': {'len': 1323,
                                   'sha256': 'e75906028250dc6ddea5340c9ec9eb5d79da7b82e069e5bbbe0d75331a0e9d1c'}},
 'fit_cover_bottom-right': {'out': {'len': 1431,
                                    'sha256': 'f587772659f80b31cebb19dd5c44a0575b7b826c6e14b39a0134f99ab3fbc658'}},
 'fit_cover_center': {'out': {'len': 1415,
                              'sha256': 'c9055695eefca60e698cb57ae658523d89a9bd777a095aa02c07ff44f983cc55'}},
 'fit_cover_left': {'out': {'len': 1323,
                            'sha256': 'e75906028250dc6ddea5340c9ec9eb5d79da7b82e069e5bbbe0d75331a0e9d1c'}},
 'fit_cover_missing_height': {'error': 'image fit requires positive width and height, a nonempty image and '
                                       'destination, and at most 16 megapixels'},
 'fit_cover_right': {'out': {'len': 1431,
                             'sha256': 'f587772659f80b31cebb19dd5c44a0575b7b826c6e14b39a0134f99ab3fbc658'}},
 'fit_cover_top': {'out': {'len': 1415,
                           'sha256': 'c9055695eefca60e698cb57ae658523d89a9bd777a095aa02c07ff44f983cc55'}},
 'fit_cover_top-left': {'out': {'len': 1323,
                                'sha256': 'e75906028250dc6ddea5340c9ec9eb5d79da7b82e069e5bbbe0d75331a0e9d1c'}},
 'fit_cover_top-right': {'out': {'len': 1431,
                                 'sha256': 'f587772659f80b31cebb19dd5c44a0575b7b826c6e14b39a0134f99ab3fbc658'}},
 'fit_native': {'out': {'len': 1812,
                        'sha256': '5daf77589e063526be223841e9796a25db64a0844542b81bd17cc8e24f33be84'}},
 'fit_native_ascii': {'out': {'len': 1083,
                              'sha256': '5e14c7baf070e4a2ec8af3ca0991a5f582d73bcd57a6ea30c7fb056ef2cec9d2'}},
 'fit_native_capped': {'out': '\x1b[38;2;37;74;37;48;2;37;181;69m▀\x1b[0m\x1b[38;2;127;74;85;48;2;127;181;125m▀\x1b[0m\x1b[38;2;218;74;108;48;2;218;181;132m▀\x1b[0m\n'},
 'fit_stretch': {'out': {'len': 3102,
                         'sha256': '9bceeb789575114c180ecd91603809d88739b1817cd3728a5a17f4fee362c13d'}},
 'gif_ball_ascii': {'ascii0': '                    \n'
                              '                    \n'
                              '                    \n'
                              '                    \n'
                              '                    \n'
                              '                    \n'
                              '                    \n'
                              '      .dKXXKd.      \n'
                              '      .KWMMWK.      \n'
                              '  .....,cllc,.....  \n',
                    'delays': [0.04,
                               0.04,
                               0.04,
                               0.04,
                               0.04,
                               0.04,
                               0.04,
                               0.04,
                               0.04,
                               0.04,
                               0.04,
                               0.04,
                               0.04,
                               0.04,
                               0.04,
                               0.04],
                    'duration': 0.64,
                    'frame_count': 16,
                    'frames': ['                    \n'
                               '                    \n'
                               '                    \n'
                               '                    \n'
                               '                    \n'
                               '                    \n'
                               '                    \n'
                               '      .dKXXKd.      \n'
                               '      .KWMMWK.      \n'
                               '  .....,cllc,.....  \n',
                               '                    \n'
                               '                    \n'
                               '                    \n'
                               '                    \n'
                               '        ....        \n'
                               '       kWMMWk       \n'
                               '       xNMMNx       \n'
                               '        ....        \n'
                               '                    \n'
                               '  ................  \n',
                               '                    \n'
                               '                    \n'
                               '         ..         \n'
                               '       cKNNKc       \n'
                               '       xWMMWx       \n'
                               "        '::'        \n"
                               '                    \n'
                               '                    \n'
                               '                    \n'
                               '  ................  \n'],
                    'played': '                    \n'
                              '                    \n'
                              '                    \n'
                              '                    \n'
                              '                    \n'
                              '                    \n'
                              '                    \n'
                              '      .dKXXKd.      \n'
                              '      .KWMMWK.      \n'
                              '  .....,cllc,.....  \n',
                    'print': '                    \n'
                             '                    \n'
                             '                    \n'
                             '                    \n'
                             '                    \n'
                             '                    \n'
                             '                    \n'
                             '      .dKXXKd.      \n'
                             '      .KWMMWK.      \n'
                             '  .....,cllc,.....  \n'},
 'gif_ball_blocks': {'ascii0': {'len': 1121,
                                'sha256': '039745f8e798956ef352a24fd7b0ccca77347badabb725ac8cd7ceea6fc7d6a1'},
                     'delays': [0.04,
                                0.04,
                                0.04,
                                0.04,
                                0.04,
                                0.04,
                                0.04,
                                0.04,
                                0.04,
                                0.04,
                                0.04,
                                0.04,
                                0.04,
                                0.04,
                                0.04,
                                0.04],
                     'duration': 0.64,
                     'frame_count': 16,
                     'frames': [{'len': 7090,
                                 'sha256': 'c5a145edf527c040f0657a08ee4c42f889d69a7f9f6b95730203d2bc796e0352'},
                                {'len': 7092,
                                 'sha256': 'f04d87aab1b93822d3e7f9102d9ca5d508da71eb2dc961c6da6c2c082b13658e'},
                                {'len': 7096,
                                 'sha256': '6c2b8dfcd5b09e8bc19eec6a64a9f33e6de6b9f81f81071405f2c8573331effe'}],
                     'played': '                    \n'
                               '                    \n'
                               '                    \n'
                               '                    \n'
                               '                    \n'
                               '                    \n'
                               '                    \n'
                               '      .dKXXKd.      \n'
                               '      .KWMMWK.      \n'
                               '  .....,cllc,.....  \n',
                     'print': {'len': 7090,
                               'sha256': 'c5a145edf527c040f0657a08ee4c42f889d69a7f9f6b95730203d2bc796e0352'}},
 'gif_capped': {'ascii0': '@@@@@@@@@@\n@@@@@@@@@@\n@@@@@@@@@@\n@@@OooO@@@\n@@@o  o@@@\n',
                'delays': [0.5, 0.5, 0.5, 0.5, 0.5, 0.5, 0.5, 0.5, 0.5, 0.5, 0.5, 0.5, 0.5, 0.5, 0.5, 0.5],
                'duration': 8.0,
                'frame_count': 16,
                'frames': ['@@@@@@@@@@\n@@@@@@@@@@\n@@@@@@@@@@\n@@@OooO@@@\n@@@o  o@@@\n',
                           '@@@@@@@@@@\n@@@@@@@@@@\n@@@O  O@@@\n@@@O  O@@@\n@OOOOOOOO@\n',
                           '@@@@@@@@@@\n@@@O..O@@@\n@@@O  O@@@\n@@@@@@@@@@\n@OOOOOOOO@\n'],
                'played': '@@@@@@@@@@\n@@@@@@@@@@\n@@@@@@@@@@\n@@@OooO@@@\n@@@o  o@@@\n',
                'print': '@@@@@@@@@@\n@@@@@@@@@@\n@@@@@@@@@@\n@@@OooO@@@\n@@@o  o@@@\n'},
 'gif_cat_ansi16': {'ascii0': {'len': 802,
                               'sha256': '9d8554635f8e3085b98730f52c6ae475fd2d2e5983498b97aa6fbe5e40ad5e94'},
                    'delays': [0.08, 0.08, 0.08, 0.08, 0.08, 0.08, 0.08, 0.08, 0.08, 0.08, 0.08, 0.08],
                    'duration': 0.96,
                    'frame_count': 12,
                    'frames': [{'len': 1736,
                                'sha256': '10d8a430c58fa57f323d4117e52a1151709f8a5519090d3c3960334b9eb1d048'},
                               {'len': 1736,
                                'sha256': 'b4ccfd4bdc06d43f567232384512d75144171e772aa89c1c4d3ad4a7e573b596'},
                               {'len': 1740,
                                'sha256': 'ebc851a12274d73287c72091a6705b206f9439876566d8b194c8f3f80da17092'}],
                    'played': '     ;     ;    \n'
                              '    ;dd:::d;    \n'
                              '    ;d:Mddd;    \n'
                              '  ; :d:dM:d:d:; \n'
                              ' ;;;;:dMdM:d::  \n'
                              ';:; ;:MMMddd;   \n'
                              ' ;::dMdMdMdd    \n'
                              '   ;ddMMMMd;    \n',
                    'print': {'len': 1736,
                              'sha256': '10d8a430c58fa57f323d4117e52a1151709f8a5519090d3c3960334b9eb1d048'}},
 'mode_ascii_color': {'out': {'len': 6812,
                              'sha256': '84ee46ba7a68feda8bf78c3e17774c9c0ef58d51cd59f5574f1d104245cefd6c'}},
 'mode_ascii_plain': {'out': {'len': 310,
                              'sha256': '9a98d15a9f625a1fd3bdb5358c050db364fdb2fef12467535ec1b20be2331265'}},
 'mode_auto_color': {'out': {'len': 3083,
                             'sha256': 'a6f984d03a91fce97ccc27c741fce4cfef66aa0022aa5bb99e6efda8a3fe2299'}},
 'mode_auto_plain': {'out': "    .....'',,;;:\n"
                            "...',,,;;:cccloo\n"
                            ',;:::clloddxxkkk\n'
                            'cooodxxkkOO0KKKX\n'
                            'dkkOO00KKXXNWWWM\n'},
 'mode_blocks': {'out': {'len': 11513,
                         'sha256': 'cd1f9781a9e5a3110a0d5f5f71b0c4de16f38ac17eb3cf305b2688ca92a0fea2'}},
 'mode_braille': {'out': {'len': 310,
                          'sha256': 'e8c197c18072a64fd341a24a11dfae194d4c76998faae0ae78027f98f1a908e6'}},
 'mode_quadrants': {'out': {'len': 11530,
                            'sha256': 'cd2fe7799f90675d2089e52c1eead7860c74a902dc8c1eaacc6fc5704d91d9ae'}},
 'mode_sixel': {'out': {'len': 9390,
                        'sha256': '9b08ac5116ad12fa6ce1ace48d906ca2c84d48a6286a1d394d8ad06215f5e44a'}},
 'mode_sixel_ansi16': {'out': {'len': 1636,
                               'sha256': '112d56f9dbbb363770b7d994e873a45a428e5ed25c62c30cf1fd6226a1cbda08'}},
 'mode_sixel_file': {'error': 'Sixel graphics require a terminal destination; use ASCII, Braille, or blocks '
                              'when redirecting output'},
 'mode_sixel_grayscale': {'out': {'len': 926,
                                  'sha256': '131e58150850256ecab217c20e1f1c0b436bd334fa7b00e5087660434459592f'}},
 'native_grid_ascii': {'out': [24, 8]},
 'native_grid_braille': {'out': [12, 4]},
 'native_grid_sixel': {'out': [2, 1]},
 'quadrant': {'out': {'len': 1170,
                      'sha256': '8c449b5c45876a1c0e92e242728e6e81a250ff4df254bee2b2dbb32d40492049'}},
 'sixel_encode': {'out': {'len': 609,
                          'sha256': 'af14da8441660326fc426e199a1908410afbfb07684fd1064a693b70de261620'}},
 'sixel_print': {'out': {'len': 7481,
                         'sha256': '95b3834a9e41415af3c3741cb296f3ac252bf6a141502643067faab062e3fcbe'}},
 'size_max_height_ascii': {'out': '  ...,;:c\n,;:cldxkO\ndxkOKXNWM\n'},
 'size_max_height_blocks': {'out': {'len': 1057,
                                    'sha256': '20293c94cd5f1930312f5faa50b4b42a2779542e41d3b6f560caf6deaeb8ff5a'}},
 'size_max_width': {'out': {'len': 1175,
                            'sha256': '9772e58fdc7ed843d282489db1af8bfb5a6e3c5a204ddfcd831c8c3b6b9d1d6a'}},
 'size_width_height': {'out': "   ...',;;:c\n';::clodxxkO\noxkOO0KXNWWM\n"},
 'stage_two': {'out': '                   ,.  .,    \n'
                      '                   0KkkK0    \n'
                      '                  .KxNNxK,,  \n'
                      '                ..;KXWWXXXX; \n'
                      "    kMMk       .'..lNMMN00.  \n"
                      ' ...xWWx...    .cldWMMMMMc   \n'
                      '                 .lNMMMMO.   \n'}}
# fmt: on
