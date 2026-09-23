#!/usr/bin/env python3
"""POSIX terminal regressions for CLI input hints and NO_COLOR handling."""
import argparse
import errno
import os
import pty
import select
import subprocess
import time
import unittest
from pathlib import Path

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--binary', type=Path, required=True)
args = parser.parse_args()
BINARY = str(args.binary.resolve())


class TerminalTests(unittest.TestCase):
    def terminal_output(self, no_color):
        master, slave = pty.openpty()
        env = dict(os.environ, TERM='xterm-256color', COLUMNS='80')
        env.pop('NO_COLOR', None)
        if no_color is not None:
            env['NO_COLOR'] = no_color
        child = subprocess.Popen([BINARY, '-p', '[red]color[/]'],
                                 stdin=subprocess.DEVNULL, stdout=slave,
                                 stderr=subprocess.PIPE, env=env)
        os.close(slave)
        output = bytearray()
        deadline = time.monotonic() + 10
        try:
            while time.monotonic() < deadline:
                if not select.select([master], [], [], 0.1)[0]:
                    continue
                try:
                    chunk = os.read(master, 65536)
                except OSError as error:
                    if error.errno == errno.EIO:
                        break
                    raise
                if not chunk:
                    break
                output.extend(chunk)
            else:
                self.fail('terminal render timed out')
            _, stderr = child.communicate(timeout=5)
            self.assertEqual(child.returncode, 0, stderr)
            self.assertEqual(stderr, b'')
            return bytes(output)
        finally:
            os.close(master)
            if child.poll() is None:
                child.kill()
                child.communicate()

    def test_no_color_unset_or_empty_preserves_terminal_color(self):
        for value in (None, ''):
            with self.subTest(value=value):
                self.assertIn(b'\x1b[31m', self.terminal_output(value))

    def test_nonempty_no_color_disables_terminal_color(self):
        output = self.terminal_output('1')
        self.assertNotIn(b'\x1b', output)
        self.assertEqual(output.replace(b'\r\n', b'\n'), b'color\n')

    def test_interactive_stdin_hint_and_eof(self):
        for resource in ([], ['-']):
            with self.subTest(resource=resource):
                master, slave = pty.openpty()
                child = subprocess.Popen([BINARY, '--markdown', *resource],
                                         stdin=slave, stdout=subprocess.PIPE,
                                         stderr=subprocess.PIPE)
                os.close(slave)
                try:
                    os.write(master, b'hello\n\x04')
                    stdout, stderr = child.communicate(timeout=10)
                    self.assertEqual(child.returncode, 0, stderr)
                    self.assertIn(b'hello', stdout)
                    self.assertEqual(stderr, b'rich: reading stdin; finish input with Ctrl-D\n')
                finally:
                    os.close(master)
                    if child.poll() is None:
                        child.kill()
                        child.communicate()


if __name__ == '__main__':
    unittest.main(argv=[__file__], verbosity=2)
