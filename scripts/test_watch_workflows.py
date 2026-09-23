"""Real PTY watch regressions; run after cargo build -p rs-rich-cli.

Usage: python scripts/test_watch_workflows.py [--binary target/debug/rich]
Only the POSIX PTY tests are skipped on platforms without pseudo-terminals.

Every wait is for output (with a generous deadline), never a fixed sleep: a
burst is proven collapsed because its intermediate contents never reach the
terminal before a later sentinel frame does.
"""

import argparse
import errno
import os
from pathlib import Path
import select
import struct
import subprocess
import sys
import tempfile
import time
import unittest


ROOT = Path(__file__).resolve().parent.parent
BINARY = ROOT / "target/debug/rich"


@unittest.skipUnless(os.name == "posix", "requires POSIX pseudo-terminals")
class WatchRecoveryTests(unittest.TestCase):
    def test_atomic_replacement_invalid_json_and_missing_file_recover(self):
        self.exercise_watch(preserve_timestamp=False)

    def test_same_size_atomic_save_with_preserved_timestamp_refreshes(self):
        self.exercise_watch(preserve_timestamp=True)

    def test_same_size_in_place_save_with_preserved_timestamp_refreshes(self):
        self.exercise_watch(preserve_timestamp=True, in_place=True)

    def exercise_watch(self, preserve_timestamp, in_place=False):
        import pty

        with tempfile.TemporaryDirectory(prefix="rich-watch-release-") as directory:
            root = Path(directory)
            resource = root / "state.json"
            resource.write_text('{"state":"INITIAL_FRAME"}', encoding="utf-8")
            master, slave = pty.openpty()
            self.addCleanup(os.close, master)
            env = dict(os.environ, HOME=str(root), COLUMNS="80", TERM="xterm")
            env.pop("NO_COLOR", None)
            try:
                child = subprocess.Popen(
                    [str(BINARY), "--no-config", "--watch", "--watch-interval",
                     "0.02", "--json", "--no-color", str(resource)],
                    stdin=subprocess.DEVNULL, stdout=slave, stderr=slave,
                    cwd=root, env=env,
                )
            finally:
                os.close(slave)
            self.addCleanup(self.stop_child, child)
            pending = bytearray()

            def await_text(needle):
                deadline = time.monotonic() + 5
                expected = needle.encode()
                while expected not in pending and time.monotonic() < deadline:
                    self.assertIsNone(child.poll(), f"watch exited: {pending!r}")
                    readable, _, _ = select.select([master], [], [], 0.1)
                    if not readable:
                        continue
                    try:
                        chunk = os.read(master, 65536)
                    except OSError as error:
                        if error.errno == errno.EIO:
                            self.fail(f"watch closed PTY: {pending!r}")
                        raise
                    self.assertTrue(chunk, f"watch reached EOF: {pending!r}")
                    pending.extend(chunk)
                self.assertIn(expected, pending, f"watch did not produce {needle!r}: {pending!r}")
                if needle.endswith("_FRAME"):
                    before_frame = pending[:pending.index(expected)]
                    self.assertIn(b"\x1b[2J\x1b[H", before_frame,
                                  f"watch appended a frame without clearing and homing: {pending!r}")
                del pending[:pending.index(expected) + len(expected)]

            def replace(text, saved_stat=None):
                temporary = root / "save.tmp"
                temporary.write_text(text, encoding="utf-8")
                if saved_stat is not None:
                    os.utime(temporary, ns=(saved_stat.st_atime_ns, saved_stat.st_mtime_ns))
                temporary.replace(resource)

            await_text("INITIAL_FRAME")
            # Same-sized contents and unchanged mtime ensure content changes,
            # rather than metadata alone, trigger a refresh after atomic save.
            original_stat = resource.stat()
            if in_place:
                resource.write_text('{"state":"REPLACE_FRAME"}', encoding="utf-8")
                os.utime(resource, ns=(original_stat.st_atime_ns, original_stat.st_mtime_ns))
            else:
                replace('{"state":"REPLACE_FRAME"}', original_stat if preserve_timestamp else None)
            await_text("REPLACE_FRAME")
            replace("{ invalid JSON")
            await_text("invalid JSON")
            await_text("watch will retry")
            replace('{"state":"RECOVERED_FRAME"}')
            await_text("RECOVERED_FRAME")
            resource.unlink()
            await_text("watch will retry")
            replace('{"state":"RESTORED_FRAME"}')
            await_text("RESTORED_FRAME")
            self.assertIsNone(child.poll(), "watch must remain active after recovery")

    @staticmethod
    def stop_child(child):
        if child.poll() is None:
            child.terminate()
        try:
            child.wait(timeout=2)
        except subprocess.TimeoutExpired:
            child.kill()
            child.wait(timeout=2)


