#!/usr/bin/env python3
"""Real PTY I/O with a strict visible-screen model for the Live example."""
import errno
import fcntl
import os
from pathlib import Path
import pty
import re
import select
import struct
import subprocess
import termios
import time

ROOT = Path(__file__).resolve().parents[1]
class Screen:
    def __init__(self, w, h):
        self.w, self.h, self.x, self.y = w, h, 0, h-1
        self.rows = [[' ']*w for _ in range(h)]
        self.visible = True
    def resize(self, w, h):
        w, h = max(w, 1), max(h, 1)
        self.rows = [(row[:w]+[' ']*w)[:w] for row in self.rows[-h:]]
        self.rows += [[' ']*w for _ in range(h-len(self.rows))]
        self.w, self.h = w, h
        self.x, self.y = min(self.x,w-1), min(self.y,h-1)
    def feed(self, text):
        while text:
            if text.startswith('\x1b'):
                m = re.match(r'\x1b\[([0-9;?]*)([A-Za-z])', text)
                assert m, repr(text)
                args, command = m.groups(); text = text[m.end():]
                if command == 'A': self.y = max(0,self.y-int(args))
                elif command == 'B': self.y = min(self.h-1,self.y+int(args))
                elif command == 'K':
                    assert args == '2'; self.rows[self.y] = [' ']*self.w
                elif command in 'hl':
                    assert args == '?25'; self.visible = command == 'h'
                elif command != 'm': raise AssertionError((args,command))
                continue
            c, text = text[0], text[1:]
            if c == '\r': self.x = 0
            elif c == '\n':
                self.y += 1
                if self.y == self.h:
                    self.rows.pop(0); self.rows.append([' ']*self.w); self.y -= 1
            else:
                assert self.x < self.w, 'unexpected terminal auto-wrap'
                self.rows[self.y][self.x] = c; self.x += 1
    def lines(self): return [''.join(row).rstrip() for row in self.rows]

def main():
    subprocess.run(['cargo','build','-q','-p','rs-rich-ext','--example','live_regions'],cwd=ROOT,check=True)
    master, slave = pty.openpty()
    fcntl.ioctl(slave,termios.TIOCSWINSZ,struct.pack('HHHH',24,80,0,0))
    child = subprocess.Popen([str(ROOT/'target/debug/examples/live_regions'),'--script'],stdin=subprocess.PIPE,stdout=slave,stderr=subprocess.PIPE)
    os.close(slave)
    screen = Screen(80,24)
    def drain(ready=True):
        data=bytearray(); deadline=time.monotonic()+10
        while ready:
            assert time.monotonic()<deadline,'Live command timed out'
            r,_,_=select.select([master,child.stderr],[],[],0.1)
            if master in r:
                try: data.extend(os.read(master,65536))
                except OSError as e:
                    if e.errno!=errno.EIO: raise
            if child.stderr in r:
                assert child.stderr.readline()==b'ready\n';break
        while select.select([master],[],[],0.05)[0]:
            try: chunk=os.read(master,65536)
            except OSError as e:
                if e.errno==errno.EIO:break
                raise
            if not chunk:break
            data.extend(chunk)
        screen.feed(data.decode())
    try:
        drain(); assert not screen.visible
        for w,h in [(20,6),(0,1),(80,24)]:
            fcntl.ioctl(master,termios.TIOCSWINSZ,struct.pack('HHHH',max(h,1),max(w,1),0,0))
            screen.resize(w,h)
            child.stdin.write(f'resize {w} {h}\n'.encode());child.stdin.flush();drain()
            assert screen.visible == (w<2 or h<2)
        child.stdin.write(b'log after grow\n');child.stdin.flush();drain()
        assert 'after grow' in screen.lines()
        child.stdin.write(b'quit\n');child.stdin.flush();child.wait(timeout=10);drain(False)
        assert child.returncode==0 and screen.visible
        assert 'after grow' in screen.lines()
        assert not any('pending' in line for line in screen.lines())
        print('Live PTY: resize, visible log retention and cursor cleanup passed')
    finally:
        os.close(master)
        if child.poll() is None: child.kill();child.wait()
if __name__=='__main__': main()
