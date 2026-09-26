"""``python -m rs_rich``: the ``rich`` command line (see :mod:`rs_rich.cli`)."""

import sys

from rs_rich.cli import main

if __name__ == "__main__":
    sys.exit(main())