class WatchSession:
    """A `rich --watch` child on a real 24x80 PTY, read incrementally."""

    def __init__(self, test, root, args, rows=24, columns=80):
        import fcntl
        import pty
        import termios

        self.test = test
        self.master, slave = pty.openpty()
        test.addCleanup(os.close, self.master)
        fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", rows, columns, 0, 0))
        env = dict(os.environ, HOME=str(root), COLUMNS=str(columns), LINES=str(rows),
                   TERM="xterm")
        env.pop("NO_COLOR", None)
        try:
            self.child = subprocess.Popen(
                [str(BINARY), "--no-config", "--watch", "--no-color", *args],
                stdin=subprocess.DEVNULL, stdout=slave, stderr=slave, cwd=root, env=env,
            )
        finally:
            os.close(slave)
        test.addCleanup(WatchRecoveryTests.stop_child, self.child)
        self.pending = bytearray()
        self.transcript = bytearray()

    def read(self, timeout):
        readable, _, _ = select.select([self.master], [], [], timeout)
        if not readable:
            return False
        try:
            chunk = os.read(self.master, 65536)
        except OSError as error:
            if error.errno == errno.EIO:
                return None
            raise
        if not chunk:
            return None
        self.pending.extend(chunk)
        self.transcript.extend(chunk)
        return True

    def await_text(self, needle, deadline=10):
        """Consume output through `needle`; return everything consumed."""
        expected = needle.encode()
        end = time.monotonic() + deadline
        while expected not in self.pending and time.monotonic() < end:
            self.test.assertIsNone(self.child.poll(), f"watch exited: {bytes(self.pending)!r}")
            if self.read(0.1) is None:
                self.test.fail(f"watch closed PTY: {bytes(self.pending)!r}")
        self.test.assertIn(expected, self.pending,
                           f"watch did not produce {needle!r}: {bytes(self.pending)!r}")
        cut = self.pending.index(expected) + len(expected)
        consumed = bytes(self.pending[:cut])
        del self.pending[:cut]
        return consumed

    def await_exit(self, deadline=10):
        end = time.monotonic() + deadline
        while self.child.poll() is None and time.monotonic() < end:
            self.read(0.1)
        while self.read(0.05):
            pass
        self.test.assertIsNotNone(self.child.poll(), "watch did not exit")
        return self.child.returncode


def save_atomically(path, text):
    temporary = path.with_name(path.name + ".tmp")
    temporary.write_text(text, encoding="utf-8")
    temporary.replace(path)


