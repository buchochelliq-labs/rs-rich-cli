#!/usr/bin/env python3
"""Capture the built CLI in a real PTY, then encode those bytes as docs media.

Linux (DejaVu font paths); requires requirements-docs-media.txt and FFmpeg. Run after
cargo build --release -p rs-rich-cli --locked. Captures contain actual stdout/stderr; presentation
adds a command caption and holds completed still output for two seconds.
"""
import argparse
import codecs
import errno
import fcntl
import hashlib
import json
import os
from pathlib import Path
import pty
import select
import signal
import struct
import subprocess
import tempfile
import termios
import time
import zipfile

from PIL import Image, ImageDraw, ImageFont
import pyte
from build_docs_media import color

ROOT = Path(__file__).resolve().parents[1]
OUT = ROOT / 'docs/assets/demos'
EVIDENCE = ROOT / '.github/evidence/release-finish'
FONT = '/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf'
BG = '#14101c'
WIDTH, HEIGHT = 88, 28
FPS = 10


def capture(binary, args, changes=(), *, no_config=True, environment=None):
    master, slave = pty.openpty()
    fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack('HHHH', HEIGHT, WIDTH, 0, 0))
    env = dict(os.environ, TERM='xterm-256color', COLORTERM='truecolor',
               COLUMNS=str(WIDTH), LINES=str(HEIGHT), RICH_SIXEL='0')
    env.pop('NO_COLOR', None)
    if environment:
        env.update(environment)
    start = time.monotonic()
    prefix = ['--no-config'] if no_config else []
    process = subprocess.Popen([str(binary), *prefix, *args], cwd=ROOT,
                               env=env, stdin=subprocess.DEVNULL, stdout=slave, stderr=slave)
    os.close(slave)
    events, raw = [], bytearray()
    decoder = codecs.getincrementaldecoder('utf-8')()
    next_change = 0
    interrupted = False
    try:
        while True:
            elapsed = time.monotonic() - start
            if next_change < len(changes) and elapsed >= changes[next_change][0]:
                changes[next_change][1]()
                next_change += 1
            if changes and elapsed >= 4.5 and not interrupted:
                process.send_signal(signal.SIGINT)
                interrupted = True
            if elapsed > 10:
                raise TimeoutError(args)
            if select.select([master], [], [], .02)[0]:
                try:
                    data = os.read(master, 65536)
                except OSError as error:
                    if error.errno == errno.EIO:
                        break
                    raise
                if not data:
                    break
                raw.extend(data)
                events.append([round(time.monotonic()-start, 4), 'o', decoder.decode(data)])
            elif process.poll() is not None:
                break
    finally:
        os.close(master)
        if process.poll() is None:
            process.kill()
        process.wait()
    expected = -signal.SIGINT if changes else 0
    if process.returncode != expected:
        raise RuntimeError(f'{args}: exit {process.returncode}: {raw.decode(errors="replace")}')
    return events, bytes(raw), process.returncode


def frame(screen, label):
    result = Image.new('RGB', (1100, 660), BG)
    d = ImageDraw.Draw(result)
    font = ImageFont.truetype(FONT, 18)
    braille = ImageFont.truetype('/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf', 18)
    d.text((20, 12), label, font=ImageFont.truetype(FONT, 17), fill='#d9c4ff')
    for y in range(HEIGHT):
        for x in range(WIDTH):
            c = screen.buffer[y][x]
            fg, bg = color(c.fg, '#d8d4e0'), color(c.bg, BG)
            if c.reverse:
                fg, bg = bg, fg
            px, py = 20+x*12, 48+y*21
            d.rectangle((px,py,px+11,py+20),fill=bg)
            if c.data == '▀':
                d.rectangle((px,py,px+11,py+9),fill=fg)
            elif c.data.strip():
                d.text((px,py),c.data,font=braille if '\u2800' <= c.data <= '\u28ff' else font,fill=fg)
    return result


def replay(events, label, live=False):
    screen = pyte.Screen(WIDTH, HEIGHT)
    stream = pyte.Stream(screen)
    if not live:
        for _, _, data in events:
            stream.feed(data)
        return [frame(screen,label)] * (2*FPS)
    frames, index = [], 0
    for tick in range(45):
        while index < len(events) and events[index][0] <= tick/FPS:
            stream.feed(events[index][2]);index += 1
        frames.append(frame(screen,label))
    return frames


