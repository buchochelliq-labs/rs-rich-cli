import unittest
from pathlib import Path
import sys

sys.path.insert(0, str(Path(__file__).parent))
import snapshot_cli


class SnapshotCliTests(unittest.TestCase):
    def test_environment_pins_terminal_inputs(self):
        env = snapshot_cli.environment("plain", 37)
        self.assertEqual(env["COLUMNS"], "37")
        self.assertEqual(env["LINES"], "25")
        self.assertEqual(env["TERM"], "dumb")
        self.assertEqual(env["NO_COLOR"], "1")

    def test_render_adds_width_and_normalizes_newlines(self):
        output = snapshot_cli.render(
            ["python", "-c", "import sys; print(sys.argv[1:]); print('x\\r\\n')"],
            {"name": "unit", "args": ["hello"]},
            19,
            "plain",
        )
        self.assertIn('"status": 0', output)
        self.assertIn("--width", output)
        self.assertIn("19", output)
        self.assertNotIn("\\r\\n", output)

    def test_diff_is_readable_and_named(self):
        output = snapshot_cli.diff('{"status": 0}\\n', '{"status": 1}\\n', "case")
        self.assertIn("--- case.expected", output)
        self.assertIn("+++ case.actual", output)


if __name__ == "__main__":
    unittest.main()
