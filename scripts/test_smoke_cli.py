#!/usr/bin/env python3
"""Tests for scripts/smoke_cli.py.

The fixture and case checks always run. The end-to-end run uses the built
binary (``$CARGO_TARGET_DIR/debug/rich``, else ``target/debug/rich``, or
``RICH_BINARY``) and is skipped when there is none.
"""

import json
import os
import subprocess
import sys
import tempfile
import unittest
import zlib
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
import smoke_cli  # noqa: E402

BINARY = Path(os.environ.get("RICH_BINARY", smoke_cli.DEFAULT_BINARY))
SCRIPT = Path(__file__).with_name("smoke_cli.py")


class FixtureTests(unittest.TestCase):
    def test_fixtures_are_written(self):
        with tempfile.TemporaryDirectory() as tmp:
            smoke_cli.write_fixtures(Path(tmp))
            for name in ("notes.md", "team.csv", "analysis.ipynb", "before.png", "ball.gif",
                         "docs/intro.md", "rich.toml"):
                self.assertTrue((Path(tmp) / name).is_file(), name)
            json.loads((Path(tmp) / "analysis.ipynb").read_text(encoding="utf-8"))

    def test_png_is_well_formed(self):
        data = smoke_cli.png(3, 2, lambda x, y: (x, y, 0, 255))
        self.assertTrue(data.startswith(b"\x89PNG\r\n\x1a\n"))
        # IDAT holds one filter byte plus 3 RGBA pixels per row.
        start = data.index(b"IDAT") + 4
        length = int.from_bytes(data[start - 8:start - 4], "big")
        self.assertEqual(len(zlib.decompress(data[start:start + length])), 2 * (1 + 3 * 4))

    def test_gif_has_one_image_per_frame(self):
        data = smoke_cli.gif(4, 1, [(0, 0, 0), (255, 255, 255)], [[0, 1, 0, 1], [1, 0, 1, 0]], 10)
        self.assertTrue(data.startswith(b"GIF89a"))
        self.assertTrue(data.endswith(b"\x3B"))
        self.assertEqual(data.count(b"\x21\xF9\x04"), 2)


class CaseTests(unittest.TestCase):
    def test_case_names_are_unique(self):
        names = [case.name for case in smoke_cli.CASES]
        self.assertEqual(len(names), len(set(names)))

    def test_screenshot_names_are_unique(self):
        shots = [case.shot for case in smoke_cli.CASES if case.shot]
        self.assertEqual(len(shots), len(set(shots)))

    def test_list_filters_cases(self):
        process = subprocess.run([sys.executable, str(SCRIPT), "--list", "-k", "inspect"],
                                 capture_output=True, text=True, check=True)
        lines = process.stdout.splitlines()
        self.assertTrue(lines)
        self.assertTrue(all(line.startswith("inspect") for line in lines), lines)


@unittest.skipUnless(BINARY.is_file(), f"no binary at {BINARY}; run cargo build -p rs-rich-cli")
class EndToEndTests(unittest.TestCase):
    def test_every_case_passes(self):
        process = subprocess.run([sys.executable, str(SCRIPT), "--binary", str(BINARY), "--json"],
                                 capture_output=True, text=True, timeout=600)
        report = json.loads(process.stdout)
        failures = [r for r in report["results"] if r["status"] == "fail"]
        self.assertEqual(failures, [], json.dumps(failures, indent=2))
        self.assertEqual(process.returncode, 0, process.stderr)

    def test_screenshots_are_written(self):
        with tempfile.TemporaryDirectory() as tmp:
            subprocess.run([sys.executable, str(SCRIPT), "--binary", str(BINARY),
                            "--screenshots", tmp, "-k", "csv"],
                           capture_output=True, text=True, check=True, timeout=120)
            svg = Path(tmp) / "cli_csv.svg"
            self.assertTrue(svg.read_text(encoding="utf-8").startswith("<svg"))


if __name__ == "__main__":
    unittest.main(verbosity=2)
