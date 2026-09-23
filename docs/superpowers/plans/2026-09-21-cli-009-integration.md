# Expanded Release Integration Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Verify the combined features and prepare accurate package/issue evidence.

**Architecture:** Integrate interfaces first, then update independent crate versions and release documentation. Keep release preparation distinct from tagging or publishing.

**Tech Stack:** Rust 2021, Rust 1.90, Cargo and the existing public Rich APIs.

**Spec:** [Approved design](../specs/2026-09-20-cli-0.0.9-expanded-design.md).

## Global Constraints

Read the [release index](2026-09-21-cli-009-index.md) and AGENTS.md before execution. Preserve default output; keep new library behaviour in rich-ext/rich-art. Do not add required Renderable methods or fields to ConsoleOptions/ImageOptions. No implicit ambient probes, source-file reads or global logger installation. Rust 1.90, Rust 2021 and `unsafe_code = "deny"` apply. Before each commit run `cargo fmt --all --check`, `cargo clippy --all-targets -- -D warnings`, and `env -u NO_COLOR cargo test`; commit only after all pass. Publication is a separate handoff.

## Review Focus

1. New core context must not change upstream goldens (G1).
2. Optional adapters, image features and defaultless builds must compile independently (G1).
3. Packaged crates must include examples/source required by published APIs (G2).
4. Internal caret requirements must match independently bumped 0.0.x versions (G2).
5. Broad roadmap issues must retain residual work after selected slices ship (G2).

---

### G1: Cross-feature acceptance and independent review

**Files:** Create `crates/rich-cli/tests/expanded_release.rs`; modify `.github/workflows/ci.yml` only if matrix lacks required feature coverage; modify `scripts/test_release_readiness.py` and `docs/superpowers/specs/2026-09-21-cli-0.0.9-design-review.md` with implementation evidence, keeping original review labelled architectural.

**Interfaces:** Consume A–F APIs unchanged. No new runtime API. Cross-feature fixtures use explicit targets, nested image/layout renderables, structured events with diagnostics, and a Live coordinator with captured writer. Tests must use real public APIs and fixture expected values.

- [ ] Add integration cases: nested rotated grayscale ImageArt in constrained Layout captured without Sixel under conflicting TERM; typed event with diagnostic exported to HTML/SVG; batch name templates with Bayer and workers1/4. Assert no leaked controls, bounded cell widths, field order and exact planned filenames:

```rust
assert!(!captured.contains("\x1bP"));
assert_eq!(serial_outputs, parallel_outputs);
assert_eq!(planned_names, vec!["1-report.html", "2-report.html"]);
```

  Fixtures come from independently labelled small inputs. Run tests before integrating branch pieces; if already passing, record that no integration patch is needed rather than manufacturing a failing test.
- [ ] Run `env -u NO_COLOR cargo test -p rs-rich-cli --test expanded_release`; identify real missing routing/config links, if any.
- [ ] Correct only failing boundary wiring, keeping owning A–F public signatures. Use target context at nested render boundaries, never a second environment detector; carry immutable options into workers. If no failure occurs, preserve code and retain acceptance tests as evidence.
- [ ] Run the full acceptance matrix:

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
env -u NO_COLOR cargo test --workspace --all-features
env -u NO_COLOR cargo test -p rs-rich --test golden
cargo check -p rs-rich-ext --no-default-features
cargo check -p rs-rich-cli --no-default-features
cargo check -p rs-rich-art --no-default-features
cargo +1.90.0 check --workspace --all-features
python scripts/test_live_regions_pty.py
python -m unittest discover -s scripts -p 'test_release*.py'
```

  Record actual outcomes; if MSRV toolchain unavailable, rely on successful CI MSRV job and say so. Run original default global gates too. Request independent whole-branch review under requesting-code-review skill; resolve blockers and rerun only affected gates plus mandatory commit checks. Commit `test: verify expanded release feature integration`.

### G2: Version cohort, documentation and issue disposition

**Files:** Modify workspace Cargo.toml/Cargo.lock, all four crate Cargo.toml files as needed, CHANGELOG.md, docs/plans/0.0.9.md, docs/cli.md, docs/PORTING.md, docs/PLUGINS.md, crate READMEs; update generated version/reference files through existing scripts rather than hand-editing. Preserve AGENTS.md.

**Interfaces:** Proposed versions core0.0.5/ext0.0.7/art0.0.7/CLI0.0.9, contingent on actual registry/tag state. Dependency order core→ext/art→CLI. A version already published requires a new version and an updated plan; no overwriting tags. Issue tracking updates record implemented acceptance and remaining scope; no runtime interface change.

- [ ] Run existing release readiness/package tests before metadata changes and record mismatches against the approved cohort. Add a regression only for a discovered automation gap, using independent manifest parsing:

```python
import tomllib
from pathlib import Path
workspace = tomllib.loads(Path('Cargo.toml').read_text())
for alias in ('rich', 'rich-ext', 'rich-art'):
    dep = workspace['workspace']['dependencies'][alias]
    package = tomllib.loads(Path(dep['path'], 'Cargo.toml').read_text())['package']
    assert dep['version'] == package['version']
```

- [ ] Inspect current tags/registry package versions and existing release script CLI help. Update only unpublished cohort versions and their workspace caret requirements. Generate lockfile/reference/version metadata with existing scripts, preserving independently versioned packages.
- [ ] Document target APIs, extension seam rationale, constraints, events/adapters, Live ownership/viewport limitations, batch flags, image pipeline, enum migration and actual feature examples. Record tests and screenshots from G1/F2. Check every selected issue against its acceptance text; fully satisfied issues may close, partial #6/#10/#125/#126/#144 retain residual scope and links. Do not pre-close any issue based only on this plan.
- [ ] Run global gates, release Python tests and package checks using `python scripts/check_packages.py --help` then its documented checks. Run strict MkDocs through existing docs workflow if unavailable locally. Validate all four publishable package dependency manifests in dependency order without publishing. Review generated diff for accidental upstream or golden changes.
- [ ] Commit `chore: prepare expanded CLI 0.0.9 release evidence`; update tracker #192 and implementation PR with exact commits/test results, residual issue scope and release order. Stop before tags/registry publication. The published package state must never be inferred solely from local versions.
