#!/usr/bin/env python3
"""Run a tape: drive a real terminal session and render it as docs media (#598).

A tape (`docs/tapes/*.tape`) scripts one demo, one command per line:

    Set Size 100x28            # columns x rows (default 100x28)
    Set TypingDelay 40ms       # per character typed by `Type`
    Set Timeout 15s            # default for `Wait`
    Set Title "Watching files" # caption for the gallery and the recording
    Set Env NAME value         # extra environment for the shell and `Exec`
    Write data.json '{"a": 1}' # create a file in the workspace (\\n escapes)
    Exec "sed -i s/1/2/ data.json"  # run a command outside the terminal
    Type "rich data.json"      # type into the shell, one character at a time
    Enter  Tab  Space  Backspace  Escape  Up  Down  Left  Right
    Home  End  PageUp  PageDown  Ctrl+C   # keys; an optional count repeats
    Sleep 500ms
    Wait "text"                # until the screen shows it (or /regex/)
    Screenshot name            # PNG, SVG and a text grid of the screen
    Hide / Show                # steps between them are not recorded
    Resize 80x24

The session is `bash` on a real PTY with a pinned environment: a temporary
HOME and working directory, TERM=xterm-256color, truecolor, UTF-8, UTC, no
NO_COLOR, and the directory of the binaries under test (`--bin-dir`) first on
PATH. The screen is followed with pyte. Nothing is drawn or invented: every
frame is what the program wrote.

Outputs go to `docs/media/tapes/<tape>/`: per screenshot `<name>.png`,
`<name>.svg` and `<name>.txt`, plus `<tape>.cast` (asciinema v2, with input
events), `<tape>.gif` (with a key overlay), `<tape>.mp4` when FFmpeg is
installed, and `provenance.json`.

    python3 scripts/tape.py docs/tapes/*.tape              # regenerate
    python3 scripts/tape.py --check docs/tapes/*.tape      # CI: compare grids

`--check` re-runs every tape and fails when a screenshot's text grid differs
from the committed `<name>.txt`, so docs media cannot silently go stale.
Requires the docs-media requirements (pyte, Pillow) and DejaVu Sans Mono.
"""
import argparse
import codecs
import difflib
import hashlib
import json
import os
import pty
import re
import shlex
import shutil
import signal
import struct
import subprocess
import sys
import tempfile
import threading
import time
from pathlib import Path

import fcntl
import termios

import pyte
from PIL import Image, ImageDraw, ImageFont

ROOT = Path(__file__).resolve().parent.parent
MEDIA = ROOT / "docs/media/tapes"
FONT_DIR = Path("/usr/share/fonts/truetype/dejavu")

# rich's SVG export theme (terminal_theme.rs, SVG_EXPORT_THEME), so tapes
# match the rest of the docs' screenshots.
BACKGROUND = (41, 41, 41)
FOREGROUND = (197, 200, 198)
ANSI = [
    (75, 78, 85), (204, 85, 90), (152, 168, 75), (208, 179, 68),
    (96, 138, 177), (152, 114, 159), (104, 160, 179), (197, 200, 198),
    (154, 155, 153), (255, 38, 39), (0, 130, 61), (208, 132, 66),
    (25, 132, 233), (255, 44, 122), (57, 130, 128), (253, 253, 197),
]
NAMES = ["black", "red", "green", "brown", "blue", "magenta", "cyan", "white"]
CHROME = (30, 30, 30)

KEYS = {
    "Enter": ("\r", "⏎"), "Tab": ("\t", "⇥"), "Space": (" ", "␣"),
    "Backspace": ("\x7f", "⌫"), "Escape": ("\x1b", "Esc"),
    "Up": ("\x1b[A", "↑"), "Down": ("\x1b[B", "↓"),
    "Right": ("\x1b[C", "→"), "Left": ("\x1b[D", "←"),
    "Home": ("\x1b[H", "Home"), "End": ("\x1b[F", "End"),
    "PageUp": ("\x1b[5~", "PgUp"), "PageDown": ("\x1b[6~", "PgDn"),
}


class TapeError(Exception):
    pass


def duration(text):
    match = re.fullmatch(r"(\d+(?:\.\d+)?)(ms|s)", text)
    if not match:
        raise TapeError(f"bad duration {text!r} (use 500ms or 2s)")
    value = float(match.group(1))
    return value / 1000 if match.group(2) == "ms" else value


