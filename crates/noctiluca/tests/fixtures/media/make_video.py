"""Make Noctiluca's video fixture, motion.webm, and its poster, poster.jpg.

"The bloom from the air": a dark sea at night seen from above, waves
rolling in to a curved shore and breaking in bioluminescent blue-green,
specks of plankton glowing in the swell, a lamp-lit coast road, and a boat
crossing the bay with a glowing wake. Five seconds at 12 frames a second,
480 x 270, VP9 in WebM; every motion repeats in five seconds, so the clip
loops without a seam. The poster is its first frame at 640 x 360.

Every frame is drawn here from gradients and sines, so the fixture carries
no third-party footage. Run from anywhere: `python3 -I make_video.py`
(needs NumPy, Pillow and an ffmpeg with libvpx-vp9); it writes beside
itself. The frames are deterministic; the encode is single-threaded and
bit-exact.
"""

import math
import os
import subprocess
import tempfile

import numpy as np
from PIL import Image, ImageFilter

HERE = os.path.dirname(os.path.abspath(__file__))

SIZE = (480, 270)
FPS = 12
SECONDS = 5
FRAMES = FPS * SECONDS
# Every motion's period divides the clip's length.
LOOP = float(SECONDS)

SEA_DEEP = np.array([3, 9, 24], dtype=np.float32)
SEA_SHALLOW = np.array([6, 22, 40], dtype=np.float32)
SAND = np.array([24, 21, 20], dtype=np.float32)
LAND = np.array([10, 12, 13], dtype=np.float32)
GLOW_BLUE = np.array([40, 190, 255], dtype=np.float32)
GLOW_GREEN = np.array([60, 255, 190], dtype=np.float32)
LAMP = np.array([255, 196, 110], dtype=np.float32)


def shore(x):
    """The shoreline's height at `x` (both in units of the frame's width):
    a wide bay, the coast rising to the right."""
    return (
        0.34
        + 0.09 * np.sin(math.pi * x * 0.95 + 0.1)
        - 0.03 * x
        + 0.012 * np.sin(x * 13.0 + 1.3)
    )


def noise(seed, shape, blur):
    """Smooth random field in 0..1, the same every run."""
    rng = np.random.default_rng(seed)
    field = rng.random(shape, dtype=np.float32)
    image = Image.fromarray((field * 255).astype(np.uint8)).filter(ImageFilter.GaussianBlur(blur))
    out = np.asarray(image, dtype=np.float32)
    out -= out.min()
    return out / max(1.0, out.max())


