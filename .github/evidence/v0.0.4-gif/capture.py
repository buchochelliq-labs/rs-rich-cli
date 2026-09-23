"""Capture actual Linux PTY playback and its capability matrix (standard library)."""
import argparse
import codecs
import gzip
import hashlib
import errno
import fcntl
import json
import os
from pathlib import Path
import pty
import select
import signal
import struct
import subprocess
import termios
import time

parser = argparse.ArgumentParser()
parser.add_argument('--binary', required=True)
parser.add_argument('--fixture', required=True)
args = parser.parse_args()
root = Path(__file__).resolve().parent
results = []
for name, mode, term, color, no_color, loops in [
    ('truecolor', 'blocks', 'xterm-256color', 'truecolor', False, '1'),
    ('256color', 'blocks', 'xterm-256color', '', False, '1'),
    ('16color', 'blocks', 'xterm', '', False, '1'),
    ('no-color', 'blocks', 'xterm-256color', 'truecolor', True, '1'),
    ('ascii', 'ascii', 'xterm-256color', 'truecolor', False, '1'),
    ('twice', 'blocks', 'xterm-256color', 'truecolor', False, '2'),
    ('interrupt', 'blocks', 'xterm-256color', 'truecolor', False, '0'),
]:
    master, slave = pty.openpty()
    fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack('HHHH', 24, 80, 0, 0))
    env = dict(os.environ, TERM=term, COLORTERM=color, COLUMNS='80', LINES='24')
    env.pop('NO_COLOR', None)
    if no_color:
        env['NO_COLOR'] = '1'
    command = [args.binary, '--gif', args.fixture, '--gif-mode', mode, '--width', '32', '--loop', loops]
    started = time.monotonic()
    child = subprocess.Popen(command, stdin=slave, stdout=slave, stderr=slave, env=env, start_new_session=True)
    os.close(slave)
    chunks, events = [], []
    decoder = codecs.getincrementaldecoder("utf-8")()
    interrupted = False
    try:
        while True:
            elapsed = time.monotonic() - started
            if name == 'interrupt' and elapsed > 0.8 and not interrupted:
                child.send_signal(signal.SIGINT)
                interrupted = True
            if elapsed > 15:
                os.killpg(child.pid, signal.SIGKILL)
                raise RuntimeError(f'{name}: playback timeout')
            if select.select([master], [], [], 0.05)[0]:
                try:
                    data = os.read(master, 65536)
                except OSError as error:
                    if error.errno == errno.EIO:
                        break
                    raise
                if not data:
                    break
                chunks.append(data)
                events.append([elapsed, 'o', decoder.decode(data)])
            elif child.poll() is not None:
                break
    finally:
        os.close(master)
        if child.poll() is None:
            child.kill()
        child.wait()
    output = b''.join(chunks)
    (root / f'{name}.ansi.gz').write_bytes(gzip.compress(output, mtime=0))
    cast = (json.dumps({'version': 2, 'width': 80, 'height': 24, 'title': name}) + '\n' + ''.join(json.dumps(event) + '\n' for event in events))
    (root / f'{name}.cast.gz').write_bytes(gzip.compress(cast.encode(), mtime=0))
    blocks = '▀'.encode() in output
    assert blocks == (mode == 'blocks' and not no_color), name
    assert child.returncode == (-signal.SIGINT if interrupted else 0), name
    if not interrupted:
        assert b'\x1b[?25h' in output, name
    results.append({'case': name, 'exit_code': child.returncode, 'bytes': len(output), 'sha256': hashlib.sha256(output).hexdigest(), 'seconds': time.monotonic()-started, 'blocks': blocks, 'cursor_restored': b'\x1b[?25h' in output})
(root / 'pty-results.json').write_text(json.dumps(results, indent=2) + '\n')
print(json.dumps(results, indent=2))
