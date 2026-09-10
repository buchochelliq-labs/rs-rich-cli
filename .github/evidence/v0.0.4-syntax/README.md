# 0.0.4 syntax verification

The unchanged Syntect grammars, themes and fancy-regex engine remain in use.
A per-render cache stores successful parse operations only when the complete
ParseState is unchanged by the line. Hits require exact text and state equality;
every line still advances the live HighlightIterator. First-line rules,
state-changing lines and parse errors retain the normal path. The cache keeps
the first 64 eligible distinct lines (at most 4096 bytes / 256 operations each),
with no eviction. Opaque parser states can retain captures, so this is an entry
limit, not a strict byte bound. No rendered styles or cross-render results cache.

## Measurements

`before.json` and `after.json` use the same optimized profile and Linux host,
width 100, no color, redirected stdout, one warmup and five measured subprocess
runs. Input/output hashes match for all five cases. Before is published 0.0.3;
after is f255e70 plus the cache. Real-source fixtures were main.rs and text.rs
from f255e70; reproduce those bytes when comparing the recorded hashes.

| Case | Before median ms | After median ms |
|---|---:|---:|
| startup | 5.2 | 3.8 |
| repetitive Rust 46 KB | 184.8 | 63.3 |
| repetitive Rust 199 KB | 697.7 | 159.4 |
| real CLI source | 431.1 | 435.2 |
| real Text source | 249.7 | 249.5 |

The repetitive 199 KB corpus exceeds the 20% target; ordinary source controls
are essentially unchanged. Do not generalize this to all source files. Startup
ranges and all individual samples are in JSON; small differences are noise.

```bash
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
probe against the same optimized rich rlib as the CLI, for example:

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
across every bundled theme, plus the cache entry limit and an oversized line.

```bash
python3 .github/evidence/v0.0.4-syntax/compare.py \
  --baseline /path/to/0.0.3/rich --binary target/release/rich \
  --output comparisons.json
rich --syntax .github/evidence/v0.0.4-syntax/demo.rs --width 68 \
  --export-svg .github/evidence/v0.0.4-syntax/demo.svg
```

`demo.svg` and `stdout.txt` are actual CLI output. Independent sub-agent review
found no correctness or API-boundary blocker; comments and workload caveats
were tightened following review. Syntect/Pygments coloring divergence remains.
