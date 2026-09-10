---
name: release
description: Cut and publish a release of the rs-rich crates. Use when the user says "cut a release", "ship 0.1.0", "tag a release candidate", "publish to crates.io", or asks to prepare a version.
---

# Prepare and publish a release

Read `docs/BRANCHING.md` from the repository root first. Every published tag
must point at a commit on `main`. Publishing is irreversible; preparation alone
does not authorize creating tags or uploading packages.

## 0. Select the packages

Every crate owns its independent SemVer. Choose a tag form explicitly:

| Tag | Scope |
|---|---|
| `vX.Y.Z` | All four crates; all manifests must match X.Y.Z |
| `<crate>-vX.Y.Z` | Only that package; its manifest must match X.Y.Z |

Supported packages: `rs-rich`, `rs-rich-ext`, `rs-rich-cli`, `rs-rich-art`.
Both forms accept prereleases such as `-rc.1`. Manual workflow dispatch takes
an existing tag, never a branch name.

Choose versions from the non-empty `Unreleased` changelog. Cargo's `^0.0.2`
means `>=0.0.2,<0.0.3`: an internal dependency bump requires updating dependent
requirements and publishing changed dependent manifests. Unrelated packages
need not bump. Unselected dependencies must already exist in the registry.

For the prepared 0.0.3 snapshot, see `docs/BRANCHING.md` under "Prepared 0.0.3
snapshot". The core Markdown fix and dependency closure put all four at 0.0.3;
this does not remove independent release support. A branch named `*-rc` does
not change manifest versions to prereleases. If publishing an RC, choose actual
prerelease versions and update the dependency requirements accordingly.

## 1. Verify the source

Start from fresh `origin/main`, or update the selected integration branch by PR
so it contains `origin/main`. Keep the checkout clean before release operations.
Run the complete CI gate:

```bash
cargo fmt --all --check
cargo clippy --all-targets -- -D warnings
cargo test --all
python3 -m unittest discover -s scripts -p 'test_release*.py' -v
python3 scripts/gen_versions.py --check
cargo build -p rs-rich-cli --locked
python3 scripts/gen_cli_reference.py --binary target/debug/rich --check
```

Also require the CI feature matrix and declared MSRV check. In a dedicated
Python environment, install `rich==$(python3 scripts/read_upstream_version.py)`;
never install rich-cli there. Regenerate with a deterministic terminal environment:

```bash
env -u NO_COLOR TERM=xterm-256color PYTHONUTF8=1 python scripts/capture_golden.py
git diff --exit-code crates/rich/tests/golden
```

The fixture diff must be clean. Run the Rust tests with `NO_COLOR` removed too;
ambient color suppression invalidates tests that assert ANSI output.

## 2. Prepare the release PR

Update each selected manifest version, every affected internal requirement in
the root `Cargo.toml`, and `Cargo.lock`. Do not invent a workspace package version.
Re-run `cargo check --workspace --locked`. Audit the intended tag locally:

```bash
python3 scripts/release.py plan v0.0.3
# or: python3 scripts/release.py plan rs-rich-cli-v0.0.3
```

These commands plan only; they do not create a tag or publish. Confirm the JSON
selection, then regenerate version docs and CLI help. Move only the selected
changes from `Unreleased` under the appropriate release heading when finalizing
the release. Record tests and actual screenshot evidence in the PR.

## 3. Land on main, then tag

After the release PR merges, fetch `origin/main` and verify the exact intended
commit. Only with authorization to publish, create and push the annotated tag
on that commit. Require:

```bash
git merge-base --is-ancestor "$(git rev-list -n1 <tag>)" origin/main
```

The release workflow rejects lightweight tags and checks that the annotated
tag peels to the checked-out commit on main. It validates the selection, passes the exact commit
SHA to the complete CI gate, and uses the protected `crates-io` environment.
Do not bypass that workflow by publishing separately from a local checkout.

## 4. Observe publication and verification

The workflow checks each selected package/version on crates.io and aborts if
one exists or the registry response is unexpected. It runs a dry run, then
publishes with identical scope and `--locked`: `--workspace` for a coordinated
tag, or `-p <crate>` for an independent tag. Cargo handles dependency order and
sibling tarballs; do not introduce a manual ordering script. For separate tags
with dependencies, finish the dependency's release and verification first.

Uploads are serialized. A partial upload requires inspecting all selected
versions and an explicit recovery plan. Versions already uploaded are immutable;
preflight deliberately rejects a blind rerun and does not silently skip them.

If every selected version uploaded but consumer verification failed, manually
dispatch the same existing tag with `verify_only: true`. This retains the tag,
ancestry, exact-SHA and full-CI gates, skips all upload steps, and retries registry
verification in the protected release job. It cannot complete a partial upload.
Normal tag pushes and manual dispatches default to publication and retain preflight.

The workflow waits for every selected version to appear, then verifies outside
the checkout. It installs a selected CLI using `--version =X.Y.Z --locked` into
a fresh prefix, and compiles a fresh consumer with an exact registry dependency
for each selected library. Unselected packages are not installed or verified as
though they changed. Do not substitute a floating `cargo install` or `cargo add`.

## Done when

- The complete CI and parity gates passed on the tagged commit on `main`.
- Every selected package/version is published and exact-version verification passed.
- The changelog describes the versions actually released; remaining changes stay
  under `Unreleased`.
