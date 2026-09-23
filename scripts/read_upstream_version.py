"""Read the library oracle pin using TOML syntax, including comments and spacing."""

from pathlib import Path
import re
import tomllib


def read_version(path):
    version = tomllib.loads(Path(path).read_text(encoding="utf-8"))["rich"]["version"]
    if not isinstance(version, str) or not re.fullmatch(r"[0-9]+\.[0-9]+\.[0-9]+", version):
        raise ValueError("[rich].version must be an exact numeric release")
    return version


if __name__ == "__main__":
    print(read_version(Path(__file__).resolve().parent.parent / "UPSTREAM.toml"))
