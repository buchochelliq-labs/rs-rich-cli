"""Real PTY watch recovery regression; run after cargo build -p rs-rich-cli.

Usage: python scripts/test_watch_workflows.py [--binary target/debug/rich]
Only the POSIX PTY test is skipped on platforms without pseudo-terminals.
"""

import argparse
import errno
import os
from pathlib import Path
import select
import subprocess
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


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, default=BINARY)
    args, remaining = parser.parse_known_args()
    BINARY = args.binary.resolve()
    if not BINARY.is_file():
        parser.error(f"binary not found: {BINARY}; run cargo build -p rs-rich-cli first")
    unittest.main(argv=[__file__, *remaining])
