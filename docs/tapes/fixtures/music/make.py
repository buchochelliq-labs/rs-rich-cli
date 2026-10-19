"""Write the music tape's library (docs/tapes/music.tape) into a directory.

Short silent WAV files (8 kHz, 8-bit mono, so each is a few kilobytes) in
two album folders, named `Artist - Title.wav` as the example reads tags
from a name, with lyrics (`.lrc`) beside one track.

    python3 make.py DIR
"""

import sys
import wave
from pathlib import Path

ALBUMS = {
    "Orbit Lights": [
        ("Nova Hale", "Morning Signal", 7),
        ("Nova Hale", "Low Earth", 5),
        ("Nova Hale", "Perigee", 9),
    ],
    "Harbour Songs": [
        ("The Tidewaters", "Rope and Lantern", 6),
        ("The Tidewaters", "Fog Bell", 8),
    ],
}

LYRICS = """[ti:Morning Signal]
[ar:Nova Hale]
[00:00.50]Wake to the hum of the morning signal
[00:02.50]Light on the dish and the dew on the wire
[00:04.50]Count down the stars as they fade one by one
"""

out = Path(sys.argv[1])
for album, tracks in ALBUMS.items():
    folder = out / album
    folder.mkdir(parents=True, exist_ok=True)
    for artist, title, seconds in tracks:
        with wave.open(str(folder / f"{artist} - {title}.wav"), "wb") as w:
            w.setnchannels(1)
            w.setsampwidth(1)
            w.setframerate(8000)
            w.writeframes(b"\x80" * 8000 * seconds)
(out / "Orbit Lights" / "Nova Hale - Morning Signal.lrc").write_text(LYRICS)
