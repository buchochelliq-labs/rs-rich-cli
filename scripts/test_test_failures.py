"""Tests for scripts/test_failures.py."""

import os
import sys
import unittest

sys.path.insert(0, os.path.dirname(__file__))

import test_failures  # noqa: E402

LOG = """\
2026-10-18T17:48:18.0000000Z      Running tests/banner_shell.rs (target/debug/deps/banner_shell-1a2b)
2026-10-18T17:48:18.0000000Z
running 2 tests
test shell::ok_one ... ok
test shell::bad_one ... FAILED

failures:

---- shell::bad_one stdout ----

thread 'shell::bad_one' panicked at crates/rich-cli/tests/banner_shell.rs:10:5:
\x1b[1massertion failed\x1b[0m: x
    left: 1
   right: 2


failures:
    shell::bad_one

test result: FAILED. 1 passed; 1 failed; 0 ignored
\x1b[1m\x1b[91merror\x1b[0m: test failed, to rerun pass `-p rs-rich-cli --test banner_shell`
     Running tests/later.rs (target/debug/deps/later-3c4d)
test later::fine ... ok
test result: ok. 1 passed; 0 failed
\x1b[1m\x1b[91merror\x1b[0m: 1 target failed:
    `-p rs-rich-cli --test banner_shell`
"""


class FailuresTest(unittest.TestCase):
    def test_finds_each_failing_test_and_its_target(self):
        found, summary = test_failures.failures(LOG.splitlines(True))
        self.assertEqual(len(found), 1)
        target, name, output = found[0]
        self.assertEqual(name, "shell::bad_one")
        self.assertIn("banner_shell.rs", target)
        text = "\n".join(output)
        self.assertIn("panicked at crates/rich-cli/tests/banner_shell.rs:10:5", text)
        self.assertIn("assertion failed: x", text)
        self.assertNotIn("\x1b", text)
        self.assertIn("error: 1 target failed:", summary)
        self.assertIn("`-p rs-rich-cli --test banner_shell`", summary)

    def test_reports_markdown(self):
        found, summary = test_failures.failures(LOG.splitlines(True))
        report = test_failures.report(found, summary)
        self.assertTrue(report.startswith("## Failing tests"))
        self.assertIn("### `shell::bad_one`", report)
        self.assertNotIn("later::fine", report)

    def test_a_clean_log_reports_nothing(self):
        found, summary = test_failures.failures(["test a ... ok\n", "test result: ok.\n"])
        self.assertEqual(test_failures.report(found, summary), "No failing tests in the log.\n")

    def test_long_output_is_cut(self):
        lines = ["---- t stdout ----\n"] + ["x" * 100 + "\n"] * 200 + ["failures:\n"]
        found, summary = test_failures.failures(lines)
        report = test_failures.report(found, summary)
        self.assertIn("more characters", report)
        self.assertLess(len(report), test_failures.PER_TEST + 500)


if __name__ == "__main__":
    unittest.main()
