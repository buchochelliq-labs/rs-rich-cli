#!/usr/bin/env python3
"""Exercise guided-tour playback and interruption in a real POSIX terminal."""
import argparse
import codecs
import errno
import fcntl
import json
import os
import pty
import select
import signal
import struct
import subprocess
import tempfile
import termios
import time
from pathlib import Path


def capture(binary, stop_at=None, delay='0'):
    with tempfile.TemporaryDirectory() as directory:
        master, slave = pty.openpty()
        fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack('HHHH', 28, 88, 0, 0))
        env = dict(os.environ, TERM='xterm-256color', COLORTERM='truecolor', RICH_SIXEL='1', COLUMNS='88', LINES='28', TMPDIR=directory)
        env.pop('NO_COLOR', None)
        process = subprocess.Popen([str(binary), '--demo', '--demo-delay', delay],
                                   stdin=subprocess.DEVNULL, stdout=slave, stderr=slave, env=env)
        os.close(slave)
        raw, events = bytearray(), []
        decoder = codecs.getincrementaldecoder("utf-8")()
        start = time.monotonic()
        sent = False
        try:
            while time.monotonic() - start < 20 + 60 * float(delay):
                if select.select([master], [], [], .05)[0]:
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
                    if stop_at and stop_at in raw and not sent:
                        # During watch, wait until its child has rendered.
                        process.send_signal(signal.SIGINT)
                        sent = True
                elif process.poll() is not None:
                    break
            else:
                raise AssertionError('tour exceeded bounded terminal-test timeout')
        finally:
            os.close(master)
            if process.poll() is None:
                process.kill()
            process.wait()
        assert process.returncode == (130 if stop_at else 0), (process.returncode, raw[-2000:])
        if stop_at:
            assert sent and b'\x1b[?25h\x1b[0m' in raw, 'interrupt must restore terminal state'
        else:
            assert b'\x1bP' not in raw, 'tour must stay portable even when Sixel is advertised'
            assert b'Tour complete' in raw
            assert b'live_update' in raw
        assert not list(Path(directory).iterdir()), 'demo must clean temporary exports on completion/interruption'
        return events, bytes(raw)


def redirected_interrupt(binary):
    with tempfile.TemporaryDirectory() as directory:
        env = dict(os.environ, TMPDIR=directory)
        process = subprocess.Popen([str(binary), '--demo'], env=env,
                                   stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        output = bytearray()
        start = time.monotonic()
        try:
            while b'markup' not in output:
                assert time.monotonic() - start < 10, 'redirected tour made no progress'
                if select.select([process.stdout], [], [], .1)[0]:
                    data = os.read(process.stdout.fileno(), 4096)
                    assert data, 'tour ended before interrupt point'
                    output.extend(data)
            process.send_signal(signal.SIGINT)
            remainder, stderr = process.communicate(timeout=10)
            output.extend(remainder)
            assert process.returncode == 130, (process.returncode, stderr)
            assert b'\x1b' not in output, 'pipe cleanup must not emit terminal controls'
            assert not list(Path(directory).iterdir()), 'interrupted pipe left temporary exports'
        finally:
            if process.poll() is None:
                process.kill()
            process.wait()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--record', type=Path)
    args = parser.parse_args()
    binary = args.binary.resolve()
    events, raw = capture(binary)
    capture(binary, b'live_update')
    capture(binary, b'\x1b[?25l')
    # A nonzero pause must really hold before the first rendered section.
    start = time.monotonic()
    capture(binary, b'markup', delay='.15')
    assert time.monotonic() - start >= .25
    if args.record:
        args.record.write_text(json.dumps({'version': 2, 'width': 88, 'height': 28}) + '\n' + ''.join(json.dumps(event)+'\n' for event in events))
        args.record.with_suffix('.ansi').write_bytes(raw)
    redirected_interrupt(binary)
    print('5 demo process checks passed: portable playback, watch/GIF/pipe interruption, pacing')


if __name__ == '__main__':
    main()
