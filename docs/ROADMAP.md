# Roadmap

Where this goes after the `0.0.4` release preparation. Ordered by what unblocks people, not by what is
most interesting to build.

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

## 0.0.4 — diagnostics, GIF and performance development

The [0.0.4 development plan](plans/0.0.4.md) scopes these four issues
above into small PRs: reproduce current baselines, improve diagnostics, add
explicit GIF blocks, reduce CSV memory and wrapping costs, then optimize the
measured syntax bottlenecks. It defines acceptance cases, per-crate version
decisions, real CLI evidence, independent review and docs-site updates.

The selected diagnostics (#62), GIF rendering (#65), CSV/wrapping (#74) and
opt-in syntax parsing reuse (#45) are implemented and included in the prepared source.
The [0.0.4 release preparation](releases/0.0.4.md) records measurements,
limitations and the independently selected package versions. Publication is
separate; the protected workflow repeats source and package verification.

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

**Theme stack** (`push_theme` / `pop_theme`) — [§14](DIVERGENCES.md).
Any application with themed output needs it. The work is a design pass, not
typing: an RAII guard borrowing the `Console` mutably makes `console.print(…)`
*inside* the guard a borrow error, which is the entire use case, and a `RefCell`
stack breaks `Console::theme() -> &Theme`.

**Windows legacy console** — [#12](https://github.com/buchochelliq-labs/rs-rich-cli/issues/12).
Needs an explicit `unsafe` opt-in, since the workspace denies `unsafe_code`.
Without it, pre-Windows-10 terminals silently fall back to plain output.

**Progress time/rate columns and Live integration** — [§16](DIVERGENCES.md),
[§17](DIVERGENCES.md). A progress bar with no ETA is half a feature, and it is
the most visible gap for anyone writing a CLI.

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

Both halves already exist — `scripts/capture_golden.py` drives real `rich`, and
the port is deterministic given a fixed console. Wiring them into a property test
that generates markup, styles, widths and overflow combinations and asserts
byte-equality converts *"we reviewed this carefully"* into *"we checked ten
million cases"* — and keeps paying out on every future change, including upstream
syncs.

### Benchmarks

CLI timing and CSV memory measurements now exist in the
[benchmarks](benchmarks.md) and [runtime audit](runtime-audit-0.0.3.md). A repeatable
library microbenchmark suite and tracked regression thresholds remain useful
follow-up work.

---

## Beyond `rich` — things Python cannot do

These are **additions**, so they live outside the faithful core. Ordered by how
much they'd change day-to-day use.

### Compile-time checked markup

```rust
// Unbalanced tag, unknown style name → a compile error, not a silent no-op.
console.print(richf!("[bold]{name}[/]"));
```

A proc-macro that parses markup at compile time. Python fundamentally cannot do
this — and it prevents a bug class this project has hit repeatedly: an unknown
tag name renders as a *no-op* upstream, so a typo silently produces unstyled
text. Making that a compile error is a genuine improvement on the original, not
just a port of it.

### Derive-driven rendering

```rust
#[derive(Table)]
struct Release { name: String, #[table(justify = "right")] downloads: u64 }

console.print(&releases.as_table());
```

Plus a `serde`-driven `Pretty` that renders any `Serialize` value as a tree or
table. This is the honest Rust answer to upstream's `inspect`/`repr` modules,
which don't map onto a language without runtime reflection — *type-driven*
instead of repr-parsing, and better than the original for it.

### `clap` integration

Render `clap` help, errors and usage through `rich`. `clap` is close to universal
in Rust CLIs, so this is the single widest-reach item here — and it is the kind
of thing people would adopt the crate *for*.

### Inline images via terminal graphics protocols

`rich-art` already does image→ASCII. Kitty's graphics protocol, iTerm2's inline
images and Sixel would render *actual* images — something upstream `rich` has no
answer to at all.

### Diagnostics integration

`miette` / `color-eyre` / `anyhow` reporters rendered through `rich`, so error
output matches the rest of an application's styling.

### A snapshot-testing helper

`rich-test`: assert terminal output in *users'* test suites, with readable diffs
and SVG artifacts on failure. It dogfoods `Console::export_svg` — the same
mechanism that generates every image in these docs.

---

## Deliberately not planned

**Jupyter integration** and **`inspect`/`repr` of Python objects**
([#10](https://github.com/buchochelliq-labs/rs-rich-cli/issues/10),
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
