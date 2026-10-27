"""Cut Noctiluca's second video fixture, bloom.webm, and its poster,
bloom-poster.jpg, from the Noctiluca promo.

Provenance: the promo (1280 x 720, 24 fps, H.264 + AAC, 36.25 s) is an
AI-generated video the maintainer made for rs-rich; it ships under the
repository's MIT licence. The full promo, re-encoded, is in the docs
(docs/assets/noctiluca/promo.mp4); the 9.5 MB master is not committed.

The cut is "The bloom from the air": five seconds of the moonlit sea from
1.5 s, where a ring of glowing plankton appears on the water, grows and
fades. 480 x 270 at 12 frames a second, VP9 in WebM, silent (the pages
play it muted), the same format and length as make_video.py's
motion.webm, so the scripted engine replays both alike. The poster is the
frame at 5.0 s, the ring at its widest, at 640 x 360.

Run from anywhere: `python3 -I make_bloom.py PROMO` with the promo's path
(needs an ffmpeg with libvpx-vp9); it writes beside itself. The encode is
single-threaded and bit-exact for a given ffmpeg.
"""

import os
import subprocess
import sys

HERE = os.path.dirname(os.path.abspath(__file__))

START = 1.5
SECONDS = 5
FPS = 12
POSTER_AT = 5.0


def main():
    if len(sys.argv) != 2:
        sys.exit("usage: make_bloom.py PROMO")
    source = sys.argv[1]
    subprocess.run(
        [
            "ffmpeg", "-v", "error", "-y",
            "-ss", str(START), "-t", str(SECONDS), "-i", source,
            "-vf", f"fps={FPS},scale=480:270:flags=lanczos",
            "-c:v", "libvpx-vp9", "-pix_fmt", "yuv420p",
            "-b:v", "0", "-crf", "34", "-g", str(FPS * SECONDS),
            "-deadline", "good", "-cpu-used", "1", "-threads", "1", "-row-mt", "0",
            "-an", "-map_metadata", "-1",
            "-fflags", "+bitexact", "-flags:v", "+bitexact",
            os.path.join(HERE, "bloom.webm"),
        ],
        check=True,
    )
    subprocess.run(
        [
            "ffmpeg", "-v", "error", "-y",
            "-ss", str(POSTER_AT), "-i", source, "-frames:v", "1",
            "-vf", "scale=640:360:flags=lanczos", "-q:v", "4",
            "-map_metadata", "-1", "-fflags", "+bitexact", "-flags:v", "+bitexact",
            os.path.join(HERE, "bloom-poster.jpg"),
        ],
        check=True,
    )


if __name__ == "__main__":
    main()
