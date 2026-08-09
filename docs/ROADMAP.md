# Roadmap

Where this goes after `0.0.1`. Ordered by what unblocks people, not by what is
most interesting to build.

Two rules constrain everything here:

- **The core stays a faithful mirror.** Anything upstream `rich` does not have
  goes in `rich-ext`, `rich-art`, or a new crate — never in `crates/rich`. This
  is what keeps an upstream sync a mechanical diff rather than a merge conflict.
  See [AGENTS.md](https://github.com/buchochelliq-labs/rs-rich-cli/blob/main/AGENTS.md).
- **Byte-parity is the correctness oracle.** A feature that cannot be checked
  against real Python `rich` needs its own argument for why it is correct.

---

## 0.0.2 — pay the known correctness debt

Not speculative. These are **confirmed and reproduced** findings from adversarial
review, left unfixed only so they could land as one coherent change.

### Rewrite the markup tag scanner against `RE_TAGS`

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

`0.1.0` is also where **independent per-crate versioning** becomes meaningful.
Below it, Cargo treats `^0.0.x` as an exact requirement, so lockstep is forced
whether or not we choose it. See [BRANCHING.md](BRANCHING.md).

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

There are none. A rendering library with no performance data is a latent
surprise for whoever first puts it in a hot loop. A baseline is worth more than
any optimisation made without one.

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
