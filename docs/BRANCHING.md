# Branching and releases

The short version: **`main` is the only long-lived branch, every PR targets it,
and every published tag points at a commit on `main`.**

Everything below exists to keep that last sentence true, because it is the one
property that makes "did this actually ship?" answerable by a machine.

## Why this shape

This repo has lost work twice. Both times the work was *merged* by GitHub's
account and absent from `main`:

1. **PR #23** — commits were pushed onto the branch *after* its PR merged. The
   branch still existed, so the push succeeded and went nowhere.
2. **PR #30** — based on `fix/justify-full` (PR #29's branch). #29 squash-merged
   to `main`; eighteen seconds later #30 merged into `fix/justify-full`, a branch
   nothing pulls from. GitHub reported "merged". The terminal-theme presets were
   silently lost and had to be recovered later.

Both share a shape: **a second long-lived head that isn't `main`.** So the model
refuses to create one. There are no release lines to back-merge, no `develop`, and
release branches live for minutes.

The usual Git Flow move — tag the release branch, then merge it back — is
deliberately inverted here. Release branches merge into `main` *first*, and the
tag goes on the resulting commit on `main`. That makes this a true invariant:

```bash
git merge-base --is-ancestor "$(git rev-list -n1 v0.0.1)" origin/main
```

If that ever fails, something was published that isn't on `main`.

## Branches

| pattern | cut from | merges to | lifetime |
|---|---|---|---|
| `main` | — | — | permanent, protected |
| `feat/<slug>` · `fix/<slug>` · `chore/<slug>` · `docs/<slug>` | fresh `origin/main` | `main` via PR | one PR |
| `port/<module>` | fresh `origin/main` | `main` via PR | one PR — the `port-module` skill |
| `sync/rich-<version>` | fresh `origin/main` | `main` via PR | one PR — the `sync-upstream` skill |
| `rc/<X.Y.Z>` | fresh `origin/main` | `main` via PR, then tag on `main` | a release cycle |
| `release/<X.Y.Z>` or `releases/<X.Y.Z>` | fresh `origin/main` | `main` via PR, then tag `vX.Y.Z` on `main` | minutes |
| `hotfix/<X.Y.Z>` | the tag `vX.Y.(Z-1)` | nothing — tagged in place, forward-ported to `main` by PR | until forward-ported |

A merged branch is **dead**. It is auto-deleted, and pushing to it is a bug, not
a shortcut. If you need to add to merged work, cut a new branch from `origin/main`.

Enable the local guard once per clone — server-side deletion only helps for
branches merged *after* it was switched on, and only once you've fetched:

```bash
git config core.hooksPath .githooks
```

It refuses any push to a branch whose PR has already merged. This is not
hypothetical: it was written immediately after two commits were pushed onto
PR #31's branch minutes after that PR merged, stranding them exactly as #23 and
PR #30 were stranded.

### Release-candidate branches

`rc/<X.Y.Z>` is an **integration branch**: ordinary work targets it during a
release cycle, and it merges to `main` when the cycle closes. Tags are still only
ever placed on `main`, so the ancestor invariant above still holds.

This is a deliberate exception to "everything targets `main`", and it reintroduces
the one thing the rest of this document exists to avoid — a second long-lived
head. It is safe **only** while the rc stays a fast-forward of `main`. The moment
`main` moves ahead, merging the rc either conflicts or silently reverts, and a
release cut from it no longer contains what is on `main`.

CI enforces that: the `rc not behind main` check applies to `rc/*`, `release/*`,
and `releases/*` (including `releases/v0.0.3-rc`). It tests the proposed merge
result, so a PR bringing in current `main` can pass. Keep it current with:

```bash
git fetch origin
git switch -c fix/refresh-release origin/releases/v0.0.3-rc
git merge origin/main
# Open a PR into releases/v0.0.3-rc.
```

#### Protection

The ruleset described below covers `rc/*`. Accepting `release/*` and `releases/*`
in workflow checks does not extend that ruleset; verify server-side protections
before creating another integration branch.

`rc/*` is covered by its own ruleset (**"rc branches"**), matching `main`'s
protection rather than being the soft underbelly of the release process:

| rule | effect |
|---|---|
| `creation` | only repository **admins** may create an `rc/*` branch |
| `deletion` | it cannot be deleted |
| `non_fast_forward` | it cannot be force-pushed |
| `pull_request` | changes land by PR (0 approvals — solo maintainer) |
| `required_status_checks` | `ci-ok`, `pr body`, `base branch`, `rc not behind main`, strict |