def size(text):
    match = re.fullmatch(r"(\d+)x(\d+)", text)
    if not match:
        raise TapeError(f"bad size {text!r} (use 100x28)")
    return int(match.group(1)), int(match.group(2))


ESCAPES = {"n": "\n", "t": "\t", "\\": "\\", '"': '"', "'": "'"}


def unescape(text):
    """`Write`'s escapes (\\n, \\t, \\\\, \\", \\'); other text, Unicode included, as is."""
    return re.sub(r"\\(.)", lambda m: ESCAPES.get(m.group(1), m.group(0)), text)


def parse(path):
    """Parse a tape into (line number, command, arguments) steps."""
    steps = []
    for number, line in enumerate(path.read_text(encoding="utf-8").splitlines(), 1):
        stripped = line.strip()
        if not stripped or stripped.startswith("#"):
            continue
        # A /regex/ argument to Wait is kept whole, backslashes included.
        match = re.fullmatch(r"Wait\s+/(.*)/(?:\s+(\S+))?", stripped)
        if match:
            steps.append((number, "Wait", [re.compile(match.group(1))] +
                          ([match.group(2)] if match.group(2) else [])))
            continue
        try:
            words = shlex.split(stripped, comments=True)
        except ValueError as error:
            raise TapeError(f"{path}:{number}: {error}") from None
        steps.append((number, words[0], words[1:]))
    return steps


def colour(value, default, bold=False):
    if value == "default":
        return default
    name = value.removeprefix("bright")
    if name in NAMES:
        index = NAMES.index(name) + (8 if value.startswith("bright") or bold else 0)
        return ANSI[index]
    if re.fullmatch(r"[0-9a-fA-F]{6}", value):
        return tuple(int(value[i:i + 2], 16) for i in (0, 2, 4))
    return default


def snapshot(screen):
    """The screen as rows of (char, fg, bg, bold, italic, underline) cells."""
    rows = []
    for y in range(screen.lines):
        line = screen.buffer[y]
        row = []
        for x in range(screen.columns):
            char = line[x]
            fg = colour(char.fg, FOREGROUND, char.bold and char.fg in NAMES)
            bg = colour(char.bg, BACKGROUND)
            if char.reverse:
                fg, bg = bg, fg
            row.append((char.data, fg, bg, char.bold, char.italics, char.underscore))
        rows.append(tuple(row))
    cursor = None if screen.cursor.hidden else (screen.cursor.x, screen.cursor.y)
    return tuple(rows), cursor


def text_grid(cells):
    return "\n".join("".join(cell[0] for cell in row).rstrip() for row in cells) + "\n"


def to_ansi(cells, cursor):
    """Repaint a snapshot from scratch: used when a hidden stretch ends."""
    out = ["\x1b[0m\x1b[2J\x1b[H"]
    for y, row in enumerate(cells):
        out.append(f"\x1b[{y + 1};1H")
        for char, fg, bg, bold, italic, underline in row:
            if char == "":
                continue
            codes = ["0"]
            if fg != FOREGROUND:
                codes.append(f"38;2;{fg[0]};{fg[1]};{fg[2]}")
            if bg != BACKGROUND:
                codes.append(f"48;2;{bg[0]};{bg[1]};{bg[2]}")
            codes += ["1"] * bold + ["3"] * italic + ["4"] * underline
            out.append(f"\x1b[{';'.join(codes)}m{char}")
    out.append("\x1b[0m")
    if cursor:
        out.append(f"\x1b[{cursor[1] + 1};{cursor[0] + 1}H\x1b[?25h")
    else:
        out.append("\x1b[?25l")
    return "".join(out)


class Fonts:
    def __init__(self, size):
        def load(name):
            return ImageFont.truetype(str(FONT_DIR / name), size)
        self.regular = load("DejaVuSansMono.ttf")
        self.bold = load("DejaVuSansMono-Bold.ttf")
        self.italic = load("DejaVuSansMono-Oblique.ttf")
        self.bold_italic = load("DejaVuSansMono-BoldOblique.ttf")
        self.label = ImageFont.truetype(str(FONT_DIR / "DejaVuSans-Bold.ttf"), size)
        self.width = self.regular.getlength("M")
        self.height = round(size * 1.3)
        self.size = size

    def pick(self, bold, italic):
        return [[self.regular, self.italic], [self.bold, self.bold_italic]][bold][italic]


