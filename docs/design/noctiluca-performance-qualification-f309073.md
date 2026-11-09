# Noctiluca performance source qualification — f309073

**Result: not release-qualified.** The three iTerm2-compatible runs all miss at least one unchanged application gate. All three Kitty and all three ANSI runs pass. This is a new qualification of the PR source; it does not replace or rewrite the earlier `cbc7d93` results.

## Method and provenance

- Source: `f3090737be9d569fd29b0260ba18148b65caa57b`; PR head `c3d667c4faf22317d5113b67a9e4a41cd47ddc2b` contains documentation/evidence-only commits after that source.
- Rust 1.90.0 optimized release test harness, locked/offline graph. Test executable SHA-256: `8b81bf72cc895711b899d0bb5d9e57a30b28476ae3df0331a0e155012d4c5406`.
- The 1.20× baseline-median thresholds are unchanged: iTerm p95 ≤13.670 ms, p99 ≤15.734 ms, input max ≤5.417 ms, CPU ≤0.806 s; Kitty p95/p99/input ≤46.442/50.885/16.178 ms; ANSI ≤14.857/15.494/13.310 ms.
- Same 120×40 stream/chat fixture and saved workload: 60 source frames at 30 fps over two seconds, 20 semantic updates, eight key inputs, 18 static images, 2.4-second drain, same protocol/quality budget. Runs were serial, with no concurrent Cargo/native job.
- Runner and independent image/identity decoder verified actual protocol routing, 20 applied pages, 8/8 input events, and all 60 offered frame identities. Kitty’s 35 censored frames per run are the declared output-budget behavior. A separate offline check found no collision between the fixture PNGs and the uniform video-marker identity pattern.
- Raw output and logs remain at `/workspace/work/noctiluca-perf-current-f309073/runs/`. The frozen run declaration and per-run machine-readable results are committed as [qualification-declaration.json](evidence/noctiluca-performance-f309073/qualification-declaration.json) and [comparison.json](evidence/noctiluca-performance-f309073/comparison.json).

## Current-source runs

| Protocol / run | p95 ms | p99 ms | input max ms | child CPU s | emitted / censored | Gate result |
|---|---:|---:|---:|---:|---:|---|
| iterm 1 | 13.230 | 13.564 | 4.596 | 0.840 | 59 / 1 | FAIL: cpu |
| iterm 2 | 24.643 | 24.806 | 6.439 | 1.352 | 60 / 0 | FAIL: p95, p99, input_max, cpu |
| iterm 3 | 18.814 | 19.560 | 2.185 | 0.792 | 60 / 0 | FAIL: p95, p99 |
| kitty 1 | 12.533 | 13.204 | 3.498 | 0.388 | 25 / 35 | PASS |
| ansi 1 | 4.385 | 4.965 | 2.734 | 0.297 | 60 / 0 | PASS |
| kitty 2 | 15.467 | 32.292 | 1.977 | 0.458 | 25 / 35 | PASS |
| ansi 2 | 4.659 | 5.767 | 3.981 | 0.262 | 60 / 0 | PASS |
| kitty 3 | 11.904 | 12.100 | 1.562 | 0.365 | 25 / 35 | PASS |
| ansi 3 | 3.521 | 4.771 | 2.332 | 0.218 | 60 / 0 | PASS |

Each run is compared independently with the original fixed ceilings. iTerm run 1 misses CPU; run 2 misses p95, p99, input max, and CPU; run 3 misses p95 and p99. Therefore iTerm passes 0/3 runs. Kitty passes 3/3 and ANSI passes 3/3. All runs preserved their binary SHA before/after and the expected workload populations.

## What the data says

The iTerm-compatible renderer is the only protocol that fails. The same gate already failed on `cbc7d93`, before the current source-grid follow-up: cbc run 1 missed input max, run 2 missed p95/p99/CPU, and run 3 missed CPU. The current follow-up therefore cannot be identified as the origin of the failure from these measurements.

Current iTerm medians across the three per-run statistics are p95 **18.814 ms**, p99 **19.560 ms**, per-run input maximum **4.596 ms**, and child CPU **0.840 s**. Compared with the cbc run medians, widget-draw time is nearly unchanged (227.884 vs 229.232 ms) and paint/encode time is lower (63.839 vs 108.786 ms); child CPU is slightly lower (0.840 vs 0.854 s). Despite those aggregate-stage improvements, the current p95/p99 tails still miss, including a severe run 2 outlier (24.643/24.806 ms, CPU 1.352 s). These stage totals are not a per-function profile and do not explain the tail spikes.

Load average recorded at run start ranged from 0.853 to 1.410 (1-minute), versus 0.001 to 0.162 for the earlier cbc sequence. No concurrent local build or native workload was running, but this host-load difference weakens causal comparison and may contribute to tail variance. The measurements are valid failures against the frozen gates; they are not enough to attribute the misses to one source function or prove a source regression.

**Qualification decision:** the current PR source does not pass release performance acceptance and should not be treated as release-qualified. The next engineering step is targeted profiling of iTerm frame conversion/painting and scheduling under a quiet, comparable host, then a measured fix and a new source qualification using these same gates—not recalculated thresholds.

## Follow-up trace-on/trace-off diagnostic

After the qualification above, the same immutable f309073 harness binary was run in three adjacent trace-on/trace-off pairs with the same Iterm workload, normalized environment, output capture, and independent decoder. These six runs are exploratory diagnostics, not replacements for the declared qualification. Linux `perf` is unavailable in this environment, so no sampled call-stack profile was collected.

| Pair | `RICH_MEDIA_TRACE` | p95 ms | p99 ms | input max ms | child CPU s | emitted / missing | Gate |
|---|---|---:|---:|---:|---:|---:|---|
| 1 | on | 11.925 | 12.769 | 3.501 | 0.727 | 59 / 1 | PASS |
| 1 | off | 12.421 | 13.640 | 3.951 | 0.751 | 59 / 1 | PASS |
| 2 | on | 13.690 | 31.230 | 2.741 | 0.765 | 60 / 0 | FAIL: p95, p99 |
| 2 | off | 12.378 | 13.133 | 3.974 | 0.700 | 59 / 1 | PASS |
| 3 | on | 11.493 | 11.920 | 3.876 | 0.696 | 60 / 0 | PASS |
| 3 | off | 11.600 | 11.645 | 3.458 | 0.671 | 59 / 1 | PASS |

Trace-on did not consistently worsen latency or CPU, though its second pair had a large p99 outlier. That run's slowest decoded frame was sequence 13 at 63.687 ms; the encoder trace also recorded a 22.813 ms encode while five pictures were live. This is a plausible correlation to investigate, not proof of causation. Five of the six diagnostic runs pass all four iTerm gates; one trace-on run fails p95 by 0.020 ms and p99 by 15.496 ms. The spread confirms that the original 0/3 failures cannot yet be attributed to a stable source-level bottleneck, and the diagnostic pass rate does not change the original qualification decision.

Per-run hashes and full sampled values are in [trace-ab.json](evidence/noctiluca-performance-f309073/trace-ab.json); raw diagnostic logs and output captures remain under `/workspace/work/noctiluca-perf-current-f309073/`.

The production code remains unchanged. Before making an encoder or scheduler change, collect a low-overhead profile that separates image fitting, PNG compression, registry/cache work, and frame scheduling; then use the unchanged gate and full semantic/output checks on the resulting source.
