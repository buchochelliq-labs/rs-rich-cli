---
name: Release
about: Track cutting and publishing a version
title: "release: <tag>"
labels: ["type:release", "type:infra"]
---

## Target

- Tag: ______ (`vX.Y.Z` selects all four; `<crate>-vX.Y.Z` selects one)
- Selected packages and versions: ______
- Kind: release candidate / final (delete one)
- Previous published version of each selected package: ______

## Checklist (see the `release` skill and docs/BRANCHING.md)

- [ ] `main` is green; `## [Unreleased]` names the affected crates
- [ ] Goldens regenerate unchanged against the exact library pin in `UPSTREAM.toml`
- [ ] Release branch contains fresh `origin/main`; all required checks pass
- [ ] Each changed crate has its own version bump; every internal requirement
      matches its target crate; `Cargo.lock` is refreshed
- [ ] `python3 scripts/release.py plan <tag>` selects exactly the intended packages
- [ ] Generated manifest-version tables and CLI reference match this checkout
- [ ] `CHANGELOG.md`: selected changes moved under the new release heading
- [ ] `cargo check --workspace --locked` passes
- [ ] PR merged into `main`
- [ ] Annotated tag points at the merge commit on `main`; ancestry check passes
- [ ] Workflow preflight finds no selected version already published
- [ ] Workflow dry run and publish use identical selection (`--workspace` or `-p <crate>`, with `--locked`)
- [ ] Each selected version is visible on crates.io
- [ ] Workflow verification installs the selected CLI with `--version =X.Y.Z --locked`
      and checks a fresh consumer with an exact registry dependency for each selected library

## Notes

<!-- For partial uploads, list every package/version already published. Versions
     are immutable and preflight rejects a blind rerun. Inspect registry state
     and agree an explicit recovery plan before any further upload. -->