@unittest.skipUnless(os.name == "posix", "requires POSIX pseudo-terminals")
class MultiFileWatchTests(unittest.TestCase):
    def workspace(self):
        directory = tempfile.TemporaryDirectory(prefix="rich-watch-multi-")
        self.addCleanup(directory.cleanup)
        root = Path(directory.name)
        first, second = root / "first.json", root / "second.json"
        first.write_text('{"first":"FIRST_ONE"}', encoding="utf-8")
        second.write_text('{"second":"SECOND_ONE"}', encoding="utf-8")
        return root, first, second

    def start(self, root, *args):
        session = WatchSession(self, root, list(args))
        session.await_text("FIRST_ONE")
        session.await_text("SECOND_ONE")
        return session

    def test_single_file_burst_renders_once_per_debounce_window(self):
        root, first, _ = self.workspace()
        session = WatchSession(self, root, ["--watch-debounce", "1", str(first)])
        session.await_text("FIRST_ONE")
        for step in range(1, 6):
            first.write_text(f'{{"first":"BURST_{step}"}}', encoding="utf-8")
        burst = session.await_text("BURST_5")
        save_atomically(first, '{"first":"SENTINEL"}')
        sentinel = session.await_text("SENTINEL")
        for step in range(1, 5):
            self.assertNotIn(f"BURST_{step}".encode(), burst + sentinel)
        # Every single-file frame starts by clearing and homing the viewport.
        self.assertEqual(burst.count(b"\x1b[2J\x1b[H"), 1, burst)
        self.assertEqual(sentinel.count(b"\x1b[2J\x1b[H"), 1, sentinel)

    def test_multi_file_burst_collapses_to_one_render(self):
        root, first, second = self.workspace()
        session = self.start(root, "--watch-debounce", "1", str(first), str(second))
        for step in range(1, 6):
            first.write_text(f'{{"first":"BURST_{step}"}}', encoding="utf-8")
        burst = session.await_text("BURST_5")
        save_atomically(second, '{"second":"SENTINEL"}')
        burst += session.await_text("SENTINEL")
        for step in range(1, 5):
            self.assertNotIn(f"BURST_{step}".encode(), burst)

    def test_atomic_rename_over_save(self):
        root, first, second = self.workspace()
        session = self.start(root, str(first), str(second))
        save_atomically(first, '{"first":"RENAMED_OVER"}')
        session.await_text("RENAMED_OVER")
        self.assertIsNone(session.child.poll())

    def test_delete_and_recreate_shows_error_then_recovers(self):
        root, first, second = self.workspace()
        session = self.start(root, str(first), str(second))
        second.unlink()
        session.await_text("cannot read")
        session.await_text("watch will retry")
        second.write_text('{"second":"RECREATED"}', encoding="utf-8")
        session.await_text("RECREATED")
        first.write_text("{ invalid", encoding="utf-8")
        session.await_text("invalid JSON")
        first.write_text('{"first":"FIXED"}', encoding="utf-8")
        session.await_text("FIXED")
        self.assertIsNone(session.child.poll())

    def test_files_change_independently(self):
        root, first, second = self.workspace()
        session = self.start(root, str(first), str(second))
        # Same row counts: only the changed region's rows are repainted.
        save_atomically(first, '{"first":"FIRST_TWO"}')
        update = session.await_text("FIRST_TWO")
        save_atomically(second, '{"second":"SECOND_TWO"}')
        update_second = session.await_text("SECOND_TWO")
        self.assertNotIn(b"SECOND_ONE", update)
        self.assertNotIn(b"second.json", update)
        self.assertNotIn(b"FIRST_TWO", update_second)
        self.assertNotIn(b"first.json", update_second)

    def test_no_duplicate_full_renders_are_appended(self):
        sys.path.insert(0, str(ROOT / "scripts"))
        from test_live_regions_pty import Screen

        root, first, second = self.workspace()
        session = self.start(root, str(first), str(second))
        for step in range(3):
            save_atomically(first, f'{{"first":"FIRST_{step}", "grow": {list(range(step))}}}')
            session.await_text(f"FIRST_{step}")
            save_atomically(second, f'{{"second":"SECOND_{step}"}}')
            session.await_text(f"SECOND_{step}")
        screen = Screen(80, 24)
        screen.feed(bytes(session.transcript).decode())
        lines = screen.lines()
        self.assertEqual(sum("first.json" in line for line in lines), 1, lines)
        self.assertEqual(sum("second.json" in line for line in lines), 1, lines)
        self.assertEqual(sum("FIRST_2" in line for line in lines), 1, lines)
        self.assertFalse(any("FIRST_0" in line or "SECOND_1" in line for line in lines), lines)

    def test_forced_polling_detects_changes(self):
        root, first, second = self.workspace()
        session = self.start(root, "--watch-poll", "--watch-interval", "0.05",
                             str(first), str(second))
        first.write_text('{"first":"POLLED_EDIT"}', encoding="utf-8")
        session.await_text("POLLED_EDIT")
        second.unlink()
        session.await_text("cannot read")
        save_atomically(second, '{"second":"POLLED_BACK"}')
        session.await_text("POLLED_BACK")

    def test_exit_on_error_ends_the_watch_non_zero(self):
        root, first, second = self.workspace()
        session = self.start(root, "--watch-exit-on-error", str(first), str(second))
        first.write_text("{ invalid", encoding="utf-8")
        self.assertNotEqual(session.await_exit(), 0)
        self.assertIn(b"invalid JSON", session.transcript)
        self.assertIn(b"\x1b[?25h", session.transcript, "cursor must be restored")
        # The failing region must not promise a retry that exit-on-error forgoes.
        self.assertNotIn(b"watch will retry", session.transcript)

    def test_single_file_exit_on_error(self):
        root, first, _ = self.workspace()
        session = WatchSession(self, root, ["--watch-exit-on-error", str(first)])
        session.await_text("FIRST_ONE")
        first.unlink()
        self.assertNotEqual(session.await_exit(), 0)
        self.assertIn(b"cannot read", session.transcript)
        self.assertNotIn(b"watch will retry", session.transcript)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, default=BINARY)
    args, remaining = parser.parse_known_args()
    BINARY = args.binary.resolve()
    if not BINARY.is_file():
        parser.error(f"binary not found: {BINARY}; run cargo build -p rs-rich-cli first")
    unittest.main(argv=[__file__, *remaining])
