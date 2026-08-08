# Parity with Python rich

The point of this project is that output is **byte-identical** to Python
`rich` 15.0.0. Not "similar", not "inspired by" — the same bytes.

## How that is enforced

Fixtures are captured from the real Python library and asserted in CI:

```bash
pip install "rich==15.0.0"
python scripts/capture_golden.py       # writes crates/rich/tests/golden/*.tsv
cargo test -p rs-rich --test golden    # asserts byte equality
```

The capture script renders each case through Python `rich` with a pinned console
(`force_terminal=True, color_system="truecolor", legacy_windows=False,
safe_box=False`) and records the exact escape sequences. The Rust tests rebuild
the same case and compare bytes.

CI regenerates the fixtures from upstream on every run and fails if they drift,
so the parity claim is checked continuously rather than asserted once.

## What this buys you

If you know Python `rich`, you already know this library — the same markup, the
same style syntax, the same box styles, the same colour downgrade behaviour on a
16-colour terminal.

It also means upstream's quirks are reproduced deliberately. An unknown markup
tag renders as a no-op rather than an error, because that is what upstream does.

## Where it stops

Three kinds of gap, kept separate on purpose:

<div class="grid cards" markdown>

- **Not ported yet**

    Listed per module in [Module status](PORTING.md). Notably: Windows legacy
    console, Jupyter integration, and Python-object `inspect`.

- **Deliberately different**

    Documented with reasons in [Divergences](DIVERGENCES.md). The big one:
    syntax highlighting uses `syntect`, not Pygments, so highlighted code is
    *not* byte-identical.

- **Impossible**

    Some things cannot match. Upstream puts a random `id=` in OSC 8 hyperlinks;
    reproducing it would mean reproducing Python's RNG.

</div>

## Reporting a parity bug

The most useful bug report shows both sides:

```python
from rich.console import Console
c = Console(force_terminal=True, color_system="truecolor",
            legacy_windows=False, safe_box=False, no_color=False, width=40)
c.print("[bold]your case here[/]")
```

Paste the **exact bytes** (`repr()` in Python, `{:?}` in Rust) rather than a
screenshot — an escape sequence is invisible otherwise. Check
[Divergences](DIVERGENCES.md) first in case it is intentional.