def frame(t, size):
    """The scene at `t` seconds, as an RGB array of `size`."""
    w, h = size
    aspect = h / w
    xs = (np.arange(w, dtype=np.float32) + 0.5) / w
    ys = (np.arange(h, dtype=np.float32) + 0.5) / w  # in widths, so square
    x, y = np.meshgrid(xs, ys)
    phase = 2 * math.pi * t / LOOP

    # Distance out to sea from the shore (positive on the water), in widths.
    edge = shore(x)
    d = edge - y + (aspect - 0.5625) * 0.5
    # The swash: the waterline itself moves up and down the sand.
    swash = 0.006 * np.sin(2 * phase * 1 - 1.2 + x * 3.0)
    d = d + swash

    # Fixed textures, scaled to the frame.
    grain = np.asarray(
        Image.fromarray((noise(7, (90, 160), 2.0) * 255).astype(np.uint8)).resize((w, h), Image.BICUBIC),
        dtype=np.float32,
    ) / 255.0
    patches = np.asarray(
        Image.fromarray((noise(11, (45, 80), 3.0) * 255).astype(np.uint8)).resize((w, h), Image.BICUBIC),
        dtype=np.float32,
    ) / 255.0

    # Water: deep far out, lighter over the shallows.
    depth = np.clip(d / 0.35, 0.0, 1.0)[..., None]
    water = SEA_SHALLOW * (1 - depth) + SEA_DEEP * depth
    # The swell, slow broad ripples across the open water.
    swell = 0.5 + 0.5 * np.sin(x * 38 + y * 22 - phase)
    water = water * (0.85 + 0.25 * swell[..., None] * grain[..., None])

    # Land: dark sand at the water's edge, then the dark coast.
    sand = np.clip(-d / 0.035, 0.0, 1.0)[..., None]
    land = SAND * (1 - sand) + LAND * sand
    # Fields and scrub inland, in patches.
    scrub = np.array([14, 20, 14], dtype=np.float32) * patches[..., None]
    land = land * (0.8 + 0.4 * grain[..., None]) + scrub * sand
    on_land = np.clip(-d * w / 1.5 + 0.5, 0.0, 1.0)[..., None]
    image = water * (1 - on_land) + land * on_land

    # Waves rolling in: crests parallel to the shore, two every loop each,
    # so every crest has moved on by exactly one spacing when it repeats.
    glow = np.zeros_like(x)
    spacing = 0.06
    count = 5
    travel = (t / LOOP) * 2 * spacing
    bend = 0.010 * np.sin(x * 15.0 + 0.7) + 0.005 * np.sin(x * 37.0 + 2.0)
    for k in range(count):
        base = (spacing * (k + 1) - travel) % (count * spacing)
        crest = base + bend * (0.3 + base / (count * spacing))
        # Brighter as it nears the shore and breaks, patchy along its
        # length, and gone into the swash as it lands.
        near = np.clip(1.0 - crest / (count * spacing), 0.0, 1.0)
        lands = np.clip(base / 0.03, 0.0, 1.0)
        strength = (0.05 + 0.95 * near**3) * (0.1 + 2.4 * patches**3) * lands
        width = 0.003 + 0.007 * near
        band = np.exp(-((d - crest) / width) ** 2)
        # The broken water behind a crest stays lit, fading out to sea.
        behind = np.clip(d - crest, 0.0, None)
        foam = np.where(d > crest, np.exp(-behind / 0.025), 0.0) * near**4 * 0.45
        glow += (band + foam) * strength * (d > -0.002)
    # The breaking line: a lit edge along the waterline, flaring as each
    # wave arrives.
    arrive = 0.5 + 0.5 * np.cos(2 * phase - x * 9.0)
    edge_glow = np.exp(-((d - 0.004) / 0.006) ** 2) * (0.5 + 0.9 * arrive**2) * (0.5 + 0.7 * patches)
    glow += edge_glow

    # Plankton specks twinkling in the swell (each with a whole number of
    # blinks a loop).
    rng = np.random.default_rng(23)
    specks = np.zeros_like(x)
    for _ in range(90):
        sx, sy = rng.random(), rng.random() * aspect
        if shore(np.float32(sx)) - sy < 0.03:
            continue
        rate = rng.integers(1, 3)
        level = max(0.0, math.sin(2 * math.pi * rate * t / LOOP + rng.random() * 6.3))
        r = 0.0035
        specks += np.exp(-((x - sx) ** 2 + (y - sy) ** 2) / (r * r)) * level * 0.8
    glow += specks

    # The boat, crossing the bay from left to right once a loop; it is off
    # screen at the seam. Its wake glows behind it, wider and dimmer aft.
    margin = 0.12
    def boat_x(at):
        return ((at / LOOP + 0.45) % 1.0) * (1 + 2 * margin) - margin
    bx = boat_x(t)
    by = 0.105
    wake = np.zeros_like(x)
    for s in np.linspace(0.0, 1.8, 73):
        px = boat_x(t - s)
        if abs(px - bx) > 0.6:
            break
        spread = 0.003 + 0.03 * s
        fade = math.exp(-s * 1.4)
        column = np.exp(-((x - px) ** 2) / (0.004**2))
        arms = np.exp(-((y - by - spread) ** 2) / ((0.003 + 0.004 * s) ** 2)) + np.exp(
            -((y - by + spread) ** 2) / ((0.003 + 0.004 * s) ** 2)
        )
        trail = np.exp(-((y - by) ** 2) / ((0.004 + 0.01 * s) ** 2))
        wake += column * (arms * 0.35 + trail * 0.3) * fade
    glow += np.minimum(wake, 1.4)

    # The glow's colour: blue in the open, greener where it is brightest.
    g = np.clip(glow, 0.0, 2.0)[..., None]
    tint = np.clip(g / 1.2, 0.0, 1.0)
    colour = GLOW_BLUE * (1 - tint) + GLOW_GREEN * tint
    lit = image + colour * np.clip(g, 0.0, 1.0) + 255 * np.clip(g - 1.0, 0.0, 1.0) * 0.35

    # The boat itself: a pale hull pointing the way it goes, a cabin, and
    # its masthead light.
    hull = (np.abs(y - by) < 0.008 * np.clip((bx + 0.025 - x) / 0.012, 0.0, 1.0)) & (x > bx - 0.03) & (x < bx + 0.025)
    lit = np.where(hull[..., None], np.array([200, 205, 212], dtype=np.float32), lit)
    cabin = (np.abs(y - by) < 0.0045) & (x > bx - 0.02) & (x < bx - 0.002)
    lit = np.where(cabin[..., None], np.array([120, 126, 136], dtype=np.float32), lit)
    lit += LAMP * np.exp(-((x - bx + 0.008) ** 2 + (y - by) ** 2) / (0.004**2))[..., None] * 0.9

    # The coast road, a faint line inland, its lamps and their pools of
    # light.
    inland = (aspect - 0.5625) * 0.5 + 0.055
    road = np.exp(-((y - shore(x) - inland) / 0.0035) ** 2) * (d < -0.02)
    lit += np.array([46, 44, 42], dtype=np.float32) * road[..., None]
    for lx in np.arange(0.03, 1.0, 0.07):
        ly = float(shore(np.float32(lx))) + inland
        if ly > aspect - 0.01:
            continue
        pool = np.exp(-((x - lx) ** 2 + (y - ly) ** 2) / (0.018**2)) * 0.18
        point = np.exp(-((x - lx) ** 2 + (y - ly) ** 2) / (0.0035**2))
        lit += LAMP * (pool + point)[..., None]

    return np.clip(lit, 0, 255).astype(np.uint8)


def render(t, size):
    """The frame at `t` as a Pillow image, softened a little as a camera's
    would be."""
    return Image.fromarray(frame(t, size)).filter(ImageFilter.GaussianBlur(0.5))


def main():
    with tempfile.TemporaryDirectory() as tmp:
        for n in range(FRAMES):
            render(n / FPS, SIZE).save(os.path.join(tmp, f"{n:03}.png"))
        subprocess.run(
            [
                "ffmpeg", "-v", "error", "-y",
                "-framerate", str(FPS), "-i", os.path.join(tmp, "%03d.png"),
                "-c:v", "libvpx-vp9", "-pix_fmt", "yuv420p",
                "-b:v", "0", "-crf", "24", "-g", str(FRAMES),
                "-deadline", "good", "-cpu-used", "1", "-threads", "1", "-row-mt", "0",
                "-an", "-map_metadata", "-1",
                "-fflags", "+bitexact", "-flags:v", "+bitexact",
                os.path.join(HERE, "motion.webm"),
            ],
            check=True,
        )
    render(0.0, (640, 360)).save(os.path.join(HERE, "poster.jpg"), quality=85)


if __name__ == "__main__":
    main()