def render_png(cells, cursor, fonts, title="", key=None):
    """Draw a snapshot in a window frame; `key` adds the key overlay."""
    columns, lines = len(cells[0]), len(cells)
    pad, bar = round(fonts.size * 1.2), round(fonts.size * 2.2)
    width = round(columns * fonts.width) + 2 * pad
    height = lines * fonts.height + 2 * pad + bar
    image = Image.new("RGB", (width, height), CHROME)
    draw = ImageDraw.Draw(image)
    draw.rounded_rectangle([0, 0, width - 1, height - 1], radius=pad // 2,
                           fill=BACKGROUND, outline=(70, 70, 70))
    draw.rectangle([1, pad // 2, width - 2, bar], fill=CHROME)
    draw.rounded_rectangle([1, 1, width - 2, bar], radius=pad // 2, fill=CHROME)
    for index, dot in enumerate([(255, 95, 86), (255, 189, 46), (39, 201, 63)]):
        cx, cy, r = pad + index * round(fonts.size * 1.4), bar // 2, round(fonts.size * 0.4)
        draw.ellipse([cx - r, cy - r, cx + r, cy + r], fill=dot)
    if title:
        draw.text((width // 2, bar // 2), title, fill=(150, 150, 150),
                  font=fonts.label, anchor="mm")
    top = bar + pad
    for y, row in enumerate(cells):
        for x, (char, fg, bg, bold, italic, underline) in enumerate(row):
            left = pad + x * fonts.width
            y0 = top + y * fonts.height
            span = 2 if x + 1 < columns and row[x + 1][0] == "" and char else 1
            if bg != BACKGROUND:
                draw.rectangle([left, y0, left + span * fonts.width, y0 + fonts.height], fill=bg)
            if cursor == (x, y):
                draw.rectangle([left, y0, left + fonts.width, y0 + fonts.height], fill=FOREGROUND)
                fg = BACKGROUND
            if char and char != " ":
                draw.text((left, y0 + fonts.height / 2), char, fill=fg,
                          font=fonts.pick(bold, italic), anchor="lm")
            if underline:
                line = y0 + fonts.height - 2
                draw.line([left, line, left + span * fonts.width, line], fill=fg)
    if key:
        label = f" {key} "
        box = draw.textbbox((0, 0), label, font=fonts.label)
        w, h = box[2] - box[0] + fonts.size, box[3] - box[1] + fonts.size
        x1, y1 = width - pad, height - pad
        draw.rounded_rectangle([x1 - w, y1 - h, x1, y1], radius=h // 3, fill=(20, 20, 20),
                               outline=(120, 120, 120))
        draw.text((x1 - w / 2, y1 - h / 2), label, fill=(240, 240, 240),
                  font=fonts.label, anchor="mm")
    return image


def hexcolour(rgb):
    return "#%02x%02x%02x" % rgb


def render_svg(cells, cursor, title=""):
    """The same window as `render_png`, as SVG with selectable text."""
    cw, lh, size = 9.6, 20.8, 16
    columns, lines = len(cells[0]), len(cells)
    pad, bar = 19, 35
    width, height = columns * cw + 2 * pad, lines * lh + 2 * pad + bar
    out = [f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {width:.1f} {height:.1f}" '
           f'font-family="Fira Code, DejaVu Sans Mono, Menlo, monospace" font-size="{size}">',
           f'<rect width="100%" height="100%" rx="9" fill="{hexcolour(BACKGROUND)}" '
           f'stroke="#464646"/>',
           f'<path d="M0 9a9 9 0 0 1 9-9h{width - 18:.1f}a9 9 0 0 1 9 9v{bar - 9}h-{width:.1f}z" '
           f'fill="{hexcolour(CHROME)}"/>']
    for index, dot in enumerate(["#ff5f56", "#ffbd2e", "#27c93f"]):
        out.append(f'<circle cx="{pad + index * 22}" cy="{bar / 2}" r="6" fill="{dot}"/>')
    if title:
        out.append(f'<text x="{width / 2:.1f}" y="{bar / 2 + 5}" fill="#969696" '
                   f'font-family="DejaVu Sans, sans-serif" font-size="13" '
                   f'text-anchor="middle">{escape(title)}</text>')
    top = bar + pad
    for y, row in enumerate(cells):
        baseline = top + y * lh + lh * 0.75
        x = 0
        while x < columns:
            char, fg, bg, bold, italic, underline = row[x]
            end = x + 1
            while end < columns and row[end][1:] == row[x][1:]:
                end += 1
            text = "".join(cell[0] for cell in row[x:end])
            left = pad + x * cw
            if bg != BACKGROUND:
                out.append(f'<rect x="{left:.1f}" y="{top + y * lh:.1f}" '
                           f'width="{(end - x) * cw:.1f}" height="{lh}" fill="{hexcolour(bg)}"/>')
            if text.strip():
                attrs = f'fill="{hexcolour(fg)}"'
                attrs += ' font-weight="bold"' * bold + ' font-style="italic"' * italic
                attrs += ' text-decoration="underline"' * underline
                cells_wide = sum(2 if c and unicode_wide(c) else (0 if c == "" else 1)
                                 for c in text)
                out.append(f'<text x="{left:.1f}" y="{baseline:.1f}" {attrs} '
                           f'textLength="{cells_wide * cw:.1f}" lengthAdjust="spacingAndGlyphs" '
                           f'xml:space="preserve">{escape(text)}</text>')
            x = end
    if cursor:
        out.append(f'<rect x="{pad + cursor[0] * cw:.1f}" y="{top + cursor[1] * lh:.1f}" '
                   f'width="{cw}" height="{lh}" fill="{hexcolour(FOREGROUND)}" opacity="0.7"/>')
    out.append("</svg>\n")
    return "\n".join(out)


def unicode_wide(char):
    import unicodedata
    return unicodedata.east_asian_width(char[0]) in "WF"


def escape(text):
    return (text.replace("&", "&amp;").replace("<", "&lt;").replace(">", "&gt;"))


class Session:
    """bash on a PTY, followed by a pyte screen, with a recording."""

    def __init__(self, workspace, columns, rows, env):
        self.columns, self.rows = columns, rows
        self.screen = pyte.Screen(columns, rows)
        self.stream = pyte.ByteStream(self.screen)
        self.lock = threading.Lock()
        self.decoder = codecs.getincrementaldecoder("utf-8")("replace")
        self.hidden = True
        self.hidden_since = time.monotonic()
        self.hidden_total = 0.0
        self.start = time.monotonic()
        self.events = []          # (t, "o"|"i", data) for the cast
        self.frames = []          # (t, cells, cursor) for video
        self.keys = []            # (t, label) for the key overlay
        self.pid, self.fd = pty.fork()
        if self.pid == 0:
            os.chdir(workspace)
            os.execvpe("bash", ["bash", "--noprofile", "--norc", "-i"], env)
        self.resize(columns, rows)
        self.alive = True
        self.reader = threading.Thread(target=self.read, daemon=True)
        self.reader.start()

    def now(self):
        return time.monotonic() - self.start - self.hidden_total

    def resize(self, columns, rows):
        fcntl.ioctl(self.fd, termios.TIOCSWINSZ, struct.pack("HHHH", rows, columns, 0, 0))
        with self.lock:
            self.columns, self.rows = columns, rows
            self.screen.resize(rows, columns)
            if not self.hidden:
                # asciinema v2 resize event, so players follow the new size.
                self.events.append((self.now(), "r", f"{columns}x{rows}"))
        if getattr(self, "alive", False):
            os.kill(self.pid, signal.SIGWINCH)

    def read(self):
        while True:
            try:
                data = os.read(self.fd, 65536)
            except OSError:
                break
            if not data:
                break
            with self.lock:
                self.stream.feed(data)
                if not self.hidden:
                    t = self.now()
                    self.events.append((t, "o", self.decoder.decode(data)))
                    self.frame(t)
        self.alive = False

    def frame(self, t):
        cells, cursor = snapshot(self.screen)
        if self.frames and self.frames[-1][1:] == (cells, cursor):
            return
        self.frames.append((t, cells, cursor))

    def send(self, data, label=None):
        with self.lock:
            if not self.hidden:
                t = self.now()
                self.events.append((t, "i", data))
                if label:
                    self.keys.append((t, label))
        os.write(self.fd, data.encode())

    def display(self):
        with self.lock:
            return "\n".join(self.screen.display)

    def hide(self):
        with self.lock:
            if not self.hidden:
                self.hidden, self.hidden_since = True, time.monotonic()

    def show(self):
        with self.lock:
            if self.hidden:
                self.hidden_total += time.monotonic() - self.hidden_since
                self.hidden = False
                t = self.now()
                cells, cursor = snapshot(self.screen)
                self.events.append((t, "o", to_ansi(cells, cursor)))
                self.frame(t)

    def snapshot(self):
        with self.lock:
            return snapshot(self.screen)

    def close(self):
        try:
            os.killpg(self.pid, signal.SIGKILL)
        except (ProcessLookupError, PermissionError):
            try:
                os.kill(self.pid, signal.SIGKILL)
            except ProcessLookupError:
                pass
        try:
            os.waitpid(self.pid, 0)
        except ChildProcessError:
            pass
        os.close(self.fd)


def environment(workspace, bin_dir, extra):
    home = workspace / ".home"
    home.mkdir(exist_ok=True)
    inputrc = home / ".inputrc"
    inputrc.write_text("set enable-bracketed-paste off\n")
    env = {
        "PATH": f"{bin_dir}:{os.environ.get('PATH', '/usr/bin:/bin')}",
        "HOME": str(home), "XDG_CONFIG_HOME": str(home / ".config"),
        "INPUTRC": str(inputrc), "TERM": "xterm-256color", "COLORTERM": "truecolor",
        "LANG": "C.UTF-8", "LC_ALL": "C.UTF-8", "TZ": "UTC",
        "PS1": "\\[\\e[1;35m\\]❯\\[\\e[0m\\] ", "PROMPT_COMMAND": "",
        "HISTFILE": "/dev/null", "REPO": str(ROOT),
    }
    env.update(extra)
    return env


def run(tape, bin_dir):
    steps = parse(tape)
    columns, rows = 100, 28
    typing, timeout, title, extra = 0.04, 15.0, tape.stem, {}
    # `Set` lines configure the session and may appear anywhere before use;
    # size, title and environment are read up front.
    for number, command, args in steps:
        if command == "Set":
            if len(args) < 2:
                raise TapeError(f"{tape}:{number}: Set needs a name and a value")
            name, value = args[0], args[1:]
            if name == "Size":
                columns, rows = size(value[0])
            elif name == "Title":
                title = value[0]
            elif name == "Env":
                extra[value[0]] = value[1] if len(value) > 1 else ""
            elif name not in ("TypingDelay", "Timeout"):
                raise TapeError(f"{tape}:{number}: unknown setting {name}")
    workspace = Path(tempfile.mkdtemp(prefix=f"tape-{tape.stem}-"))
    env = environment(workspace, bin_dir, extra)
    session = Session(workspace, columns, rows, env)
    shots = {}

    def wait_for(needle, limit, number):
        end = time.monotonic() + limit
        while time.monotonic() < end:
            screen = session.display()
            if (needle.search(screen) if isinstance(needle, re.Pattern) else needle in screen):
                return
            if not session.alive:
                break
            time.sleep(0.02)
        shown = needle.pattern if isinstance(needle, re.Pattern) else needle
        raise TapeError(f"{tape}:{number}: timed out waiting for {shown!r}; screen:\n"
                        + session.display())

    try:
        # Start hidden on a clear screen at the prompt.
        wait_for("❯", timeout, 0)
        session.send("clear\r")
        time.sleep(0.2)
        wait_for("❯", timeout, 0)
        session.show()
        for number, command, args in steps:
            where = f"{tape}:{number}"
            if command == "Set":
                if args[0] == "TypingDelay":
                    typing = duration(args[1])
                elif args[0] == "Timeout":
                    timeout = duration(args[1])
            elif command == "Type":
                for char in " ".join(args):
                    session.send(char)
                    time.sleep(typing)
            elif command in KEYS or command.startswith("Ctrl+"):
                count = int(args[0]) if args else 1
                if command.startswith("Ctrl+"):
                    letter = command[5:].upper()
                    if len(letter) != 1 or not "@" <= letter <= "_":
                        raise TapeError(f"{where}: unknown key {command}")
                    data, label = chr(ord(letter) - 64), f"Ctrl+{letter}"
                else:
                    data, label = KEYS[command]
                for _ in range(count):
                    session.send(data, label)
                    time.sleep(max(typing, 0.12))
            elif command == "Sleep":
                time.sleep(duration(args[0]))
            elif command == "Wait":
                limit = duration(args[1]) if len(args) > 1 else timeout
                wait_for(args[0], limit, number)
            elif command == "Screenshot":
                time.sleep(0.15)  # let a repaint in flight land
                shots[args[0]] = session.snapshot()
            elif command == "Hide":
                session.hide()
            elif command == "Show":
                session.show()
            elif command == "Resize":
                session.resize(*size(args[0]))
            elif command == "Write":
                path = workspace / args[0]
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_text(unescape(args[1]), encoding="utf-8")
            elif command == "Exec":
                result = subprocess.run(args[0], shell=True, cwd=workspace, env=env,
                                        capture_output=True, text=True, timeout=60)
                if result.returncode:
                    raise TapeError(f"{where}: Exec failed ({result.returncode}): "
                                    f"{result.stderr.strip()}")
            else:
                raise TapeError(f"{where}: unknown command {command}")
        time.sleep(0.3)
        session.hide()
    finally:
        session.close()
        shutil.rmtree(workspace, ignore_errors=True)
    if not shots:
        raise TapeError(f"{tape}: no Screenshot")
    return {"title": title, "columns": columns, "rows": rows, "shots": shots,
            "events": session.events, "frames": session.frames, "keys": session.keys}


def write_cast(path, result):
    header = {"version": 2, "width": result["columns"], "height": result["rows"],
              "title": result["title"], "env": {"TERM": "xterm-256color", "SHELL": "bash"},
              # Players (and asciinema.org) use this palette, so a cast looks
              # like the screenshots taken from the same run.
              "theme": {"fg": hexcolour(FOREGROUND), "bg": hexcolour(BACKGROUND),
                        "palette": ":".join(hexcolour(c) for c in ANSI)}}
    lines = [json.dumps(header, ensure_ascii=False)]
    for t, kind, data in result["events"]:
        if data:
            lines.append(json.dumps([round(t, 3), kind, data], ensure_ascii=False))
    path.write_text("\n".join(lines) + "\n", encoding="utf-8")


def video_frames(result, fps=12, hold=2.5):
    """Frames sampled at `fps`, each with the key pressed in the last 0.8s."""
    frames, keys = result["frames"], result["keys"]
    out = []
    step = 1 / fps
    index = 0
    end = frames[-1][0] + hold
    t = frames[0][0]
    while t <= end:
        while index + 1 < len(frames) and frames[index + 1][0] <= t:
            index += 1
        recent = [label for when, label in keys if t - 0.8 <= when <= t]
        key = recent[-1] if recent else None
        _, cells, cursor = frames[index]
        # Blink the cursor at 2 Hz, as a terminal does.
        shown = cursor if int(t * 2) % 2 == 0 else None
        state = (cells, shown, key)
        if out and out[-1][0] == state:
            out[-1][1] += step
        else:
            out.append([state, step])
        t += step
    return out


def write_video(directory, stem, result, fonts):
    frames = video_frames(result)
    images = [(render_png(cells, cursor, fonts, result["title"], key), seconds)
              for (cells, cursor, key), seconds in frames]
    # After a `Resize` frames differ in size: centre each on one canvas.
    width = max(image.width for image, _ in images)
    height = max(image.height for image, _ in images)
    for index, (image, seconds) in enumerate(images):
        if image.size != (width, height):
            canvas = Image.new("RGB", (width, height), CHROME)
            canvas.paste(image, ((width - image.width) // 2, (height - image.height) // 2))
            images[index] = (canvas, seconds)
    first, rest = images[0][0], [image for image, _ in images[1:]]
    first.save(directory / f"{stem}.gif", save_all=True, append_images=rest,
               duration=[round(seconds * 1000) for _, seconds in images], loop=0,
               optimize=True)
    if not shutil.which("ffmpeg"):
        print(f"  ffmpeg not found: skipped {stem}.mp4")
        return
    with tempfile.TemporaryDirectory() as scratch:
        listing = Path(scratch) / "frames.txt"
        with listing.open("w") as handle:
            for number, (image, seconds) in enumerate(images):
                frame = Path(scratch) / f"{number:05}.png"
                image.save(frame)
                handle.write(f"file '{frame}'\nduration {seconds:.3f}\n")
            handle.write(f"file '{frame}'\n")
        subprocess.run(["ffmpeg", "-y", "-loglevel", "error", "-f", "concat", "-safe", "0",
                        "-i", str(listing), "-vf", "pad=ceil(iw/2)*2:ceil(ih/2)*2",
                        "-pix_fmt", "yuv420p", "-movflags", "+faststart",
                        str(directory / f"{stem}.mp4")], check=True)


def provenance(tape, bin_dir):
    rich = shutil.which("rich", path=str(bin_dir))
    version = subprocess.run([rich, "--version"], capture_output=True, text=True).stdout.strip() \
        if rich else None
    commit = subprocess.run(["git", "rev-parse", "HEAD"], cwd=ROOT, capture_output=True,
                            text=True).stdout.strip()
    return {"tape": str(tape.relative_to(ROOT)),
            "tape_sha256": hashlib.sha256(tape.read_bytes()).hexdigest(),
            "rich": version, "commit": commit, "pyte": pyte.__version__
            if hasattr(pyte, "__version__") else "0.8.2",
            "note": "Every frame is real output of the binary under test on a PTY."}


def main():
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("tapes", nargs="+", type=Path)
    parser.add_argument("--bin-dir", type=Path, default=ROOT / "target/debug")
    parser.add_argument("--check", action="store_true",
                        help="compare screenshot text grids with the committed ones")
    parser.add_argument("--no-video", action="store_true", help="skip the GIF and MP4")
    args = parser.parse_args()
    bin_dir = args.bin_dir.resolve()
    failures = []
    for tape in args.tapes:
        tape = tape.resolve()
        print(f"{tape.relative_to(ROOT)}")
        try:
            result = run(tape, bin_dir)
        except TapeError as error:
            failures.append(str(error))
            print(f"  FAILED: {error}")
            continue
        directory = MEDIA / tape.stem
        # Screenshots the tape no longer takes: stale media the docs may serve.
        orphans = sorted(path.stem for path in directory.glob("*.txt")
                         if path.stem not in result["shots"]) if directory.exists() else []
        if args.check:
            for name in orphans:
                failures.append(f"{tape.name}: committed screenshot {name} is no longer taken; "
                                f"regenerate to remove it")
                print(f"  {name}: ORPHANED")
            for name, (cells, _) in result["shots"].items():
                committed = directory / f"{name}.txt"
                got = text_grid(cells)
                want = committed.read_text(encoding="utf-8") if committed.exists() else ""
                if got != want:
                    diff = "".join(difflib.unified_diff(
                        want.splitlines(True), got.splitlines(True),
                        f"{committed.relative_to(ROOT)} (committed)", "this run"))
                    failures.append(f"{tape.name}: screenshot {name} differs\n{diff}")
                    print(f"  {name}: DIFFERS")
                else:
                    print(f"  {name}: ok")
            continue
        directory.mkdir(parents=True, exist_ok=True)
        for name in orphans:
            for suffix in (".txt", ".png", ".svg"):
                (directory / f"{name}{suffix}").unlink(missing_ok=True)
            print(f"  {name}: removed (no longer taken)")
        big = Fonts(28)
        for name, (cells, cursor) in result["shots"].items():
            render_png(cells, cursor, big, result["title"]).save(directory / f"{name}.png",
                                                                 optimize=True)
            (directory / f"{name}.svg").write_text(render_svg(cells, cursor, result["title"]),
                                                   encoding="utf-8")
            (directory / f"{name}.txt").write_text(text_grid(cells), encoding="utf-8")
            print(f"  {name}: png svg txt")
        write_cast(directory / f"{tape.stem}.cast", result)
        if not args.no_video:
            write_video(directory, tape.stem, result, Fonts(16))
        (directory / "provenance.json").write_text(
            json.dumps(provenance(tape, bin_dir), indent=2) + "\n", encoding="utf-8")
        print(f"  {tape.stem}.cast" + ("" if args.no_video else f" {tape.stem}.gif"))
    if failures:
        print("\n" + "\n\n".join(failures), file=sys.stderr)
        sys.exit(1)


if __name__ == "__main__":
    main()
