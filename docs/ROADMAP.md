# Roadmap

What each release delivered, and where this goes next: the
[0.0.12 plan](plans/0.0.12.md), after the 0.0.11 cohort (published 2026-09-24).
Ordered by what unblocks people, not by what is most interesting to build.

Two rules constrain everything here:

- **The core stays a faithful mirror.** Anything upstream `rich` does not have
  goes in `rich-ext`, `rich-art`, or a new crate — never in `crates/rich`. This
  is what keeps an upstream sync a mechanical diff rather than a merge conflict.
  See [AGENTS.md](https://github.com/buchochelliq-labs/rs-rich-cli/blob/main/AGENTS.md).
- **Byte-parity is the correctness oracle.** A feature that cannot be checked
  against real Python `rich` needs its own argument for why it is correct.

---

## 0.0.2 — released 2026-08-11

The correctness milestone shipped. It paid down the confirmed rendering,
markup, colour, and CLI correctness debt; see the
[0.0.2 changelog](https://github.com/buchochelliq-labs/rs-rich-cli/blob/main/CHANGELOG.md#002--2026-08-11)
for the released work.

---

## 0.0.3 — released 2026-09-10

The patch release accepted:

- correctness fixes confirmed against current `main` and, where applicable, the
  pinned upstream oracle;
- documentation and package-metadata consistency fixes; and
- release hardening that makes the documented CI, parity, packaging, or
  clean-room verification gates more reliable.

New features were deferred from `0.0.3`. Subsequent fixes go
under `Unreleased`, naming every affected crate.

### Completed source scope (2026-09-10)

The 0.0.3 fixes are merged into main through PR #111: Markdown image and container parity,
independent release selection, tested TOML pin parsing, JSON precision/depth,
optional escape-safe JSON presentation, safe GIF redirection, CSV streaming,
title markup, notebook layout, graphical diff exports and native Windows paging.

See [release notes](releases/0.0.3.md) and [UAT closeout](remaining-uat-0.0.3.md).
The selected changes are under the 0.0.3 changelog heading; `Unreleased` now tracks
subsequent work. Registry publication and verification are tracked by the
protected release workflow, separately from the completed source merge.

### Deferred from 0.0.3 into 0.0.4

- [#65](https://github.com/buchochelliq-labs/rs-rich-cli/issues/65): GIF half-block rendering.
- [#74](https://github.com/buchochelliq-labs/rs-rich-cli/issues/74): further CSV memory reductions and long-line wrapping performance.
- [#62](https://github.com/buchochelliq-labs/rs-rich-cli/issues/62): image/encoding diagnostics and additional encoding support.
- [#45](https://github.com/buchochelliq-labs/rs-rich-cli/issues/45): syntax-highlighting performance.

---

## 0.0.4 — released 2026-09-10

The [0.0.4 development plan](plans/0.0.4.md) scopes these four issues
above into small PRs: reproduce current baselines, improve diagnostics, add
explicit GIF blocks, reduce CSV memory and wrapping costs, then optimize the
measured syntax bottlenecks. It defines acceptance cases, per-crate version
decisions, real CLI evidence, independent review and docs-site updates.

Diagnostics (#62), GIF rendering (#65), CSV/wrapping (#74) and opt-in syntax
parsing reuse (#45) are merged into main and published in all four 0.0.4
packages. The [release notes](releases/0.0.4.md) record measurements,
limitations and successful exact-version consumer verification.

---

## 0.0.5 — published 2026-09-13

`rs-rich-ext` and `rs-rich-cli` 0.0.5 were published on 2026-09-13; core and art
stayed at 0.0.4. They shipped the `--sanitize` opt-in (#64), the version-checked
golden capture, the differential corpus harness and the `library_bench` probe.

The [0.0.5 preparation plan](plans/0.0.5.md) proposed four bounded workstreams:
issue #15 golden-test gaps, #34 reproducible differential fuzzing, #35 library
benchmarks with CI artifacts, and #64 explicit input sanitization. Start with
the oracle/case contract; benchmark work can proceed independently. Keep default
rendering unchanged and sanitizer policy in extensions.

The plan audits existing evidence and dependency PRs, defines acceptance gates,
and preserves independent package selection.

---

## 0.0.6 — CLI foundations and streaming automation

The [0.0.6 plan](plans/0.0.6.md) scopes the next release to `rich-cli`
automation: task-oriented subcommands that preserve existing flat flags,
stable exit-code classes, a shared JSON report envelope, and bounded JSONL/log
streaming from files and stdin.

This deliberately defers the larger intuiTUIve/TUI, watch, batch, config and
dependency-upgrade work so the command/output contract can land first.

## 0.0.7 — rich-art image commands

The image workstream adds the reusable `rich-art::ImageArt` capability facade,
ASCII, Braille, half-block and Sixel backends, and `rich image` / `--image`
CLI routing with width, height and explicit mode selection. Existing GIF and
diff report envelopes remain unchanged; unsupported graphics modes downgrade
only where documented or return an actionable error.

## 0.0.7 — watch and workflow foundations

The 0.0.7 CLI workstream adds the first binary-boundary watch convenience:
`rich --watch RESOURCE` polls local files without busy-looping, keeps running
through atomic-save gaps and parse failures, and recovers on a later valid
frame. Fetch-enabled builds may poll URLs with configurable intervals and
response caching. Redirected output remains a deterministic one-shot snapshot.
Rendering stays in the existing CLI/core paths; no new core refresh behavior is
introduced.

### Batch and configuration

The CLI now plans explicit files, directories, and globs deterministically,
reuses the existing render/export pipeline per item, refuses silent overwrites,
and reports aggregate machine-readable status. TOML profiles are discovered
from platform roots (or selected explicitly), with command-line values taking
precedence. Since CLI 0.0.8, full TOML and every inactive profile are strictly
validated; scalar-only parsing is historical. `--jobs N` runs bounded subprocess
workers for file exports, spooling output to disk and replaying in input order.
Terminal-only batches remain serial. `--dry-run` inspects plans without writing
exports; `config show` / `config validate` expose effective configured settings.

## 0.0.8 — published workflow and crop controls

[CLI 0.0.8 / art 0.0.6](releases/0.0.8.md) completed independent publication and
exact-version registry verification. It shipped strict TOML profiles, parallel
file-export batches, dry-run/config inspection, automatic paging, crop anchors
and the guided suite tour. Core 0.0.4 and ext 0.0.6 stayed unchanged.

## 0.0.9 — published 2026-09-22

The [accepted plan](plans/0.0.9.md) delivered CLI 0.0.9 and art 0.0.7: named themes
and explicit style overrides, terminal-only human batch progress, Ctrl+C worker
cleanup, listable/selectable demo sections and read-only doctor diagnostics.
Art adds opt-in ANSI256 and Floyd–Steinberg preprocessing for ASCII/half-block
still images. Truecolor/no-dither remains the default; GIF, diff, Braille and
Sixel preprocessing remain outside this slice of #125.

The expanded scope (#192) added core 0.0.5 and ext 0.0.7: render targets and
capabilities, layout constraints, typed events and diagnostics, coordinated Live
regions, render snapshots, batch naming and hardened publication (#196), and image
transforms with Bayer dithering. All four packages were published on 2026-09-22;
see the [expanded release notes](releases/0.0.9-expanded.md).

## 0.0.10 — progress you can ship with (published)

Every workstream is merged to `main`, and the cohort was published on 2026-09-23. The
release test and release runs are recorded in the [0.0.10 release notes](releases/0.0.10.md).

The [0.0.10 plan](plans/0.0.10.md) targets core parity over new surface area:
Progress time, rate and spinner columns with a task-driving API (#6), upstream's
theme stack (#3), wider golden and differential coverage around both (#15, #34),
the four 0.0.9 leftovers (#134, #146, #149, #151) and release hardening (Trusted
Publishing, Node 24 actions, pending dependency bumps). Also in scope: multi-file
debounced watch (#139), ANSI16/grayscale image modes, image adjustments and quadrant
blocks (#125, #126, #124 follow-up), and `~~~` strikethrough parity (#9). Core moves to 0.0.6, so
ext, art and CLI move with it (0.0.8, 0.0.8, 0.0.10). The Python wrapper (#197)
proceeds as a separate spike.

### Confidence tooling for expanded CLI surfaces

The confidence slice adds bounded, deterministic support around the selected
batch/profile/watch/image work:

- **#150/#135:** `scripts/snapshot_cli.py` provides injectable width and
  terminal capability profiles with PTY terminal forcing for color modes,
  option-terminator (`--`) positioning, stable environment variables, newline
  normalization, and readable failure diffs. The initial corpus covers the
  currently stable Markdown and JSON stdin paths.
- **#34:** the differential corpus and generator now include box renderables
  (`Panel` box variations) and vary the safe-box capability profile in addition
  to width, color, markup, styles and overflow. 0.0.10 adds Table, Rule,
  Padding and Align generators, per-colour-system oracle isolation, a
  failure-kind-preserving shrinker and a nightly 20,000-case run on `main`.
  Its first runs filed #442–#449 (see [parity](parity.md#differential-fuzzing)).
- **#35:** `library_bench` accepts explicit width and color-system arguments and
  records them in its JSON artifact. CI publication and threshold enforcement
  remain deferred; output hashes stay the blocking correctness signal.

Image behavior remains covered by the existing focused `rich-cli` integration
tests. Snapshot cases for the merged batch/profile/watch/image workflows remain
follow-up work; capability/RenderTarget decisions (#147/#148) remain separate.

## 0.0.11 — developer ergonomics and core usability (published 2026-09-24)

All 12 workstreams of the [0.0.11 plan](plans/0.0.11.md) and the release test's
fixes shipped. The published cohort is core 0.0.7, the new `rs-rich-macros` 0.0.1, ext 0.0.9, art 0.0.9
and CLI 0.0.11. See the [0.0.11 release notes](releases/0.0.11.md).

The plan covered
[milestone 2](https://github.com/buchochelliq-labs/rs-rich-cli/milestone/2) plus
everything unfinished from 0.0.10 (#6, #9, #34, #126, #144, and the new #498
and #499). Delivered:

- core parity fixes for the fuzzer's findings (#442–#449), Markdown styled
  table cells (#9), the rest of Progress (#6: Live-driven display, `track()`,
  `RenderableColumn`, transient and disabled displays) and `LogRender` (#10);
- diagnostics and stack traces with `anyhow` adapters, structured data and serde
  helpers with `rich inspect`, the `richf!`/`#[derive(Rich)]` macros, `clap`
  help and errors, `tracing` spans and `RichHandler`;
- a shared diff engine with test helpers and `rich diff` / `rich bench compare`,
  capability and accessibility policies with `rich ansi explain` and
  `rich doctor`, workflow renderables, and the `view`, `hex`, `unicode`, `env`
  and `capture` commands;
- Atkinson dithering, OKLab distance, Sixel/GIF colour modes, alpha backgrounds
  (#498) and `--theme-file` (#499).

## 0.0.12 — plugin platform and extensibility (planned)

The [0.0.12 plan](plans/0.0.12.md) covers
[milestone 3](https://github.com/buchochelliq-labs/rs-rich-cli/milestone/3):

- pluggable code highlighters with syntect and lumis adapters (#521–#526);
- a public plugin API on the extension registry (#14, #232 phase 1);
- Markdown fence extensions, with Mermaid flowcharts as the first plugin (#222);
- composable transforms (#216);
- a render-tree design spike (#226);
- native image sizing (#519).

Core gains extension points only, and its default output stays upstream's.

---

## 0.0.2 planning record

The milestone centered on rewriting the markup tag scanner against `RE_TAGS`.

The scanner is hand-rolled and diverges from upstream's
`((\\*)\[([a-z#/@][^[]*?)])` in three ways:

| divergence | effect |
|---|---|
| Backslashes counted individually, not by parity (`divmod(n, 2)`) | `\\[b]x[/b]` **errors** where upstream renders bold |
| `[` accepted inside a tag body (upstream's `[^[]*?` forbids it) | text swallowed *and* spurious errors, in both directions |
| Zero-length spans discarded | a segment boundary upstream produces is lost |

One rewrite closes all three, and closes
[DIVERGENCES §2](DIVERGENCES.md) — which today documents a gap without fixing it.

---

## 0.1.0 — the gaps that block real adoption

~~**Theme stack**~~ — done in 0.0.10 (core 0.0.6): `push_theme`, `pop_theme`
and a `use_theme` guard that derefs to the console; see [§14](DIVERGENCES.md).

**Windows legacy console** — [#12](https://github.com/buchochelliq-labs/rs-rich-cli/issues/12).
Needs an explicit `unsafe` opt-in, since the workspace denies `unsafe_code`.
Without it, pre-Windows-10 terminals silently fall back to plain output.

~~**Progress time/rate columns and Live integration**~~ — done: time, rate and
spinner columns in 0.0.10 (core 0.0.6); the Live-driven display, `track()` and
`RenderableColumn` in 0.0.11 (core 0.0.7). See [§16](DIVERGENCES.md) and
[§17](DIVERGENCES.md).

Independent per-crate versioning already applies below `0.1.0`. A `0.0.x`
dependency bump requires updating its dependents and publishing changed
manifests; unrelated crates need not bump. See [BRANCHING.md](BRANCHING.md).

---

## Confidence, not features

The highest-leverage work on this list is not a feature.

### Differential fuzzing against real Python rich

Two adversarial reviews found **18 confirmed defects** in code that had passing
tests and looked finished. Every one was found the same way: generate an input,
run both implementations, compare bytes. That is what a fuzzer does, tirelessly
and without getting bored.

**Done.** `scripts/diff_rich.py` generates markup, styles, widths, overflow
and box renderables, compares the port against real `rich` byte for byte, and
shrinks any mismatch. It has run nightly on `main` (20,000 cases) since 0.0.10;
its first runs filed #442–#449, all fixed in 0.0.11. See
[parity](parity.md#differential-fuzzing).

### Benchmarks

CLI timing and CSV memory measurements now exist in the
[benchmarks](benchmarks.md) and [runtime audit](runtime-audit-0.0.3.md). Partly
done in 0.0.11: `rich_ext::qa::bench` records benchmark runs and
`rich bench compare` gates on regressions (exit 5). A tracked library
microbenchmark suite with thresholds enforced in CI remains follow-up work.

---

## Beyond `rich` — things Python cannot do

These are **additions**, so they live outside the faithful core. Ordered by how
much they'd change day-to-day use.

### ~~Compile-time checked markup~~ — delivered in 0.0.11

`rs-rich-macros` 0.0.1 (through ext's `macros` feature) provides `richf!`,
`style!`, `theme_key!` and `markup!`; see [Macros](guide/ext/macros.md). The
original proposal:

```rust
// Unbalanced tag, unknown style name → a compile error, not a silent no-op.
console.print(richf!("[bold]{name}[/]"));
```

A proc-macro that parses markup at compile time. Python fundamentally cannot do
this — and it prevents a bug class this project has hit repeatedly: an unknown
tag name renders as a *no-op* upstream, so a typo silently produces unstyled
text. Making that a compile error is a genuine improvement on the original, not
just a port of it.

### ~~Derive-driven rendering~~ — delivered in 0.0.11

Delivered as `#[derive(Rich)]` and the serde-driven `data::print_json`,
`print_table`, `print_tree` and `Explorer`; see
[Structured data](guide/ext/structured-data.md). The original proposal:

```rust
#[derive(Table)]
struct Release { name: String, #[table(justify = "right")] downloads: u64 }

console.print(&releases.as_table());
```

Plus a `serde`-driven `Pretty` that renders any `Serialize` value as a tree or
table. This is the honest Rust answer to upstream's `inspect`/`repr` modules,
which don't map onto a language without runtime reflection — *type-driven*
instead of repr-parsing, and better than the original for it.

### ~~`clap` integration~~ — delivered in 0.0.11

Delivered as ext's `clap` feature on top of `cli_doc`; see
[CLI authoring](guide/ext/cli-authoring.md). The original proposal: render `clap` help, errors and usage through `rich`. `clap` is close to universal
in Rust CLIs, so this is the single widest-reach item here — and it is the kind
of thing people would adopt the crate *for*.

### Inline images via terminal graphics protocols

`rich-art` already does image→ASCII. Kitty's graphics protocol, iTerm2's inline
images and Sixel would render *actual* images — something upstream `rich` has no
answer to at all. Sixel shipped in 0.0.7 (art 0.0.5); Kitty and iTerm2 remain.

### Diagnostics integration

`miette` / `color-eyre` / `anyhow` reporters rendered through `rich`, so error
output matches the rest of an application's styling. The `Diagnostic` type and
its `anyhow` adapter shipped in 0.0.11 (see [Diagnostics](guide/ext/diagnostics.md));
`miette` and `color-eyre` remain.

### ~~A snapshot-testing helper~~ — delivered

Delivered in ext's `testing` feature rather than a separate crate:
`RenderSnapshot` (ext 0.0.7), then `assert_rich_eq!` and `qa::screenshot` in
0.0.11; see [QA tooling](guide/ext/qa.md). The original proposal, `rich-test`:
assert terminal output in *users'* test suites, with readable diffs
and SVG artifacts on failure. It dogfoods `Console::export_svg` — the same
mechanism that generates every image in these docs.

---

## Deliberately not planned

**Jupyter integration** and **`inspect`/`repr` of Python objects**
([#19](https://github.com/buchochelliq-labs/rs-rich-cli/issues/19),
[§19](DIVERGENCES.md)). These don't map onto Rust. Reimagining them is a design
project rather than a port, nobody has asked, and the derive/serde work above is
the useful half of the same idea.

---

## Open questions

**Is byte-parity still the north star at `1.0`?** It has been an excellent
correctness oracle. But [§3](DIVERGENCES.md) — byte offsets in `Text` spans,
where upstream uses code points — is a case where the *faithful* choice is
arguably the wrong Rust API. There will be more. Better decided deliberately than
by drift.

**Upstream sync cadence.** `rich` 15.x will move. Watch releases and sync
promptly, or sync on demand when something is needed? The `sync-upstream` skill
handles the mechanics either way.
