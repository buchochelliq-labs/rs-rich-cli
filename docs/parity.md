# Parity with Python rich

The point of this project is that output is **byte-identical** to Python
`rich` 15.0.0. Not "similar", not "inspired by" — the same bytes.

## How much of the port is done

*Last verified 2026-09-24 against Python `rich` 15.0.0, at the 0.0.11 release test (core 0.0.7).*

<figure class="port-status">
--8<-- "docs/assets/port-status.svg"
<figcaption>
<strong>60 of 65 upstream modules (92%) have a working implementation.</strong>
Click any bar to jump to that part of the
<a href="../PORTING/">module status table</a>.
</figcaption>
</figure>

**"Partial" is the honest majority, and it is not a synonym for unfinished.** A
module is marked partial while any part of upstream's surface is unported, even
when everything the CLI exercises is byte-identical. Eight modules are marked
complete, and only because a differential sweep or the golden fixtures say so:

| Module | Measurement |
|--------|-------------|
| `cells.rs` | 0 mismatches across **127,754 code points**, plus emoji clusters |
| `wrap.rs` | 0 mismatches across a **30,680-case** wrap matrix |
| `cell_widths.rs` | all 21 upstream Unicode tables, selectable via `UNICODE_VERSION` |
| `control.rs`, `box.rs`, `styled.rs` | full upstream surface, golden-tested |
| `progress_bar.rs` | full upstream surface, including the pulse animation, ASCII and no-colour bars, golden-tested (`progress_bar.tsv`) |
| `theme.rs` | full upstream surface, including the theme stack and theme files, golden-tested (`themes.tsv`, `theme_stack.tsv`) |

End to end, the current CLI renders **0 mismatches across 138 document cases**
(Markdown, JSON and syntax at six widths each) against Python `rich` 15.0.0.

!!! warning "What the percentage does not mean"

    92% of *modules* is not 92% of *upstream's behaviour*, and neither is a
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

## Differential fuzzing

Golden fixtures pin cases someone thought of. `scripts/diff_rich.py` generates
cases nobody thought of: markup, styled text with every overflow and justify
mode, panels, tables (column justify, `no_wrap`, min/max widths, ratios, edges,
lines, titles), rules, padding and alignment, at random widths and colour
systems. It renders each case through the pinned Python `rich` and through the
Rust `diff_render` example, then compares the bytes.

- **Pull requests** replay the small checked-in corpus
  (`scripts/fixtures/diff_rich_cases.jsonl`), so CI stays fast and every case
  there must match.
- **Nightly** (`.github/workflows/nightly-parity.yml`, `main` only) compares
  20,000 generated cases with a fresh seed. It fails loudly, printing shrunk,
  replayable corpus lines to the log and the job summary.
- **Known divergences** it has found live in
  `scripts/fixtures/diff_rich_known.jsonl`, one line per case, each naming its
  issue. 0.0.11 fixed the first eight families (#442–#449), so the file is
  empty until the nightly run finds something new.

The Python side renders each colour system in its own interpreter. rich memoises
a `Style`'s escape codes on the instance whatever the colour system, so a shared
interpreter reports mismatches that are the oracle's own. Markup uses the strict
parser on both sides, because upstream raises `MarkupError`.

### Triage a red nightly

1. Take the seed from the log (`Replay with: … --seed N`), or a shrunk line from
   the summary, and reproduce it locally in a virtualenv holding only the pinned
   `rich`:

    ```bash
    echo '<shrunk line>' > case.jsonl
    env -u NO_COLOR TERM=xterm-256color PYTHONUTF8=1 \
      python scripts/diff_rich.py --corpus case.jsonl
    ```

2. Check [Divergences](DIVERGENCES.md) and the known list. If the case belongs
   to a known family, there is nothing new; the issue it names tracks it.
3. Otherwise file a `type:bug` issue with the shrunk line, both outputs and the
   upstream code path, and add the case to `diff_rich_known.jsonl` with its
   issue number.
4. When fixing an issue, move its known cases into `diff_rich_cases.jsonl` in the
   same PR, so the pull-request corpus keeps the fix.

Useful flags: `--generate N --seed S` for a generated run, `--no-shrink`, and
`--max-shrink N` to limit how many failures are shrunk (one per case kind
first), `--write-failures PATH` to append shrunk cases as corpus JSONL, and
`--self-test-mutation` to prove the harness detects a mismatch.

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