The required checks are deliberately **not** the same list as `main`'s. `main`
additionally requires the CodeQL `Analyze (…)` contexts, and those do not run on
PRs into `rc/*` — requiring them would leave every rc PR blocked forever on a
check that can never report. Only require a context you have watched report on a
PR against that specific base.

**An rc branch must be cut from `main`, and GitHub cannot enforce it.** No
protection rule or ruleset constrains the commit a branch is created *from*. The
`rc branch guard` workflow fires on branch creation and fails if the new `rc/*`
branch does not contain `origin/main` — a detector, not a gate, since the branch
already exists by then. It pairs with `rc not behind main`, which gates drift
afterwards.

```bash
git fetch origin --prune
git switch -c rc/0.0.3 origin/main    # from main, always
```

### Stacked branches

Opening a PR based on another PR's branch is fine. **Merging it while stacked is
not** — that is incident 2 exactly. Retarget before merge:

```bash
gh pr edit <number> --base main
```

CI enforces this: the `base branch` check fails any PR not targeting `main` or
`rc/*`, `release/*`, or `releases/*`.

## Releases

There are two separate decisions here:

1. **Each crate owns its SemVer.** A number describes that crate's Rust API and
   contents; it is never copied from either Python upstream. A change in one
   crate does not, by policy alone, require an unrelated crate's version to
   change.
2. **One release, one run.** A release is every package of a `RELEASES.toml`
   on one commit of `main`, named by the version of its last crate
   (`rs-rich-cli`). Its release tag, `release-X.Y.Z`, starts one
   `release.yml` run that checks it once, runs CI once and publishes **every
   crate of it not yet on crates.io** in one job. Each published crate also
   has its own annotated `<crate>-vX.Y.Z` tag on the same commit: a record,
   which starts nothing. Unchanged crates keep their versions, are already on
   crates.io, and are neither published nor verified again.

