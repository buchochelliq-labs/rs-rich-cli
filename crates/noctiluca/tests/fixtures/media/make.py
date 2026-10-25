"""Make the pictures of Noctiluca's media fixtures (news.html, media-grid.html).

Every picture is drawn here from shapes and gradients, so the fixtures carry
no third-party image. Run from anywhere: `python3 -I make.py` (needs Pillow);
it writes beside itself. The output is deterministic.
"""

import math
import os
import random

from PIL import Image, ImageDraw, ImageFilter

HERE = os.path.dirname(os.path.abspath(__file__))


def gradient(size, top, bottom):
    w, h = size
    image = Image.new("RGB", size)
    draw = ImageDraw.Draw(image)
    for y in range(h):
        t = y / max(1, h - 1)
        draw.line(
            [(0, y), (w, y)],
            fill=tuple(int(a + (b - a) * t) for a, b in zip(top, bottom)),
        )
    return image


def harbour(size=(640, 360)):
    """A harbour at night: lights on the water, boats, a quay."""
    rng = random.Random(1)
    w, h = size
    image = gradient(size, (12, 18, 48), (40, 60, 110))
    draw = ImageDraw.Draw(image)
    horizon = int(h * 0.55)
    # Town on the far shore.
    x = 0
    while x < w:
        bw, bh = rng.randint(20, 60), rng.randint(20, 90)
        draw.rectangle([x, horizon - bh, x + bw, horizon], fill=(18, 22, 40))
        for _ in range(rng.randint(1, 6)):
            lx, ly = x + rng.randint(3, bw - 3), horizon - rng.randint(5, bh - 2)
            draw.rectangle([lx, ly, lx + 3, ly + 4], fill=(255, 200, 90))
        x += bw + rng.randint(0, 8)
    # Water with reflections.
    draw.rectangle([0, horizon, w, h], fill=(10, 24, 52))
    for _ in range(400):
        rx, ry = rng.randint(0, w), rng.randint(horizon, h)
        length = rng.randint(4, 30)
        colour = rng.choice([(255, 190, 80), (90, 160, 255), (40, 70, 120)])
        draw.line([(rx, ry), (rx + length, ry)], fill=colour, width=1)
    # Boats.
    for bx in (120, 360, 520):
        draw.polygon(
            [(bx, horizon + 40), (bx + 70, horizon + 40), (bx + 58, horizon + 58), (bx + 10, horizon + 58)],
            fill=(220, 220, 230),
        )
        draw.line([(bx + 35, horizon + 40), (bx + 35, horizon - 10)], fill=(200, 200, 210), width=2)
    # Moon.
    draw.ellipse([w - 120, 30, w - 80, 70], fill=(250, 245, 220))
    return image.filter(ImageFilter.GaussianBlur(0.6))


def market(size=(640, 360)):
    """A market: striped awnings and crates of fruit."""
    rng = random.Random(2)
    w, h = size
    image = gradient(size, (250, 220, 170), (200, 150, 110))
    draw = ImageDraw.Draw(image)
    colours = [(220, 50, 60), (250, 200, 40), (60, 150, 80), (40, 110, 200), (240, 120, 30)]
    stalls = 5
    for i in range(stalls):
        x0, x1 = i * w // stalls, (i + 1) * w // stalls
        a, b = colours[i % 5], (250, 250, 245)
        for k in range(8):
            sx0 = x0 + k * (x1 - x0) // 8
            sx1 = x0 + (k + 1) * (x1 - x0) // 8
            draw.polygon([(sx0, 40), (sx1, 40), (sx1 + 6, 120), (sx0 + 6, 120)], fill=a if k % 2 else b)
        draw.rectangle([x0 + 8, 200, x1 - 8, h], fill=(120, 80, 45))
        for _ in range(60):
            fx, fy = rng.randint(x0 + 12, x1 - 20), rng.randint(190, 260)
            r = rng.randint(6, 11)
            draw.ellipse([fx, fy, fx + r * 2, fy + r * 2], fill=rng.choice(colours))
    return image


