"""Make the WebP fixtures from the crate's own ball.gif (examples/assets).

- ball.webp: the GIF's first frame, a still, lossless.
- ball-animated.webp: every frame with the GIF's delays, lossless, looping.

Run from anywhere: `python3 -I make_webp.py` (needs Pillow built with
libwebp); it writes beside itself.
"""

import os

from PIL import Image, ImageSequence

HERE = os.path.dirname(os.path.abspath(__file__))
SOURCE = os.path.join(HERE, "..", "..", "examples", "assets", "ball.gif")

with Image.open(SOURCE) as gif:
    frames = [frame.convert("RGBA") for frame in ImageSequence.Iterator(gif)]
    durations = []
    for index in range(gif.n_frames):
        gif.seek(index)
        durations.append(gif.info.get("duration", 100))

frames[0].save(os.path.join(HERE, "ball.webp"), lossless=True, method=6)
frames[0].save(
    os.path.join(HERE, "ball-animated.webp"),
    save_all=True,
    append_images=frames[1:],
    duration=durations,
    loop=0,
    lossless=True,
    method=6,
)
