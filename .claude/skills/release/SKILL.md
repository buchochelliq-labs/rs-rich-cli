---
name: release
description: Cut and publish a release of the rs-rich crates. Use when the user says "cut a release", "ship 0.1.0", "tag a release candidate", "publish to crates.io", or asks to prepare a version.
---

# Prepare and publish a release

Read `docs/BRANCHING.md` from the repository root first. Every published tag
must point at a commit on `main`. Publishing is irreversible; preparation alone
does not authorize creating tags or uploading packages.

## 0. Select the packages

Releases run on rab, the Release Asset Bundle (formerly release-kit; v2.1.1,
installed under `.github/rab/`; `docs/BRANCHING.md`, "Which path is current";
docs at https://buchochelliq-labs.github.io/release-asset-bundle/). Every crate owns its
independent SemVer. A release is every package of `rab.yml` (the
crates, then the PyPI `rs-rich`) on one commit of `main`, named by
`rs-rich-cli`'s version; one `release.yml` run publishes all of it that is
not yet on its registry:

| Tag | Scope |
|---|---|
| `release-X.Y.Z` | Every package of release X.Y.Z not yet published, crates.io and PyPI, in one `release.yml` run |
| `<crate>-vX.Y.Z` | A record of that crate in its release (starts nothing) |
| `python-vX.Y.Z` | A record of the PyPI `rs-rich` in its release (starts nothing) |

`rab.yml` holds the publication order (each crate after the workspace
crates it depends on; `python3 .github/rab/scripts/release.py check` enforces it), and the release run uploads in
that order. New crates may be in the same release as existing ones: their
first versions upload with the environment's token. Versions accept
prereleases such as `-rc.1`. Manual workflow dispatch takes an existing
release tag, never a branch name. `RELEASES.toml` stays in step with
`rab.yml` until the kit has published a release (CI checks both).

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
python scripts/validate_release.py --tag release-0.0.25
```

`--tag` is required because the wrapper always includes
`scripts/release.py plan <tag> --semver-checks` as the final release-plan audit
(for a crate past 0.0.x it runs `cargo-semver-checks`, pinned in
`api/tools.toml`). If you run the
steps manually, keep them in the wrapper's order and run both explicit Python
release test modules: `test_release.py` and `test_release_readiness.py`.
Then rehearse the release with the kit: `python3 scripts/release_rehearsal.py`
runs `check`, `vendored`, `plan` and `tag --dry-run` (against a throwaway
remote holding `HEAD` and `origin`'s tags; nothing is pushed).

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
Re-run `cargo check --workspace --locked`, then `python3 .github/rab/scripts/release.py sync` and
`python3 scripts/release_cohort.py sync` so `rab.yml` and
`RELEASES.toml` carry the new versions (each `check` fails CI until it does).
Audit the intended release locally:

```bash
python3 .github/rab/scripts/release.py status            # what is published, tagged, due
python3 scripts/release_rehearsal.py                     # check, vendored, plan, tag --dry-run
python3 scripts/release.py plan release-0.0.25 --semver-checks   # as release.yml's semver job runs it
```

`--semver-checks` also refuses a version bump smaller than the change, by
Cargo's rules, with `cargo-semver-checks` (pinned in `api/tools.toml`) against
each crate's last release. Every 0.0.x bump counts as breaking, so it fits any
change; from 0.1.0 a breaking change needs a new minor version. Before the
release PR, check that CI's `public API and SemVer` job is green: the API
snapshots in `api/` match, and every 0.0.x break is listed in
`api/semver-breaks.toml` and has a migration note. After the release, the
entries for the published versions are spent: remove them.

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

Build both docs sites strictly with the pinned Zensical (`requirements-docs.txt`,
as the docs job does), and check the built pages' raw-HTML URLs:

```bash
pip install -r requirements-docs.txt
zensical build --strict -f mkdocs.yml
zensical build --strict -f mkdocs-python.yml
python scripts/check_site_links.py site
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
`release-0.0.25`, and the packages it will publish (`python3 .github/rab/scripts/release.py tag
--dry-run` on `main`, or the rehearsal before the merge).

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
(`git switch main && git pull --ff-only`). `tag` releases the local checkout
(or a release already tagged and unfinished), and the commit must be on
`origin/main` as last fetched. Only with authorization to publish:

```bash
python3 .github/rab/scripts/release.py tag --dry-run   # skip, keep, tag, move or wait, per package
python3 .github/rab/scripts/release.py tag             # tag, push, wait for the run (up to three hours)
```

It never tags a version already on its registry. It writes an annotated
record tag per package due (`<name> <version> (<registry>)`, with the
release's `Release:` and `Commit:` lines), pushes them in one atomic push, then
pushes `release-<name>` on its own (GitHub starts no workflow for a push of
more than three tags), and waits until the run has succeeded and every package
is on its registry. It refuses a package tag on the remote that is not that
package's record, a `rab.yml` that fails `check`, and a split release
(a package's `paths` changed under a version tagged on another commit). Tags
from before the kit (up to the 0.0.24 cohort) all name published versions,
so the kit never selects or moves them (`docs/BRANCHING.md`, "Tags from
before release-kit").

The release run gates the tag once (the kit's `gate`: annotated, on `main`,
`check` passes at the commit, not split, the selection is what is due), runs
the complete CI gate once on the exact commit, builds and tests the wheels,
runs this repository's `semver` job (`scripts/release.py plan
--semver-checks`), and publishes from the protected `crates-io` and `pypi`
environments. Do not bypass it by publishing from a local checkout. A crate's
**first** version cannot use Trusted Publishing: the kit uploads it with the
`crates-io` environment's `CARGO_REGISTRY_TOKEN` secret in the same run, and
refuses before uploading anything if the secret is missing. Then add the
crate's Trusted Publishing entry. See `docs/BRANCHING.md`, "Registry
authentication".

## 4. Observe publication and verification

crates.io and PyPI use Trusted Publishing: every existing crate needs a
publisher entry for this repository, `release.yml` and the `crates-io`
environment; PyPI's `rs-rich` one for `release.yml` and the `pypi`
environment.

`publish-crates` re-checks each selected crate/version on crates.io and
aborts if one exists, runs one `cargo publish --dry-run --locked -p a -p b …`
for the whole selection, then uploads one `cargo publish --locked` per run of
crates sharing a credential, in order. `publish-pypi` uploads the tested
wheels and the sdist. Each then runs `verify` (every version served) and
`consume` (an exact-version `cargo install --locked` of each binary, a fresh
consumer crate for each library, `pip install` in a new virtualenv); do not
substitute a floating `cargo install` or `cargo add`. `publish-playground`
attaches the docs playground's module to the GitHub release afterwards. The
last job, `record`, runs whatever happened before it and uploads the run's
release record (`gh run download <run> -n release-record`: each job's result
and each package's outcome); check its `result` is `complete`.

If the run fails: re-run, don't repair. Fix the cause (merged to `main` and
pulled when nothing was published; outside the tagged commit when something
was), then `python3 .github/rab/scripts/release.py tag --dry-run` and `tag` again. A run that published nothing
has its tags moved to the fix; one that published some keeps its commit, and
the new run publishes only what is still due. Never move or delete tags by
hand, and never "Re-run jobs" on a publish job. A fix to `release.yml` itself
needs a dispatch from main (`gh workflow run release.yml --ref main -f
tag=release-X.Y.Z`). If PyPI stopped partway, upload the missing files from
the run's artifacts by hand. See `docs/BRANCHING.md`, "Re-running a release".

If everything uploaded but `verify` or `consume` failed, dispatch the same
release tag with `verify_only: true` (the gate's `--verify-only`): it keeps
the gate and CI, skips the build and every upload, and verifies again. It
cannot complete a partial upload; `tag` does.

## Done when

- The complete CI and parity gates passed on the tagged commit on `main`.
- Every selected package/version is published and exact-version verification passed.
- The changelog describes the versions actually released; remaining changes stay
  under `Unreleased`.