def storm(size=(640, 360)):
    """A storm over fields: dark clouds and lightning."""
    rng = random.Random(3)
    w, h = size
    image = gradient(size, (30, 30, 45), (90, 95, 110))
    draw = ImageDraw.Draw(image)
    for _ in range(60):
        cx, cy, r = rng.randint(-40, w), rng.randint(-20, 120), rng.randint(30, 90)
        g = rng.randint(40, 80)
        draw.ellipse([cx - r, cy - r // 2, cx + r, cy + r // 2], fill=(g, g, g + 12))
    x, y = 380, 90
    points = [(x, y)]
    while y < h * 0.7:
        x += rng.randint(-25, 25)
        y += rng.randint(15, 35)
        points.append((x, y))
    draw.line(points, fill=(240, 240, 255), width=4)
    draw.rectangle([0, int(h * 0.72), w, h], fill=(50, 80, 40))
    return image.filter(ImageFilter.GaussianBlur(1.0))


def field(size=(640, 360)):
    """Hills under a low sun."""
    w, h = size
    image = gradient(size, (255, 180, 120), (255, 230, 170))
    draw = ImageDraw.Draw(image)
    draw.ellipse([w // 2 - 50, h // 2 - 90, w // 2 + 50, h // 2 + 10], fill=(255, 245, 210))
    for k, colour in enumerate([(120, 160, 80), (80, 130, 60), (50, 100, 50)]):
        base = int(h * (0.55 + 0.13 * k))
        points = [(0, h)]
        for x in range(0, w + 1, 16):
            points.append((x, base + int(18 * math.sin(x / (60 + 20 * k) + k))))
        points.append((w, h))
        draw.polygon(points, fill=colour)
    return image


def thumbnail(n, size=(320, 180)):
    """A video thumbnail: a bold scene in one of six palettes."""
    palettes = [
        ((20, 40, 90), (60, 160, 220)),
        ((90, 20, 60), (240, 120, 90)),
        ((10, 60, 40), (120, 220, 140)),
        ((60, 40, 10), (250, 200, 80)),
        ((30, 10, 70), (170, 110, 250)),
        ((15, 15, 15), (200, 200, 200)),
    ]
    top, bottom = palettes[n % len(palettes)]
    rng = random.Random(10 + n)
    w, h = size
    image = gradient(size, top, bottom)
    draw = ImageDraw.Draw(image)
    for _ in range(6):
        r = rng.randint(20, 60)
        cx, cy = rng.randint(0, w), rng.randint(0, h)
        tone = tuple(min(255, c + 60) for c in bottom)
        draw.ellipse([cx - r, cy - r, cx + r, cy + r], outline=tone, width=6)
    # A duration badge, as video lists show.
    draw.rectangle([w - 58, h - 26, w - 6, h - 6], fill=(0, 0, 0))
    return image


def avatar(n, size=(64, 64)):
    """A round avatar: a face of shapes on a coloured disc."""
    colours = [(230, 90, 80), (80, 150, 230), (90, 190, 120), (240, 180, 60)]
    image = Image.new("RGBA", size, (0, 0, 0, 0))
    draw = ImageDraw.Draw(image)
    w, h = size
    draw.ellipse([0, 0, w - 1, h - 1], fill=colours[n % len(colours)] + (255,))
    draw.ellipse([w * 0.3, h * 0.22, w * 0.7, h * 0.62], fill=(250, 225, 200, 255))
    draw.pieslice([w * 0.15, h * 0.55, w * 0.85, h * 1.2], 180, 360, fill=(250, 250, 250, 255))
    return image


def poster(size=(640, 360)):
    """The player's poster: an abstract motion scene."""
    w, h = size
    image = gradient(size, (10, 20, 40), (30, 80, 120))
    draw = ImageDraw.Draw(image)
    for k in range(9):
        y = 40 + k * 34
        draw.arc([-80, y - 120, w + 80, y + 120], 200, 340, fill=(80 + k * 15, 200, 255 - k * 10), width=5)
    return image


def tide_gif(path, size=(160, 96), frames=12):
    """An animated GIF: waves rolling under a moon, looping."""
    w, h = size
    images = []
    for f in range(frames):
        image = gradient(size, (10, 20, 60), (20, 50, 110))
        draw = ImageDraw.Draw(image)
        draw.ellipse([w - 40, 10, w - 20, 30], fill=(250, 245, 220))
        for band in range(4):
            base = int(h * 0.5) + band * 11
            points = [(0, h)]
            for x in range(0, w + 1, 4):
                phase = 2 * math.pi * f / frames + band
                points.append((x, base + int(5 * math.sin(x / 14 + phase))))
            points.append((w, h))
            draw.polygon(points, fill=(20 + band * 20, 80 + band * 30, 160 + band * 20))
        images.append(image.convert("P", palette=Image.ADAPTIVE, colors=32))
    images[0].save(path, save_all=True, append_images=images[1:], duration=100, loop=0)


def main():
    out = HERE
    harbour().save(os.path.join(out, "harbour.jpg"), quality=82)
    market().save(os.path.join(out, "market.jpg"), quality=82)
    storm().save(os.path.join(out, "storm.jpg"), quality=82)
    field().save(os.path.join(out, "field.jpg"), quality=82)
    poster().save(os.path.join(out, "poster.jpg"), quality=82)
    for n in range(6):
        thumbnail(n).save(os.path.join(out, f"thumb-{n + 1}.jpg"), quality=80)
    for n in range(4):
        avatar(n).save(os.path.join(out, f"avatar-{n + 1}.png"), optimize=True)
    tide_gif(os.path.join(out, "tide.gif"))


if __name__ == "__main__":
    main()