| Tag | What it does |
|---|---|
| `release-0.0.20` | Pushed: publishes every crate of release 0.0.20 (rs-rich-cli 0.0.20's `RELEASES.toml`) not yet on crates.io |
| `rs-rich-ext-v0.0.18` | A record of that crate in its release; pushing it starts nothing. Dispatched by hand: publishes that crate alone |
| `python-v0.0.8` | Pushed: publishes the Python bindings to PyPI (`pypi-release.yml`) |
| `vX.Y.Z` | Historical (every crate at one version, until 0.0.3); dispatch only |

Versions accept SemVer prereleases, for example `release-0.0.20-rc.1` or
`rs-rich-cli-v0.0.20-rc.1`. Manual dispatch takes an **existing tag** in
one of these forms, never a branch name. Every job checks out the validated
tag's commit SHA, including the reusable CI gate. Tags belong on `main` after
merging the release changes; this does not relax the ancestry rule.

All workspace requirements are checked against their respective crate
versions, for every crate selected. Refresh `Cargo.lock` whenever manifests
or dependency requirements change. A dependency outside the selection must
already be on crates.io: it is, since the selection is everything of the
release that is not.

### What Cargo means by `0.0.x`

The workspace dependency spelling `version = "0.0.2"` is Cargo shorthand for
the caret requirement `^0.0.2`. Cargo permits versions `>=0.0.2,<0.0.3`: for a
`0.0.x` requirement, changing the patch component is incompatible. A path
dependency can use the local package while developing, but its version must
still satisfy that requirement, and the requirement is what consumers see in
the published package. Thus bumping `rs-rich` from `0.0.2` to `0.0.3` requires
updating the requirements used by its direct dependents (`rs-rich-plugin-api`,
`rs-rich-macros`, `rs-rich-ext`, `rs-rich-art`, and `rs-rich-cli`); bumping
`rs-rich-plugin-api` or `rs-rich-macros` requires updating `rs-rich-ext`; bumping `rs-rich-ext` or `rs-rich-art`
requires updating `rs-rich-cli`. Cargo does **not** force unrelated crates to
share a version. Our coordinated-tag policy does.

### Released 0.0.4 snapshot

All four packages published at 0.0.4 because each changed or consumed the new
core version. Core adds an ownership extension point, wrapping improvements and
an optional syntax cache; ext adds explicit text decoding; art adds GIF
half-block rendering; the CLI wires these options and reduces CSV copies.

The annotated `v0.0.4` tag selects all four and points to main commit
`355a333b853606fcff830f498db1c35b9f94061d`. The
[release workflow](https://github.com/buchochelliq-labs/rs-rich-cli/actions/runs/34451499873) passed source, package and exact-version
registry-consumer verification. See [release notes](releases/0.0.4.md).

The [0.0.5 preparation plan](plans/0.0.5.md) does not preselect package versions.
Assess each changed crate and its dependency closure before choosing coordinated
or per-crate tags. Never repeat the upload path for an already published version.

### Prepared 0.0.3 snapshot

This preparation includes the Markdown fix from `main`, requiring core 0.0.3.
The ext package also changes its core dependency and must publish a new version.
Together with the prepared CLI and art versions, all four manifests in that
snapshot said 0.0.3. This is the dependency closure for this release, not a lockstep policy.
The selected coordinated tag `v0.0.3` selects all four:

| package | decision | manifest change | internal requirement change |
|---|---|---|---|
| `rs-rich` | publish `0.0.3` | `crates/rich/Cargo.toml`: `0.0.2` → `0.0.3` | root `rich` requirement: `0.0.2` → `0.0.3` |
| `rs-rich-ext` | publish `0.0.3` | `crates/rich-ext/Cargo.toml`: `0.0.2` → `0.0.3` | root `rich-ext` requirement: `0.0.2` → `0.0.3`; it consumes the updated root `rich` requirement |
| `rs-rich-art` | publish `0.0.3` | `crates/rich-art/Cargo.toml`: `0.0.2` → `0.0.3` | root `rich-art` requirement: `0.0.2` → `0.0.3`; it consumes the updated root `rich` requirement |
| `rs-rich-cli` | publish `0.0.3` | `crates/rich-cli/Cargo.toml`: `0.0.2` → `0.0.3` | it consumes all three updated root requirements |

The manifest and lockfile updates are finalized, and selected changes are under
the 0.0.3 changelog heading. `Unreleased` is reserved for subsequent work. Regenerate
version tables with `python3 scripts/gen_versions.py` and CLI help with
`python3 scripts/gen_cli_reference.py` after building the release binary. CI
checks both generated documents. No tag or registry upload is created by preparation.

This coordinated option is separate from the independent CLI/art tags above.
Do not publish a crate at `0.0.3` independently and then expect a coordinated
`v0.0.3` to skip it: the already-published-version guard deliberately rejects
that mixed attempt. Choose the release scope before tagging.

Tags are annotated: `vX.Y.Z` or `<crate>-vX.Y.Z` for releases, with `-rc.N`
appended for candidates.

The full procedure lives in the **`release` skill** (`.claude/skills/release/`).
In outline:

1. `main` is green and `CHANGELOG.md` has content under `## [Unreleased]`.
2. Cut `rc/X.Y.Z-rc.1`, make the single version+changelog commit, PR it, merge.
3. Tag `vX.Y.Z-rc.N` on the merge commit **on `main`**; the release workflow
   publishes it.
4. Soak. Fixes land on `main` as ordinary PRs, then cut another rc.
5. Cut `release/X.Y.Z`, same shape, merge, tag `vX.Y.Z`.

### Final PR readiness gate

Green CI is necessary, but it is not enough. Before saying a release PR is ready,
record a final readiness snapshot that proves review feedback and branch
protection are accounted for:

```bash
gh pr checks <number> --watch --interval 15
gh pr view <number> --json headRefOid,mergeable,mergeStateStatus,reviewDecision,statusCheckRollup
gh api graphql -f owner=<owner> -f name=<repo> -F number=<number> \
  -f query='query($owner:String!,$name:String!,$number:Int!,$cursor:String){ repository(owner:$owner,name:$name){ pullRequest(number:$number){ reviewThreads(first:100, after:$cursor){ nodes{ isResolved } pageInfo{ hasNextPage endCursor } } } } }' \
  --jq '.data.repository.pullRequest.reviewThreads.nodes | map(select(.isResolved == false)) | length'
git status --short --branch
```

The ready state is: every required/expected check is green, unresolved review
thread count is `0`, latest review comments have been inspected, the worktree is
clean, and the PR's `headRefOid`, `mergeable`, `mergeStateStatus`, and
`reviewDecision` are written into the handoff. If `mergeable` is `MERGEABLE` but
`mergeStateStatus` is `BLOCKED`, say exactly which external branch-protection
requirement remains, or state that maintainer review/merge is the only visible
blocker.

Post a release handoff note on the PR before stopping. It must include these
field labels exactly so CI can enforce the final snapshot:
`Head SHA:`, `Mergeable:`, `Merge state:`, `Review decision:`,
`Selected publish tag`, `Publish target:`,
`Validation summary:`, `Unresolved review threads:`, and
`Remaining visible blocker:`. For an independent crate release, spell out the
crate tag (for example `rs-rich-cli-v0.0.6`) and explicitly say not to use the
coordinated `vX.Y.Z` tag unless all selected manifests match it.

The release-readiness policy is centralized in
`.github/release-readiness.json`; keep the workflow, tests, and this document
using those fields rather than adding another release-file, trigger, or
handoff-field list. The policy owns:

- `versionPattern`: text that marks a PR as release-related.
- `releaseFiles` / `releaseFileSuffixes`: changed files that require a release
  handoff even if no version appears in the title, body, or branch.
- `handoffTriggers`: the allowed trigger checks the workflow may combine.
- `handoffCommentWaitSeconds`: the bounded wait that lets CI observe a
  just-posted current-SHA handoff comment before failing as stale.
- `handoffRequiredText`: the exact handoff labels CI requires.

Update `scripts/test_release_readiness.py` with any policy-shape change so the
schema fails locally before the workflow fails remotely.

### Windows validation ordering

Do not run Cargo commands that build the same binary in parallel on Windows when
they share the default `target` directory. Windows can hold `target\debug\*.exe`
open long enough for a concurrent Cargo invocation to fail with
`Access is denied`. For release prep, prefer the serialized wrapper:

```bash
python scripts/validate_release.py --tag release-0.0.20
```

`--tag` is required. The wrapper always ends by running
`scripts/release.py plan <tag>`, so the local validation list proves the selected
release tag as well as build/test health. Run locally, `plan` of a release
tag checks every crate of the release's `RELEASES.toml` (the run checks the
unpublished ones); a crate tag checks that crate. Its Python release checks are listed
explicitly (`test_release.py` and `test_release_readiness.py`) instead of through
a glob, so adding another release test module requires updating the wrapper on
purpose.

For ad-hoc parallel validation, isolate jobs with separate `CARGO_TARGET_DIR`
values:

```powershell
$env:CARGO_TARGET_DIR = "target\check-cli"
cargo test -p rs-rich-cli --test cli
```

Use isolation only for validation scratch builds; release packaging and locked
workspace checks should continue to use the normal workspace target unless there
is a specific reason to do otherwise.

### Publish order

One job publishes the whole selection. It re-checks each selected
crate/version against crates.io (a hit, or any answer but 404, aborts), then
runs `cargo publish -p a -p b … --locked --dry-run` for all of it: Cargo
(1.90 and later) packages every crate and verifies each against the earlier
ones' tarballs before anything uploads. The upload is then `cargo publish -p
… -p … --locked`, with Cargo ordering the crates by dependency and waiting
for each to reach the index before uploading what needs it. Uploads are
serialized across releases and manual dispatches by the `crates-io-release`
concurrency group, which never cancels a run in progress.

One `cargo publish` uses one credential, so the upload is split into runs of
crates that share one (`release.py publish_runs`): existing crates with the
Trusted Publishing exchange, brand-new crates with the environment's token.
In `RELEASES.toml` order, each run takes every crate of its kind whose
selected dependencies are already up or in the same run; a new crate that
needs an existing one goes after it, and an existing crate that needs a new
one goes after that. With no new crate, it is one upload.

The gate requires an annotated tag pointing at the checked-out commit on `main`;
lightweight tags are rejected before CI or publication.

### Release numbers and tagging (`RELEASES.toml`)

`RELEASES.toml` lists every package this repository publishes, its version
and its place in the publication order; each tag is derived from it
(`<crate>-v<version>`, `python-v<version>` for PyPI, and `release-<name>`
for the release). CI runs `scripts/release_cohort.py check`, which fails when
an entry's version differs from its manifest, a publishable crate is missing,
or a crate is listed before a workspace crate it depends on. After a version
bump, `release_cohort.py sync` rewrites the versions from the manifests.

A release is named by the version of the last crate in `RELEASES.toml`,
`rs-rich-cli`: release 0.0.20 is the commit whose `RELEASES.toml` has
rs-rich-cli 0.0.20, and its release tag is `release-0.0.20`. Every tag
`release_cohort.py tag` makes records its release and the commit it was cut
from in its message (`Release: 0.0.20`, `Commit: <sha>`); the release tag's
also lists the crates due when it was pushed.

#### How to release

From an up-to-date `main` (`git switch main && git pull --ff-only`), with
authorization to publish:

```bash
python3 scripts/release_cohort.py tag --dry-run   # the plan: what is tagged, kept, skipped
python3 scripts/release_cohort.py tag             # tag, push, and wait for the runs
```

`tag` does, in order:

1. **Picks the commit.** The newest release that has tags is resumed on the
   commit those tags are on while any of it is still unpublished; with
   nothing in progress, the local checkout (`HEAD`). `--release 0.0.20` (or
   `v0.0.20`) releases that one: from the commit its tags are on, else the
   last commit on `main` (as last fetched) naming it. `--commit <sha>` names
   any commit on `main` outright. `RELEASES.toml` and the manifests come from
   that commit; `main` is never fetched.
2. **Refuses a split release**: a commit that changes the shipped files (by
   each manifest's `include`) of a package tagged under the same version on
   another commit, naming each package, the files and the commit. Release
   from that commit (`--commit <sha>`) or bump the package.
3. **Tags every crate not yet on crates.io** with an annotated record tag
   (skipping published versions, never tagging them), and pushes all the
   records in **one `git push --atomic`**: all or none.
4. **Pushes the release tag** `release-<name>`, and `python-v<version>` when
   PyPI is due, in a second atomic push. They are separate because GitHub
   starts no workflow for a push of more than three tags; the records start
   nothing anyway, and must exist before the release tag's gate looks for
   them.
5. **Waits** for the release run to succeed with every selected crate on
   crates.io, then for the PyPI run, stopping as soon as a run fails (with
   the GitHub CLI, `gh`) or after `--timeout` minutes per run.

`--registry crates.io` tags only the crates and the release tag, leaving
the Python package for later; `--registry pypi` only the Python package.
`status` shows every package's state and the release tag.

The release run (`release.yml`), from the release tag:

1. **Gate, once:** the tag is annotated, on `main` and on the checked-out
   commit (`release.py validate-tag`); `RELEASES.toml` there matches the
   manifests (`release_cohort.py check`); no package of the release is
   tagged on another commit shipping other files, and the tag's recorded
   `Release:`/`Commit:` are its own (`same-commit`); the **selection** is
   every crate of the release not yet on crates.io, in `RELEASES.toml`
   order, refused when empty or when any selected crate's record tag is
   missing or elsewhere (`unreleased`); every selected manifest and internal
   requirement agrees (`release.py plan`).
2. **CI, once:** `ci.yml` on the validated commit.
3. **Publish, once:** preflight, the multi-package dry run, the Trusted
   Publishing exchange (when any selected crate exists), the upload runs,
   then one verification of the whole selection (below).

#### Re-running a release

Re-run `python3 scripts/release_cohort.py tag`: it finds the release, and
the gate of a new run selects only what is still unpublished, so nothing is
uploaded twice (crates.io would refuse it anyway).

- **The run ended before publishing anything** (the gate or CI failed, or
  it was cancelled before the publish job started): merge the fix to `main`,
  pull, and run `tag`. A release held only by such dead tags is not resumed
  where it was: `HEAD` is released, and the records and the release tag are
  deleted on the remote and pushed again there. With no fix needed (a
  flake), `tag` pushes the release tag again on the same commit.
- **The run failed after publishing some crates** (an index timeout, a
  verification flake, an expired token): run `tag` again. The published
  crates hold the release to its commit; `tag` deletes the release tag and
  pushes it again there (its records stay), and the new run publishes the
  rest. A fix to an unpublished crate cannot go on another commit under the
  same version (the records bind it): bump that crate, which gives it a new
  tag, then `tag --commit <the fix>`.
- **A fix is needed in `release.yml` itself:** a tag push runs the workflow
  from the tagged commit, so dispatch the fixed one from `main` instead:
  `gh workflow run release.yml --ref main -f tag=release-0.0.20`. The gate
  still checks out and publishes the tag's commit, with that commit's
  scripts. `tag` does not see a dispatched run (it is on `main`, not the
  tag): watch it with `gh run watch`.

`tag` decides from the release tag's latest run: one still going, or one
that succeeded, is waited on; one that ended without success is pushed
again. The wait ignores the replaced run. A record tag on another commit is
moved to the commit being released only when the run that publishes it
(the release tag's on that commit, or for a crate tag pushed alone before
release tags, its own) ran on that commit, ended without success and never
started its publish job; anything less certain holds the release to that
commit.

#### Publishing one crate by hand

Dispatch `release.yml` with that crate's existing tag:
`gh workflow run release.yml --ref main -f tag=rs-rich-ext-v0.0.18`. The run
publishes that crate alone, refusing it when it is already out, after the
same gate and CI. Create the tag first (annotated, on `main`, with the
release's `Release:`/`Commit:` lines as `tag` writes them) if it does not
exist. Every dependency it names must already be on crates.io.

#### After a release: notes first, then the next versions

The docs site keeps a copy of each release's docs
(`scripts/build_versioned_docs.py`): release X.Y.Z's are built from the last
commit on `main`'s first-parent line before `RELEASES.toml` moves a package
past what X.Y.Z shipped, or from `main` while nothing has moved. So the
order after a release matters:

1. Merge the post-release docs (the release notes, the CHANGELOG's cohort
   heading, any docs fixes for that release) on their own, with no version
   change.
2. Only then open the next release's versions, in a later merge.

Docs fixed for a release after its versions moved on show under **dev**
only.

#### Release 0.0.19

Release 0.0.19 was cut (2fc9772) before release tags: that commit's
`release.yml` does not start on `release-*`, and `tag` refuses to push a
release tag there. Finish it one tag at a time with the scripts of `main`
before this change:

```bash
git worktree add ../release-old 5989aac
python3 ../release-old/scripts/release_cohort.py tag
```

The first release from a commit with this workflow is the first one-run
release.

### Registry authentication (Trusted Publishing)

Since 0.0.10 the workflow publishes existing crates with no long-lived
crates.io secret. After the preflight and dry run pass, `rust-lang/crates-io-auth-action` exchanges the job's
GitHub OIDC token (`id-token: write` on the `publish` job only) for a short-lived
crates.io token, which only the upload step receives. A failed exchange stops
the job before any crate is uploaded; `verify_only` runs never request a token.
The exchanged token is short-lived (about half an hour): a release whose
uploads outlast it fails partway, and is resumed as above.

Each crate needs a Trusted Publishing entry on crates.io
(crate → Settings → Trusted Publishing) with repository
`buchochelliq-labs/rs-rich-cli`, workflow `release.yml` and environment
`crates-io`. A crate set to "trusted publishing only" rejects token uploads
with `403 Forbidden`, which is how the first 0.0.9 core upload failed. Once the
first trusted publish succeeds, delete the old `CARGO_REGISTRY_TOKEN`
repository secret.

**A brand-new crate cannot start with Trusted Publishing.** crates.io only
offers the setting on a crate that already exists, so a new crate's first
version must be uploaded with a maintainer's API token. The workflow does this
itself, in the same run as the rest of the release: its preflight reports
the selected crates that do not exist yet, the upload step receives the
`crates-io` environment's `CARGO_REGISTRY_TOKEN` secret only then, and
`release.py publish` uploads the new crates with it and every existing crate
with the exchanged token, in separate `cargo publish` runs in dependency
order (see Publish order). It fails before uploading anything if a run's
credential is missing. After the first version is up, add the crate's
Trusted Publishing entry. Uploading by hand also works, from the release
commit on `main`, once every dependency it names (optional ones included) is
on crates.io:

```bash
cargo publish -p rs-rich-macros --locked   # with a maintainer's API token
```

Before one-run releases, a new crate went up with its own tag, and a
release could not mix new and existing crates: `rs-rich-macros` 0.0.1 was
published by hand on 2026-09-24, `rs-rich-plugin-api`, `rs-rich-lumis` and
`rs-rich-mermaid` 0.0.1 on 2026-09-26, and each later new crate
(`rs-rich-record`, `rs-rich-interact`, `rs-rich-diagram`, `rs-rich-data`,
`rs-rich-ratatui`, `rs-rich-intuituive`, `rs-rich-embed`, `rs-rich-web`)
with its own `<crate>-v0.0.1` tag, after its dependencies and before its
dependents.

A tag push runs the workflow file from the tagged commit. To publish an existing
tag with a newer workflow on `main`, dispatch the workflow manually from `main`
with that tag; the gate still checks out and publishes the tag's exact commit.

After publishing, verification reuses the protected `crates-io` job's validated
checkout: fresh registry consumers can execute build scripts too. The short-lived
registry token is scoped only to the upload step.
Verification runs once, for the whole selection: it waits for **each
selected version** on crates.io and fails if one does not appear. In fresh
temporary directories outside the checkout, it installs the CLI with an
exact version and `--locked`, or compiles a consumer with an exact registry
dependency for each selected library. A crate not selected is not installed
or verified again.

To retry verification after every selected package has been uploaded,
manually dispatch the release workflow with the same existing tag and
`verify_only: true`. For a release tag it verifies the crates whose record
tags are on the release commit, all of which must be on crates.io. This keeps
the annotated-tag, ancestry and full-CI gates, skips preflight and the
uploads, and runs exact-version verification. Normal tag pushes and manual
dispatches default to publication and still reject any existing version.
Verification-only mode cannot complete a partial upload; re-running the
release does (above).

Publishing is **irreversible**: versions are immutable and can only be
yanked. Planning and documentation updates do not create tags or publish
packages; `release_cohort.py tag` does, from `main`, and the protected
release workflow publishes.

### Python package (PyPI)

The Python bindings (`crates/rich-py`, `rs-rich` on PyPI, `import rs_rich`)
release separately from the crates, from their own `python-vX.Y.Z` tags,
which `release_cohort.py tag` pushes beside the release tag when PyPI is due.
A Python tag never matches `release.yml`'s `release-*` tags, and a release
tag never matches the Python workflow's.

1. Bump `version` in `crates/rich-py/pyproject.toml` and `Cargo.toml`. The
   package has its own SemVer, independent of the crates.
2. Tag the merged commit on `main`: `git tag -a python-v0.0.1 -m "rs-rich
   (PyPI) 0.0.1"`, then push the tag.
3. `.github/workflows/pypi-release.yml` checks that the tag matches the
   version and is on `main`. It builds and smoke-tests the wheels, builds the
   sdist, and publishes from the `pypi` environment with PyPI Trusted
   Publishing (`id-token: write`, no token secret).

PyPI's publisher for `rs-rich` was created as a pending publisher on
2026-09-24. It expects this repository, the workflow file
`pypi-release.yml` and the environment `pypi`, so none of them may be
renamed. The first successful upload creates the project. Create the `pypi`
environment under Settings → Environments before the first tag, and protect it
like `crates-io`: tags only, with a required reviewer if wanted. Uploads are
permanent: a version can be yanked but never replaced.

## What is enforced, and what is merely written down

Documented discipline decays; these are the mechanisms.

| Risk | Mechanism | Prevents or detects |
|---|---|---|
| Pushing to a branch whose PR merged | `delete_branch_on_merge` — the branch ceases to exist | prevents |
| …the same, on a clone that still has the branch | `.githooks/pre-push` — refuses the push | prevents |
| Merging a stacked PR into a dead base | `base branch` CI check | prevents |
| An rc branch drifting behind `main` | `rc not behind main` CI check | detects, before release |
| Anyone creating an `rc/*` branch | `creation` rule in the "rc branches" ruleset — admins only | prevents |
| An rc branch deleted or force-pushed | `deletion` + `non_fast_forward` rules on `rc/*` | prevents |
| An rc branch cut from something other than `main` | `rc branch guard` workflow on branch creation | detects (GitHub cannot prevent it) |
| A required check that can never report | one aggregate `ci-ok` context, not per-matrix-job names | prevents |
| Merging a branch cut from a stale `main` | `strict: true` on required checks | prevents |
| A PR that says nothing useful | `pr body` CI check against the template | prevents |
| Publishing content that isn't on `main` | tags only ever go on `main`; the ancestor assertion above | detects |
| "Is it actually merged?" | `git merge-base --is-ancestor "$(gh pr view N --json mergeCommit -q .mergeCommit.oid)" origin/main` | detects |

That last one matters because the obvious tools lie: after a squash or rebase
merge, `git branch --contains` finds nothing and `git cherry` reports false
negatives. Testing the **merge commit** works regardless, because the merge commit
lives on the base branch and its SHA is never rewritten.
