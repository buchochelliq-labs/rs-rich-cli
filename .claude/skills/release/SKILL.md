---
name: release
description: Cut and publish a release of the rs-rich crates. Use when the user says "cut a release", "ship 0.1.0", "tag a release candidate", "publish to crates.io", or asks to prepare a version.
---

# Prepare and publish a release

Read `docs/BRANCHING.md` from the repository root first. Every published tag
must point at a commit on `main`. Publishing is irreversible; preparation alone
does not authorize creating tags or uploading packages.

## 0. Select the packages

Every crate owns its independent SemVer. A release is every package of
`RELEASES.toml` on one commit of `main`, named by `rs-rich-cli`'s version;
one run publishes all of it that is not yet on crates.io:

| Tag | Scope |
|---|---|
| `release-X.Y.Z` | Every crate of release X.Y.Z not yet on crates.io, in one `release.yml` run |
| `<crate>-vX.Y.Z` | A record of that crate in its release (starts nothing); dispatched by hand, that crate alone |
| `python-vX.Y.Z` | The Python bindings on PyPI (`pypi-release.yml`) |

`RELEASES.toml` holds the publication order (each crate after the workspace
crates it depends on; `release_cohort.py check` enforces it), and the
release run uploads in that order. New crates may be in the same release as
existing ones: their first versions upload with the environment's token, in
separate `cargo publish` runs in dependency order. Versions accept
prereleases such as `-rc.1`. Manual workflow dispatch takes an existing tag,
never a branch name.

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
Run the complete CI gate. Prefer the serialized wrapper for release prep,
especially on Windows where parallel Cargo commands can lock the same binary:

```bash
python scripts/validate_release.py --tag release-0.0.20
```

`--tag` is required because the wrapper always includes
`scripts/release.py plan <tag>` as the final release-plan audit. If you run the
steps manually, keep them in the wrapper's order and run both explicit Python
release test modules: `test_release.py` and `test_release_readiness.py`.

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
Re-run `cargo check --workspace --locked`, then
`python3 scripts/release_cohort.py sync` so `RELEASES.toml` carries the new
versions (`release_cohort.py check` fails CI until it does). Audit the intended
tag locally:

```bash
python3 scripts/release.py plan release-0.0.20     # every crate of the release
# or: python3 scripts/release.py plan rs-rich-cli-v0.0.20
```

These commands plan only; they do not create a tag or publish. Confirm the JSON
selection, then regenerate version docs and CLI help. Move only the selected
changes from `Unreleased` under the appropriate release heading when finalizing
the release. Record tests and actual screenshot evidence in the PR.

Screenshots, the guided tour and every other terminal recording come from
tapes (`docs/tapes/*.tape`) recorded by `rich record`, never from ad hoc
scripts. Add a tape for each new user-visible command, then regenerate with the
release build and share the media:

```bash
cargo build --release -p rs-rich-cli -p rs-rich-interact -p rs-rich-intuituive --bins --examples --locked
target/release/rich record --bin-dir target/release --output docs/media/tapes docs/tapes/*.tape
target/release/rich record --check --bin-dir target/release --output docs/media/tapes docs/tapes/*.tape
```

Before calling the PR ready, take and report the final readiness snapshot:

```bash
gh pr checks <number> --watch --interval 15
gh pr view <number> --json headRefOid,mergeable,mergeStateStatus,reviewDecision,statusCheckRollup
gh api graphql -f owner=<owner> -f name=<repo> -F number=<number> \
  -f query='query($owner:String!,$name:String!,$number:Int!,$cursor:String){ repository(owner:$owner,name:$name){ pullRequest(number:$number){ reviewThreads(first:100, after:$cursor){ nodes{ isResolved } pageInfo{ hasNextPage endCursor } } } } }' \
  --jq '.data.repository.pullRequest.reviewThreads.nodes | map(select(.isResolved == false)) | length'
git status --short --branch
```

Ready means all expected checks are green, unresolved review thread count is `0`,
latest review comments were inspected, the worktree is clean, and the PR's
`headRefOid`, `mergeable`, `mergeStateStatus`, and `reviewDecision` are recorded.
If `mergeable` is `MERGEABLE` but `mergeStateStatus` is `BLOCKED`, state the
visible branch-protection blocker instead of calling it fully merge-ready.

Before calling a PR ready, also read every review *summary* (the review body), not
just inline threads: a bot finding written in a review body is not a thread, so
the readiness job's unresolved-thread count does not see it. Verify each such
finding and record its disposition on the PR.

Post a release handoff note on the PR before stopping. Use the centralized
policy in `.github/release-readiness.json`; CI requires these field labels:
`Head SHA:`, `Mergeable:`, `Merge state:`, `Review decision:`,
`Selected publish tag`, `Publish target:`,
`Validation summary:`, `Unresolved review threads:`, and
`Remaining visible blocker:`. Spell out the release tag, e.g.
`release-0.0.20`, and the crates it will publish (`release_cohort.py tag
--dry-run`).