def encode(frames, name):
    path = OUT / f'{name}.mp4'
    p = subprocess.Popen(['ffmpeg','-y','-loglevel','error','-f','rawvideo','-pix_fmt','rgb24',
        '-s','1100x660','-r',str(FPS),'-i','-','-an','-c:v','libx264','-crf','18',
        '-pix_fmt','yuv420p','-movflags','+faststart',str(path)],stdin=subprocess.PIPE)
    for im in frames:
        p.stdin.write(im.tobytes())
    p.stdin.close()
    if p.wait():
        raise RuntimeError('FFmpeg failed')
    subprocess.run(['ffmpeg','-v','error','-i',str(path),'-f','null','-'],check=True)
    frames[len(frames)//2].save(OUT/f'{name}.png')


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary',type=Path,default=ROOT/'target/release/rich')
    args=parser.parse_args()
    binary=args.binary.resolve()
    OUT.mkdir(parents=True,exist_ok=True);EVIDENCE.mkdir(parents=True,exist_ok=True)
    results=[]; recordings={}; modes=[]; fits=[]
    def record(name, command, label, changes=()):
        events,raw,code=capture(binary,command,changes)
        recordings[name+'.ansi']=raw
        header={'version':2,'width':WIDTH,'height':HEIGHT,'title':name}
        recordings[name+'.cast']=(json.dumps(header)+'\n'+''.join(json.dumps(e)+'\n' for e in events)).encode()
        results.append({'case':name,'args':['--no-config',*command], 'exit_code':code,
                        'sha256':hashlib.sha256(raw).hexdigest(),'bytes':len(raw)})
        return replay(events,label,bool(changes))
    fixture='crates/rich-art/examples/assets/cat.gif'
    for mode in ['blocks','braille','ascii']:
        command=['image',fixture,'--image-mode',mode,'--width','44','--height','22']
        modes.extend(record('image-'+mode,command,f'rich image cat.gif --image-mode {mode} --width 44 --height 22'))
    for fit in ['contain','cover']:
        command=['image',fixture,'--image-mode','blocks','--width','44','--height','12',
                 '--image-fit',fit,'--image-background','#542080']
        fits.extend(record('fit-'+fit,command,f'rich image cat.gif --image-fit {fit} --width 44 --height 12'))
    # A deterministic RGBA fixture makes transparent background handling visible.
    transparent=EVIDENCE/'alpha.png'
    im=Image.new('RGBA',(64,32),(255,0,255,0));draw=ImageDraw.Draw(im)
    draw.ellipse((20,4,44,28),fill=(255,184,64,180));im.save(transparent)
    command=['image',str(transparent.relative_to(ROOT)),'--image-mode','blocks','--width','44','--height','12',
             '--image-fit','contain','--image-background','#542080']
    fits.extend(record('alpha-background',command,'Transparent RGBA input: --image-background "#542080"'))
    with tempfile.TemporaryDirectory(prefix='rich-release-demos-') as tmp:
        state=Path(tmp)/'status.json'
        state.write_text('{"status":"building","progress":10}\n')
        def update(value):
            def action():
                replacement=state.with_suffix('.tmp');replacement.write_text(value);replacement.replace(state)
            return action
        changes=[(1,update('{"status":"checking","progress":60}\n')),
                 (2,update('{ invalid JSON')),
                 (3,update('{"status":"complete","progress":100}\n'))]
        live=record('watch',['json',str(state),'--watch','--watch-interval','0.1','--width','72'],
                    'rich json status.json --watch  |  edit, invalid input, recovery',changes)
        dest=Path(tmp)/'rendered';dest.mkdir()
        batch=record('batch',['json','--batch','scripts/fixtures/workflows/a.json','scripts/fixtures/workflows/b.json',
                              '--export-html',str(dest/'document.html'),'--report','json','--width','72'],
                     'rich json --batch a.json b.json --export-html rendered/document.html --report json')
        assert sorted(p.name for p in dest.glob('*.html'))==['a.html','b.html']
        for f in dest.glob('*.html'):
            recordings['exports/'+f.name]=f.read_bytes()
    encode(modes,'release-image-modes');encode(fits,'release-image-fit');encode(live+batch,'release-workflows')
    previews=[modes[i].resize((880,528)) for i in range(0,len(modes),FPS*2)]
    previews[0].save(OUT/'release-image-modes.gif',save_all=True,append_images=previews[1:],duration=2000,loop=0)
    with zipfile.ZipFile(EVIDENCE/'recordings.zip','w',compression=zipfile.ZIP_DEFLATED) as archive:
        for name,data in recordings.items():archive.writestr(name,data)
    source=hashlib.sha256()
    for f in sorted([*ROOT.glob('crates/**/*.rs'),*ROOT.glob('**/Cargo.toml'),ROOT/'Cargo.lock']):
        if 'target' in f.parts:continue
        source.update(str(f.relative_to(ROOT)).encode()+b'\0'+f.read_bytes())
    manifest={'version':subprocess.check_output([str(binary),'--no-config','--version'],text=True).strip(),
              'binary_path':str(binary.relative_to(ROOT)) if binary.is_relative_to(ROOT) else str(binary),
              'binary_sha256':hashlib.sha256(binary.read_bytes()).hexdigest(),'source_sha256':source.hexdigest(),
              'terminal':{'width':WIDTH,'height':HEIGHT,'TERM':'xterm-256color','COLORTERM':'truecolor','RICH_SIXEL':'0'},
              'presentation':'Still captures held 2 seconds; watch sampled at 10fps, original event times. Captions added. No audio.',
              'cases':results}
    (EVIDENCE/'results.json').write_text(json.dumps(manifest,indent=2)+'\n')
    print(json.dumps({'cases':len(results),'version':manifest['version'],'source_sha256':manifest['source_sha256']},indent=2))


if __name__=='__main__':main()
