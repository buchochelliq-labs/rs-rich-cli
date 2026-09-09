---
name: release
description: Cut and publish a release of the rs-rich crates. Use when the user says "cut a release", "ship 0.1.0", "tag a release candidate", "publish to crates.io", or asks to prepare a version.
---

# Cut and publish a release

Goal: get a version of all four crates onto crates.io, from a tag that provably
points at a commit on `main`. Read [docs/BRANCHING.md](../../../docs/BRANCHING.md)
first — the branch model is binding, and the invariant it protects is the reason
this procedure has the shape it does.

**Publishing is irreversible.** A crates.io version can be yanked but never
deleted or edited, and the name is claimed forever. Everything before step 5 is
cheap; step 5 is not.

## 0. Decide what you are cutting

Each crate owns its SemVer, independently of the Python projects and of the
other crates. The current **release train** is coordinated: a plain `vX.Y.Z` tag
selects all four crates, and all four manifests must say `X.Y.Z`. Do not use this
skill for a single-crate release; that requires a prior tag/workflow redesign.

Cargo's compatibility rule is narrower than "below 0.1 is lockstep": `0.0.2`
means `^0.0.2`, or `>=0.0.2,<0.0.3`. A bump of an internal dependency therefore
requires its direct dependents' requirements to change, but Cargo does not make
unrelated workspace packages bump. The coordinated release policy does.

Pick the number from what is in `## [Unreleased]` in `CHANGELOG.md`. If that
section is empty there is nothing to release — stop.

Cut a **release candidate first** whenever the publish pipeline itself has
changed, or when it has not run recently. An rc costs nothing and is the only way
to exercise tagging, credentials and the four-crate upload with a number nobody
depends on.

## 1. Confirm main is releasable

```bash
git fetch origin --prune && git switch main && git pull --ff-only
git status --porcelain                     # must be empty
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --all
python scripts/capture_golden.py && git diff --exit-code crates/rich/tests/golden
```

The fixture check matters: a release whose goldens drift is a release that no
longer matches upstream `rich`.

## 2. Cut the branch

```bash
git switch -c rc/0.1.0-rc.1 origin/main      # or release/0.1.0
```

Never from another branch, never from a stale `main`.

## 3. Make the single release commit

Exactly seven version sites, all in `Cargo.toml`:

- the `version` in each of the four `crates/*/Cargo.toml` manifests;
- the three `[workspace.dependencies]` requirements (`rich`, `rich-ext`, and
  `rich-art`) in the root manifest.

For `v0.0.3`, the per-crate decisions and exact edit inventory are recorded in
`docs/BRANCHING.md` under "Decision for v0.0.3". Update `Cargo.lock` too.

Then `CHANGELOG.md`: move everything under `## [Unreleased]` beneath a new
`## [X.Y.Z]` heading, leaving `[Unreleased]` empty.

Re-run `cargo check --workspace`. Then run the workflow's manifest audit locally;
it must prove that all four selected packages and all three requirements match
the tag. A path dependency can mask publication mistakes, so compilation alone
is not the release audit.

Check the docs don't contradict the new number (`README.md`, `docs/ARCHITECTURE.md`).

## 4. Land it, then tag on main

```bash
gh pr create --base main --title "Release 0.1.0-rc.1"
# after it merges:
git fetch origin --prune && git switch main && git pull --ff-only
git merge-base --is-ancestor HEAD origin/main    # must pass
git tag -a v0.1.0-rc.1 -m "0.1.0-rc.1"
git push origin v0.1.0-rc.1
```

The tag goes on `main`, never on the release branch. That is what makes "every
published artifact is on `main`" checkable rather than aspirational.

## 5. Publish

```bash
cargo publish --workspace --locked --dry-run    # always first
cargo publish --workspace --locked
```

Do **not** write an ordering script. Cargo derives the topological order from the
dependency graph and cross-verifies dependents against sibling tarballs. The order
it picks here is `rs-rich` → `rs-rich-art` → `rs-rich-ext` → `rs-rich-cli`.

The workflow first checks all four package/version pairs on crates.io and aborts
if even one already exists. This prevents publishing an unchanged crate and
prevents treating a partial prior upload as a coherent release. If an upload
fails partway, versions already uploaded stay uploaded and the coordinated tag
cannot simply be rerun; inspect every crate and make an explicit recovery plan.
Check a version with:

```bash
curl -s -H 'User-Agent: rs-rich-release' https://crates.io/api/v1/crates/rs-rich/0.1.0 \
  | head -c 80
```

A missing User-Agent gets a 403, which is easy to misread as "not published".

## 6. Verify from outside

Do not trust the upload's own output. Install from crates.io into a clean prefix
and use it as a stranger would:

```bash
cargo install rs-rich-cli --root "$(mktemp -d)" --locked
cargo new /tmp/verify && cd /tmp/verify && cargo add rs-rich
# then confirm `use rich::…` compiles — the package is rs-rich, the lib is rich
```

This step has already caught two real bugs that the whole test suite missed (the
`highlight` default, and `--export-html` taking no path). It is not ceremony.

## Done when

- `main` is green and the fixtures regenerate byte-identically.
- The tag is annotated, named `vX.Y.Z` (or `vX.Y.Z-rc.N`), and
  `git merge-base --is-ancestor "$(git rev-list -n1 <tag>)" origin/main` passes.
- All four crates are on crates.io at the new version.
- A clean-room `cargo install` and a fresh consumer project both work.
- `CHANGELOG.md` has the released section and an empty `## [Unreleased]`.
