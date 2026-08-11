# Parity with Python rich

The point of this project is that output is **byte-identical** to Python
`rich` 15.0.0. Not "similar", not "inspired by" — the same bytes.

## How much of the port is done

*Last verified 2026-08-11 against Python `rich` 15.0.0.*

<figure class="port-status">
--8<-- "docs/assets/port-status.svg"
<figcaption>
<strong>55 of 61 upstream modules (90%) have a working implementation.</strong>
Click any bar to jump to that part of the
<a href="../PORTING/">module status table</a>.
</figcaption>
</figure>

**"Partial" is the honest majority, and it is not a synonym for unfinished.** A
module is marked partial while any part of upstream's surface is unported, even
when everything the CLI exercises is byte-identical. Six modules are marked
complete only because a differential sweep says so:

| Module | Measurement |
|--------|-------------|
| `cells.rs` | 0 mismatches across **127,754 code points**, plus emoji clusters |
| `wrap.rs` | 0 mismatches across a **30,680-case** wrap matrix |
| `cell_widths.rs` | all 21 upstream Unicode tables, selectable via `UNICODE_VERSION` |
| `control.rs`, `box.rs`, `styled.rs` | full upstream surface, golden-tested |

End to end, the current CLI renders **0 mismatches across 138 document cases**
(Markdown, JSON and syntax at six widths each) against Python `rich` 15.0.0.

!!! warning "What the percentage does not mean"

    90% of *modules* is not 90% of *upstream's behaviour*, and neither is a
    promise about your document. It means most modules have a working
    implementation; the honest per-area detail is in
    [Module status](PORTING.md), and the known gaps are below.

## How parity is enforced

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

!!! danger "Never install `rich-cli` beside `rich`"

    `rich-cli` 1.8.1 requires `rich<13`, so installing it **downgrades** `rich`
    from 15.0.0 to 12.6.0 — the library this port measures itself against. `pip`
    prints no error, the fixtures still run, and every comparison afterwards is
    silently against the wrong upstream. Keep the CLI oracle in its own
    virtualenv, and check the version before trusting a parity result:

    ```bash
    python -c "from importlib.metadata import version; print(version('rich'))"
    ```

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

See [Known issues](known-issues.md) for the ones you might actually hit.

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
