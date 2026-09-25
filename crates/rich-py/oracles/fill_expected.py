"""Fill a test file's ``EXPECTED`` table from the Rust oracle's output.

    python crates/rich-py/oracles/fill_expected.py crates/rich-py/tests/test_art.py

runs the oracle (built first, see README.md) on the file's ``CASES`` and
rewrites the block between ``# fmt: off`` and ``# fmt: on``.
"""

import importlib.util
import json
import os
import pprint
import re
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[3]
ORACLE = Path(os.environ.get("ART_ORACLE", ROOT / "target" / "rich-py" / "release" / "art-oracle"))


def main(path: str) -> None:
    spec = importlib.util.spec_from_file_location("cases", path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    out = subprocess.run(
        [str(ORACLE)], input=json.dumps(module.CASES), capture_output=True, text=True, check=True
    ).stdout
    results = {name: module.digest(value) for name, value in json.loads(out).items()}
    text = Path(path).read_text(encoding="utf-8")
    body = "EXPECTED: dict = " + pprint.pformat(results, width=110, sort_dicts=True) + "\n"
    text = re.sub(
        r"(# fmt: off\n).*?(# fmt: on)", lambda m: m.group(1) + body + m.group(2), text, flags=re.S
    )
    Path(path).write_text(text, encoding="utf-8")
    print(len(results), "cases")


if __name__ == "__main__":
    main(sys.argv[1])