The same policy file owns the handoff triggers and
`handoffCommentWaitSeconds`. Post the current-SHA handoff as soon as possible
after pushing; the readiness job waits briefly for the comment to appear, but
the wait is only a race cushion, not a substitute for a real handoff. If the
head changes after the handoff, post a fresh handoff and wait for CI again.

On Windows, do not run Cargo commands that build the same binary in parallel
against the same `target` directory; a concurrent process can hold
`target\debug\*.exe` and cause `Access is denied`. Prefer the serialized release
wrapper:

```bash
python scripts/validate_release.py --tag rs-rich-cli-v0.0.6
```

For ad-hoc parallel validation, give jobs separate `CARGO_TARGET_DIR` values.
Keep release packaging and locked workspace checks on the normal workspace
target unless there is a specific reason not to.

## 3. Land on main, then tag

After the release PR merges, check out `main` at the exact intended commit
(`git switch main && git pull --ff-only`). `tag` releases the local checkout and
fetches nothing, so a resumed run tags the same commit even after `main`
moves. Only with authorization to publish, tag it from `RELEASES.toml`:

```bash
python3 scripts/release_cohort.py tag --dry-run   # the plan
python3 scripts/release_cohort.py tag             # tag, push, wait for the runs
```

It never tags a version already on crates.io or PyPI. It creates an
annotated record tag for every crate due and pushes them in one atomic push,
then pushes `release-<name>` (and `python-v…` when PyPI is due) in a second
one: GitHub starts no workflow for a push of more than three tags. Then it
waits for the release run and the PyPI run (see `docs/BRANCHING.md`, "Release
numbers and tagging"). Every tag must be on that commit, on main:

```bash
git merge-base --is-ancestor "$(git rev-list -n1 <tag>)" origin/main
```

The release workflow rejects lightweight tags and checks that the annotated
tag peels to the checked-out commit on main. It computes the selection, passes
the exact commit SHA to the complete CI gate once, and publishes from the
protected `crates-io` environment once. Do not bypass that workflow by
publishing separately from a local checkout. A crate's **first** version
cannot use Trusted Publishing (crates.io offers it only on an existing crate):
the workflow uploads new crates with the `crates-io` environment's
`CARGO_REGISTRY_TOKEN` secret, in the same run, and fails before uploading if
the secret is missing. Then add the crate's Trusted Publishing entry. See
`docs/BRANCHING.md`, "Registry authentication".

Release 0.0.19's commit predates release tags, and `tag` refuses to push one
there: finish it with the scripts of main before one-run releases (see
`docs/BRANCHING.md`, "Release 0.0.19").

## 4. Observe publication and verification

Publication uses crates.io Trusted Publishing (see `docs/BRANCHING.md`,
"Registry authentication"); every existing crate must have a publisher entry
for this repository, `release.yml` and the `crates-io` environment.

The publish job checks each selected package/version on crates.io and aborts
if one exists or the registry response is unexpected. It runs one
`cargo publish -p a -p b … --locked --dry-run` for the whole selection, then
uploads with `cargo publish -p … --locked`, one command per run of crates
sharing a credential. Cargo orders each run by dependency and waits for each
crate on the index; do not introduce another ordering script.

If the run fails, re-run `python3 scripts/release_cohort.py tag` (after
merging any fix to `main` and pulling, when nothing was published). It pushes
the release tag again, and the new run's gate selects only what is still
unpublished, so a partial upload is resumed, not repeated. A fix to
`release.yml` itself needs a dispatch from main
(`gh workflow run release.yml --ref main -f tag=release-X.Y.Z`), since a tag
push runs the tagged commit's workflow. See `docs/BRANCHING.md`, "Re-running
a release". To publish one crate alone, dispatch its crate tag.

If every selected version uploaded but consumer verification failed, manually
dispatch the same existing tag with `verify_only: true`. This retains the tag,
ancestry, exact-SHA and full-CI gates, skips all upload steps, and retries registry
verification in the protected release job. It cannot complete a partial upload.
Normal tag pushes and manual dispatches default to publication and retain preflight.

The workflow waits for every selected version to appear, then verifies outside
the checkout, once for the whole selection. It installs a selected CLI using
`--version =X.Y.Z --locked` into a fresh prefix, and compiles a fresh consumer
with an exact registry dependency for each selected library. Unselected
packages are not installed or verified as though they changed. Do not
substitute a floating `cargo install` or `cargo add`.

## Done when

- The complete CI and parity gates passed on the tagged commit on `main`.
- Every selected package/version is published and exact-version verification passed.
- The changelog describes the versions actually released; remaining changes stay
  under `Unreleased`.
