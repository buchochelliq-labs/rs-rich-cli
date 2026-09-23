# 0.0.4 syntax verification

The off-by-default `syntax-cache` Cargo feature enables repository-specific
parser reuse. Default builds keep Syntect's uncached `HighlightLines` path.
Grammars, themes and the fancy-regex engine are unchanged.

The cache takes at most one parser-state snapshot, after a successful first
source line of at most 4096 bytes. An oversized first line disables caching for
that render. Later parser states are never cloned, preventing large captured
heredoc openers from being copied into cache entries. Entries store operations
only: at most 64 distinct lines, each at most 4096 bytes and 256 operations.
Admission requires complete state equality with the reference both before and
after parsing; hits require the same reference state and exact line text.
Every line advances the live `HighlightIterator`. Parse errors keep the normal
path. There is no eviction, rendered-style cache or cross-render cache.

## Measurements

`before.json` and `after.json` use the same optimized profile and Linux host,
width 100, no color, redirected stdout, one warmup and five measured subprocess
runs. Input/output hashes match for all five cases. Before is published 0.0.3;
after is f5ac2fb plus the opt-in cache review fixes. Real-source fixtures were
main.rs and text.rs from f5ac2fb; reproduce those bytes when comparing the recorded hashes.

| Case | Before median ms | After median ms |
|---|---:|---:|
| startup | 4.2 | 3.7 |
| repetitive Rust 46 KB | 185.1 | 72.3 |
| repetitive Rust 199 KB | 629.9 | 159.3 |
| real CLI source | 445.0 | 442.0 |
| real Text source | 253.8 | 256.9 |

The repetitive 199 KB corpus improves by 75% and exceeds the 20% target; ordinary source controls
are essentially unchanged. Do not generalize this to all source files. Startup
ranges and all individual samples are in JSON; small differences are noise.

```bash
cargo build -p rs-rich-cli --release --features syntax-cache
python3 scripts/bench_v004.py --binary /path/to/rich --revision REV \
  --output results.json --case startup --case syntax-46000 \
  --case syntax-199000 --case syntax-real-cli --case syntax-real-text
```

Temporary stage timers around setup, highlight_line, token conversion and word
wrapping identified parsing/highlighting as the dominant cost. Two diagnostic
runs (under varying build/host load, not acceptance benchmarks) measured:

| Run | Setup ms | Highlight ms | Convert ms | Wrap ms | Total render ms |
|---|---:|---:|---:|---:|---:|
| 1 | 8.5 | 1442.1 | 67.9 | 63.8 | 1620.9 |
| 2 | 14.3 | 988.9 | 46.7 | 63.9 | 1139.3 |

Those timers were removed; the production code emits no profiling output.
`in_process.rs` separately measures one first render and five repeated renders
of the same Syntax object; `in-process.json` retains times and input/output
hashes. Its wall time excludes process startup. Syntax/theme sets are warmed
between renders, but the new parse cache is rebuilt each render. Compile the
probe against the same optimized, `syntax-cache`-enabled rich rlib as the CLI, for example:

```bash
rustc --edition=2024 -O .github/evidence/v0.0.4-syntax/in_process.rs \
  --extern rich=target/release/deps/librich-HASH.rlib \
  -L dependency=target/release/deps -o /tmp/syntax-in-process
/tmp/syntax-in-process source.rs captured.txt
```

## Output verification

`compare.py` checks 144 fixture/width/decoration combinations against 0.0.3,
including Rust raw strings, multiline comments, Python Unicode, Ruby heredocs,
embedded JavaScript in HTML, empty input and missing final newline. Exact
stdout, stderr, exit status, HTML and SVG are compared. Exports retain colors.
Unit tests compare the cache directly with uncached Syntect HighlightLines
across every bundled theme, plus the cache entry limit, oversized lines, and
large Ruby heredoc captures on both first and later source lines.

```bash
python3 .github/evidence/v0.0.4-syntax/compare.py \
  --baseline /path/to/0.0.3/rich --binary target/release/rich \
  --output comparisons.json
rich --syntax .github/evidence/v0.0.4-syntax/demo.rs --width 68 \
  --export-svg .github/evidence/v0.0.4-syntax/demo.svg
```

`demo.svg` and `stdout.txt` are actual CLI output. Independent sub-agent review
found no correctness or API-boundary blocker after the cache was made opt-in
and per-entry parser-state snapshots were removed. Syntect/Pygments coloring divergence remains.
