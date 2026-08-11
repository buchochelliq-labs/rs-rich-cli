# Benchmarks

How this Rust `rich` CLI compares to the Python `rich-cli` it mirrors.

Reproduce with:

```bash
cargo build --release -p rs-rich-cli
python scripts/bench_cli.py --setup-venv
python scripts/bench_cli.py
```

## Results

Medians of 15 runs after 3 warmup runs, `--width 100`, stdout on a pipe.
Ranges are across three separate passes on one Windows 11 machine.

| case | Rust | Python | speedup |
|---|---|---|---|
| startup floor (one-line markdown) | 14–17 ms | 390–414 ms | **~26×** |
| markdown, 19 KB | 15–18 ms | 502–530 ms | **~30×** |
| JSON, 60 KB | 25–27 ms | 668–730 ms | **~27×** |
| `--rule` (no input file at all) | 12–15 ms | 316–356 ms | **~26×** |
| syntax highlight, 46 KB `.rs` | 283–306 ms | 1050–1184 ms | **~3.8×** |

Versions: this crate at 0.0.2 (release build) against `rich-cli` 1.8.1, which
pins **`rich` 12.6.0** — not the 15.0.0 that `UPSTREAM.toml` mirrors. That is
what `pip install rich-cli` gives you today, so it is the honest real-world
comparison, but it is not a controlled one.

## What the numbers mean

**The startup floor is the interesting one.** Rendering a one-line document
costs ~14 ms here and ~400 ms there, and most of that 400 ms is interpreter
start plus imports, paid on every invocation. That is the difference between a
tool you can put in a pipe, a shell hook, or `$PAGER`, and one you can't.

**Syntax highlighting is this port's weak spot.** At ~290 ms it is roughly 17×
the cost of the markdown path on a comparable file, which drags the advantage
from ~27× down to ~3.8×. It has two components:

| input | time |
|---|---|
| 30 B | 63 ms |
| 21.7 KB | 182 ms |
| 46 KB | 297 ms |
| 199 KB | 912 ms |

A fixed cost of roughly 50 ms above baseline (loading the syntax set) **plus a
linear ~4.3 ms/KB**. The linear term is the one that matters, and it is tracked
as a performance issue.

## Method, and why it is shaped this way

Both binaries are spawned as subprocesses with stdout on a pipe, so neither is
charged for the terminal's own drawing speed and both take their non-TTY path.
Verified symmetric: neither emits ANSI escapes when piped.

Process spawn cost is included on both sides. It is real cost a user pays per
invocation, and on Windows it is not negligible — but it does not favour either
implementation.

Two traps the harness exists to avoid, both of which produced wrong numbers
before they were caught:

1. **The two CLIs do not share short flags.** Python's `-x` is `--lexer`, which
   takes an argument; syntax mode is `--syntax`, and `--json` is capital `-J`.
   Each case therefore carries a separate argv per implementation.

2. **Python's `rich-cli` prints a usage message and exits 0 on an unknown
   flag.** The first version of this benchmark timed a 78-byte usage message
   against a 122 KB render and reported it as "1.1×". Exit status and timing
   both looked healthy. Every case is now validated — output must clear a
   per-case byte floor and must not contain usage text — *before* it is timed.

## Caveats

- **The ANSI path is not measured.** Piped output means neither side emits
  colour, so styling cost is excluded. This CLI has no `--force-terminal`, so
  forcing colour symmetrically is not currently possible.
- **Different `rich` versions** (12.6.0 vs the 15.0.0 this port mirrors).
- **Output is not byte-identical** and is not expected to be — among other
  differences, this port pads syntax lines to the full width where Python
  leaves them ragged (122 KB vs 47 KB for the same 1215 lines).
- **One machine, one OS.** No cross-platform claim is made.
- **Single passes are noisy.** One pass measured the JSON case at 135 ms
  against a 25–27 ms consensus across every other pass. Take the median of
  several passes before believing a number, and never quote a single run.
