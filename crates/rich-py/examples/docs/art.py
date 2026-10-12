"""The pictures on docs/python/art.md: run it, or scripts/capture_python_docs.py."""

from _shots import Shots

from rs_rich import art


# --8<-- [start:gradient]
def gradient(w, h):
    pixels = bytearray()
    for y in range(h):
        for x in range(w):
            pixels += bytes((x * 255 // (w - 1), y * 255 // (h - 1), 128, 255))
    return art.ArtImage.frombytes("RGBA", (w, h), pixels)


image = gradient(32, 16)
# --8<-- [end:gradient]


def ascii_art(console):
    # --8<-- [start:ascii]
    console.print(art.ImageArt(image, mode="ascii", width=32))
    # --8<-- [end:ascii]


def braille(console):
    # --8<-- [start:braille]
    console.print(art.ImageArt(image, mode="braille", width=16))
    console.print(art.ImageArt(image, mode="ascii", fit="cover", width=12, height=3, anchor="left"))
    # --8<-- [end:braille]


def colour(console):
    # --8<-- [start:color]
    console.print(art.ImageArt(image, mode="ascii", width=32, color=True))
    console.print(art.ImageArt(image, mode="blocks", width=32))
    console.print(art.ImageArt(image, mode="quadrants", width=16))
    # --8<-- [end:color]


def figlet(console):
    # --8<-- [start:figlet]
    console.print(art.Figlet("rs", justify="center"))
    # --8<-- [end:figlet]


shots = Shots("art")
shots.shot("ascii", ascii_art, width=40)
shots.shot("braille", braille, width=40)
shots.shot("color", colour, width=40)
shots.shot("figlet", figlet, width=40)
shots.finish()
