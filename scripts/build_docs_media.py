#!/usr/bin/env python3
"""Convert rs-rich's committed SVG frames and verified PTY recordings to media.

Run from the repository root. Requires Pillow, CairoSVG, pyte, FFmpeg and
DejaVu Sans Mono. This replays recorded output; it does not run the Rust CLI.
"""
import copy
import hashlib
import io
import json
import re
from pathlib import Path
import subprocess
import xml.etree.ElementTree as ET
import zipfile

import cairosvg
from PIL import Image, ImageDraw, ImageFont
import pyte

ROOT = Path(__file__).resolve().parents[1]
OUT = ROOT / 'docs/assets/demos'
FONT = '/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf'
BG = '#14101c'


def svg_animation(name, duration):
    source = ROOT / f'docs/assets/{name}-animated.svg'
    root = ET.fromstring(source.read_text())
    ns = '{http://www.w3.org/2000/svg}'
    style = root.find(ns + 'style').text.split('@keyframes')[0]
    style = style.replace('Fira Code, monospace', 'DejaVu Sans Mono, DejaVu Sans, monospace')
    frames = []
    for group in root.findall(ns + 'g'):
        scene = ET.Element(ns + 'svg', root.attrib)
        ET.SubElement(scene, ns + 'style').text = style
        scene.append(copy.deepcopy(group))
        # Cairo does not automatically fall back for missing Braille glyphs.
        xml = ET.tostring(scene, encoding='unicode')
        xml = re.sub(r'([\u2800-\u28ff])',
                     r'<tspan xmlns="http://www.w3.org/2000/svg" font-family="DejaVu Sans">\1</tspan>', xml)
        png = cairosvg.svg2png(bytestring=xml.encode(), scale=2)
        frames.append(Image.open(io.BytesIO(png)).convert('RGB'))
    assert len(frames) > 1, 'expected exported animation frames'
    frames[0].save(OUT / f'{name}.gif', save_all=True,
                   append_images=frames[1:], duration=round(duration / len(frames)),
                   loop=0, disposal=2)


def color(value, default):
    palette = {'black':'000000','red':'cd0000','green':'00cd00','brown':'cdcd00',
               'blue':'0000ee','magenta':'cd00cd','cyan':'00cdcd','white':'e5e5e5',
               'brightblack':'7f7f7f','brightred':'ff0000','brightgreen':'00ff00',
               'brightbrown':'ffff00','brightyellow':'ffff00','brightblue':'5c5cff',
               'brightmagenta':'ff00ff','brightcyan':'00ffff','brightwhite':'ffffff',
               # pyte 0.8.2 spells SGR 105 (bright magenta background) this way.
               'bfightmagenta':'ff00ff'}
    if value == 'default': return default
    return '#' + palette.get(value, value)


def replay(events):
    screen = pyte.Screen(80, 24)
    stream = pyte.Stream(screen)
    font = ImageFont.truetype(FONT, 22)
    title = ImageFont.truetype(FONT, 16)
    frames = []
    raw = ''.join(e[2] for e in events if e[1] == 'o')
    # Present complete Live frames, avoiding partial PTY writes during repaint.
    chunks = re.split(r'\r\x1b\[2K(?:\x1b\[1A\x1b\[2K)*', raw)
    for chunk in chunks:
        screen.reset()
        stream.feed(chunk)
        im = Image.new('RGB', (480, 476), BG)
        draw = ImageDraw.Draw(im)
        for y in range(16):
            for x in range(32):
                ch = screen.buffer[y][x]
                fg, bg = color(ch.fg, '#d8d4e0'), color(ch.bg, BG)
                if ch.reverse: fg, bg = bg, fg
                px, py = 16 + x * 14, 20 + y * 26
                draw.rectangle((px, py, px+13, py+25), fill=bg)
                if ch.data == '▀':
                    draw.rectangle((px, py, px+13, py+12), fill=fg)
                elif ch.data.strip():
                    draw.text((px, py), ch.data, font=font, fill=fg)
        draw.text((16, 452), 'Recorded frames | 100 ms/frame', font=title, fill='#bcb3cc')
        frames.extend([im] * 3)
    return frames


def save_video(frames, name):
    w, h = frames[0].size
    cmd = ['ffmpeg','-y','-loglevel','error','-f','rawvideo','-pix_fmt','rgb24',
           '-s',f'{w}x{h}','-r','30','-i','-','-an','-c:v','libx264',
           '-crf','18','-pix_fmt','yuv420p','-movflags','+faststart',str(OUT/f'{name}.mp4')]
    process = subprocess.Popen(cmd, stdin=subprocess.PIPE)
    for _ in range(3):
        for frame in frames: process.stdin.write(frame.tobytes())
    process.stdin.close()
    if process.wait(): raise RuntimeError('FFmpeg failed')
    frames[len(frames)//2].save(OUT/f'{name}.png')


def main():
    OUT.mkdir(parents=True, exist_ok=True)
    svg_animation('progress', 3000)
    svg_animation('spinner', 1000)
    evidence = ROOT / '.github/evidence/v0.0.4-gif'
    expected = {r['case']:r for r in json.loads((evidence/'pty-results.json').read_text())}
    rendered = {}
    with zipfile.ZipFile(evidence/'recordings.zip') as archive:
        for case in ['truecolor','ascii']:
            ansi = archive.read(f'{case}.ansi')
            assert hashlib.sha256(ansi).hexdigest() == expected[case]['sha256']
            lines = archive.read(f'{case}.cast').decode().splitlines()
            events = [json.loads(line) for line in lines[1:]]
            assert ''.join(e[2] for e in events if e[1]=='o').encode() == ansi
            frames = replay(events)
            rendered[case] = frames
            save_video(frames, f'rich-art-{case}')
    pairs = []
    font = ImageFont.truetype(FONT, 20)
    for left,right in zip(rendered['truecolor'],rendered['ascii']):
        im = Image.new('RGB',(980,530),BG)
        im.paste(left,(0,45));im.paste(right,(500,45))
        d=ImageDraw.Draw(im)
        d.text((16,12),'HALF-BLOCK / TRUECOLOR',font=font,fill='#d9c4ff')
        d.text((516,12),'ASCII / TRUECOLOR',font=font,fill='#d9c4ff')
        pairs.append(im)
    save_video(pairs,'rich-art-comparison')
    small=[im.resize((784,424)) for im in pairs[::3]]
    small[0].save(OUT/'rich-art-comparison.gif',save_all=True,
                  append_images=small[1:],duration=100,loop=0,disposal=2)
    print('Verified recorded ANSI hashes and exported 3 MP4s, 3 GIFs, and posters.')


if __name__ == '__main__':
    main()
