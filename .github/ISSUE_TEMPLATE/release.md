---
name: Release
about: Track cutting and publishing a version
title: "release: <version>"
labels: ["type:release", "type:infra"]
---

## Target

- Version: ______ (all four crates move in lockstep)
- Kind: release candidate / final (delete one)
- Previous released version: ______

## Checklist (see the `release` skill and docs/BRANCHING.md)

- [ ] `main` is green; `## [Unreleased]` in `CHANGELOG.md` has content
- [ ] `python scripts/capture_golden.py` leaves `git diff` clean
- [ ] Branch cut from fresh `origin/main` (`rc/<version>` or `release/<version>`)
- [ ] Version bumped in `Cargo.toml` — the workspace version **and** the three
      `[workspace.dependencies]` pins
- [ ] `CHANGELOG.md`: `[Unreleased]` moved under the new `## [<version>]` heading
- [ ] `cargo check --workspace` passes (catches a mismatched dependency pin)
- [ ] PR merged into `main`
- [ ] Tag is annotated, on the **merge commit on `main`**, and
      `git merge-base --is-ancestor "$(git rev-list -n1 v<version>)" origin/main`
      passes
- [ ] `cargo publish --workspace --locked --dry-run` clean, then published
- [ ] All four crates visible on crates.io at the new version
- [ ] **Verified from outside**: `cargo install rs-rich-cli --root $(mktemp -d)`,
      and a fresh project doing `cargo add rs-rich` + `use rich::…` compiles

## Notes

<!-- Anything unusual: a divergence introduced, an rc that needed a second round,
     a crate that failed partway through publishing (say which — versions are
     immutable and the re-run must skip them). -->
