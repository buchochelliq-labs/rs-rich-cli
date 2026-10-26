# Fuzzing

The `fuzz/` directory holds [cargo-fuzz](https://github.com/rust-fuzz/cargo-fuzz)
targets for every parser in the repository that takes outside input. They
run every night in CI (`.github/workflows/fuzz.yml`). This
is the fuzzing item of the 1.0 plan's
[Stage 4](../plans/1.0.0.md#stage-4--hardening-01x), brought forward to
0.0.23 ([item 18](../plans/0.0.23.md)).

This is a different tool from the
[differential fuzzer](../parity.md#differential-fuzzing). That one checks that
our output matches Python rich's. These targets check that no input makes a
parser panic, hang or run out of memory.

## The targets

`fuzz/` is its own workspace, with its own `Cargo.lock`, as `cargo fuzz init`
makes it, so the main workspace never builds it. Each target uses the
crates' public entry points. Rendering targets cut their input to 4 KiB and
draw on a console 1 to 160 columns wide, so a big input that is legitimately
slow is not reported as a timeout. Parse-only targets read up to 64 KiB. The
bounds are in `fuzz/src/lib.rs`.

| Target | Crate | What it feeds |
|---|---|---|
| `markup` | rs-rich | `Text::from_markup`, `markup::render`, `Style::parse`, then a render |
| `ansi` | rs-rich | `AnsiDecoder`, then a render |
| `markdown` | rs-rich | `Markdown::new`, then a render (no syntect) |
| `json` | rs-rich | `Json::new`, then a render |
| `dot` | rs-rich-diagram | the DOT parser, then the layout and drawing |
| `mermaid` | rs-rich-mermaid | the flowchart, sequence and ER parsers, then `Mermaid` drawn as text |
| `csv` | rs-rich-data | the sniffer, `CsvReader::read` and `source` (CSV and TSV), then type inference |
| `jsonl` | rs-rich-data | `jsonl::read` and `jsonl::source`, then type inference |
| `stylesheet` | rs-rich-intuituive | `Stylesheet::parse` |
| `vt` | rs-rich-record | the VT emulator (`Terminal::process`, in chunks, at a size a tape may ask for), then the screen's text and snapshot |
| `tape` | rs-rich-record | the tape parser |
| `web_input` | rs-rich-web | the input decoder (`input::decode`) |
| `web_frames` | rs-rich-web | WebSocket frames read as the server reads them (tungstenite, the server's limits), each message's input decoded |
| `marketplace_documents` | rs-rich-marketplace | a listing (then version resolution), a lockfile, a `keys-N.json`, a `minisign.pub`, a key id |
| `marketplace_signature` | rs-rich-marketplace | a `.minisig` file, against a real index and a key rotation |
| `marketplace_signed` | rs-rich-marketplace | an `index.json` and a `keys-N.json`, signed with the test keys so the parsers behind the signature check are reached |
| `marketplace_artifact` | rs-rich-marketplace | a micro pack's zip and `pack.json`, a theme file, a WASM header |
| `plugin_manifest` | rs-rich-plugin-api | the ABI's manifest text; one that parses must read back the same after `to_manifest` |
| `plugin_wasm` | rs-rich-ext | a WASM plugin loaded with the default limits (fuel cut to 5 million per call), each capability called |

Not reached, because they are crate-private: rs-rich-web's own message
parser (`parse_message`) and HTTP request-head reader. `web_frames` reaches
only the input decoder behind them. A native (dylib) plugin's descriptor is
raw pointers and cannot be fuzzed this way.

Each target has a small seed corpus in `fuzz/seeds/<target>/`, taken from
the crates' test fixtures (golden markup and Markdown, DOT and tape files,
the CSV and JSONL fixtures, the example stylesheets, the listing examples,
the example WASM plugins). The first byte of a rendering target's input is a
parameter (the width, ASCII or box drawing), so its seeds start with one.
Only seeds are committed: the corpus a run grows stays in
`fuzz/corpus/`, which Git ignores, and in CI's cache.

## Running a target locally

cargo-fuzz needs a nightly toolchain. CI pins both:

```bash
rustup toolchain install nightly-2026-05-20
cargo install --locked cargo-fuzz --version 0.13.2

cargo +nightly-2026-05-20 fuzz list
cargo +nightly-2026-05-20 fuzz run markup fuzz/corpus/markup fuzz/seeds/markup -- -max_total_time=60
```

The first directory is where new inputs are written, the second is read
only. Without `-max_total_time` a run goes on until it finds something or
is stopped. Building every target with sanitizers takes about 2.5 GB in
`fuzz/target`.

## Triaging a finding

A failing run writes the input to `fuzz/artifacts/<target>/` (`crash-…`,
`timeout-…` or `oom-…`). In CI the job fails, the input is the
`fuzz-artifacts-<target>` artifact, and the job summary says how to
reproduce it.

1. **Reproduce it:** `cargo +nightly-2026-05-20 fuzz run <target>
   fuzz/artifacts/<target>/<file>` runs that one input and prints the panic
   and its backtrace.
2. **Shrink it:** `cargo +nightly-2026-05-20 fuzz tmin <target> <file>`.
3. **Decide where it belongs:**
   - A bug in our code: fix it with a regression test in the crate, with
     the shrunk input as a literal. In `crates/rich`, compare with Python
     rich at the pinned version first: the fix must keep parity, and the
     test says it was captured from rich 15.0.0.
   - A bug in a dependency (vt100, pulldown-cmark, tungstenite, wasmi, zip):
     report it upstream, and guard the call in our crate if that is cheap.
   - A slow input that is legitimate (a timeout on a large drawing): bound
     the work in the crate if a user could hit it. Do not lower the
     target's bounds to hide it.
4. **Record it** until it is fixed, in [Known findings](#known-findings)
   below, with the target, the input and the issue. A finding stays open
   until its fix is merged and the target runs clean on it.

Never weaken a target to make a finding go away.

## The four-week clock

The [1.0 exit criteria](../plans/1.0.0.md#exit-criteria-for-100) ask that the
fuzz targets have "run in nightly CI for at least four weeks without an open
finding". The clock starts with the first scheduled run of `fuzz.yml` on
`main` after this lands, expected the night of the merge (the schedule is
01:43 UTC). Record that date here when it happens:

- **First nightly run:** not yet (the workflow is on the 0.0.23 branch).
- **Earliest the criterion can be met:** four weeks after that date, with
  no open finding below. A new finding restarts the clock once it is fixed.

## Known findings

None open. Fixed while the targets were written (0.0.23), each with a
regression test in its crate:

- `ansi`: `AnsiDecoder` panicked on a multi-byte character after a
  charset-select escape (`ESC (`), slicing inside the character. It now
  skips one character, as upstream does (rs-rich, `ansi.rs`, with a test
  captured from rich 15.0.0).
- `plugin_wasm`: a plugin that loops over `memory.grow` with a register
  operand overflowed the host's stack and aborted the process, in an
  optimised build only. wasmi 2.0.0's tail-call dispatch grew the stack on
  every instruction there. rs-rich-ext now builds wasmi with its portable
  (loop) dispatch, which is slower but cannot do that. The test fails
  without the fix only under `cargo test --release`: a debug build always
  used the portable dispatch.
- `plugin_wasm`: wasmi 2.0.0 divided by zero (a panic) on a code section
  that declares no functions but holds bytes. rs-rich-ext now validates a
  module with wasmparser (the parser wasmi uses, at its version) first,
  which refuses that section.

Recorded, not a finding of our code:

- vt100 0.16 panics on a wide character in a one-column screen, and on a
  resize that cuts a wide character at the right edge. rs-rich-record
  catches the panic and reports it as an error (tested by
  `a_vt100_panic_is_an_error`), rs-rich-embed starts a blank screen, and a
  tape cannot ask for fewer than `tape::MIN_COLUMNS` (2) columns. The `vt`
  target therefore uses tape sizes and does not resize. Lifting either
  limit needs a fixed vt100.

The two wasmi bugs are worth reporting upstream; neither is reported yet.
