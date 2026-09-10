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
#30 were stranded.

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
2. **The tag explicitly selects what ships.** A `vX.Y.Z` tag retains the
   coordinated workspace meaning: all four manifests and their internal
   requirements must agree at `X.Y.Z`. A `<crate>-vX.Y.Z` tag selects only that
   crate, whose manifest must match the tag. Unselected crates keep their own
   versions and are neither published nor verified as if they had changed.

| Tag | Packages published and verified |
|---|---|
| `v0.0.3` | All four crates at `0.0.3` |
| `rs-rich-v0.0.3` | Only `rs-rich` at `0.0.3` |
| `rs-rich-ext-v0.0.3` | Only `rs-rich-ext` at `0.0.3` |
| `rs-rich-cli-v0.0.3` | Only `rs-rich-cli` at `0.0.3` |
| `rs-rich-art-v0.0.3` | Only `rs-rich-art` at `0.0.3` |

The same forms accept SemVer prereleases, for example
`rs-rich-cli-v0.0.3-rc.1`. Manual dispatch accepts an **existing tag** in one of
these forms, never a branch name. Every job checks out the validated tag's
commit SHA, including the reusable CI gate. Tags still belong on `main` after
merging the release changes; this does not relax the ancestry rule.

For a CLI-only `0.0.3` release with core still at `0.0.2`, use
`rs-rich-cli-v0.0.3`, not `v0.0.3`. For an art-only release, use
`rs-rich-art-v0.0.3` and update the root `rich-art` dependency requirement to
match. All three workspace requirements are checked against their respective
crate versions, not against the selected crate's tag version. Refresh
`Cargo.lock` whenever manifests or dependency requirements change.

Dependencies outside the selected set must already be available on crates.io;
`cargo publish -p <crate> --locked --dry-run` verifies the packaged crate against
those registry dependencies before any upload. If both art and CLI advance and
CLI requires the new art version, publish and verify art first, then CLI.

### What Cargo means by `0.0.x`

The workspace dependency spelling `version = "0.0.2"` is Cargo shorthand for
the caret requirement `^0.0.2`. Cargo permits versions `>=0.0.2,<0.0.3`: for a
`0.0.x` requirement, changing the patch component is incompatible. A path
dependency can use the local package while developing, but its version must
still satisfy that requirement, and the requirement is what consumers see in
the published package. Thus bumping `rs-rich` from `0.0.2` to `0.0.3` requires
updating the requirements used by its direct dependents (`rs-rich-ext`,
`rs-rich-art`, and `rs-rich-cli`); bumping `rs-rich-ext` or `rs-rich-art`
requires updating `rs-rich-cli`. Cargo does **not** force unrelated crates to
share a version. Our coordinated-tag policy does.

### Prepared 0.0.3 snapshot

This preparation includes the Markdown fix from `main`, requiring core 0.0.3.
The ext package also changes its core dependency and must publish a new version.
Together with the already-prepared CLI and art versions, all four manifests now
say 0.0.3. This is the dependency closure for this release, not a lockstep policy.
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

### Publish order

`cargo publish --workspace --locked` retains the coordinated path's topological
order and sibling-tarball verification. Independent tags use
`cargo publish -p <crate> --locked`. Both paths run a dry run with the identical
selection first. The workflow resolves that selection once, checks the
manifests and internal requirements, and queries crates.io for each selected
package/version. A hit or an unexpected registry response aborts the job;
unchanged, unselected versions are not queried. Uploads are serialized across
all release tags and manual dispatches.

After publishing, verification waits for **each selected version** on crates.io
and fails if it does not appear. In fresh temporary directories outside the
checkout, it installs the CLI with an exact version and `--locked`, or compiles
a consumer with an exact registry dependency for each selected library. An
art-only release never installs the CLI or waits for a new core version.

Publishing is **irreversible** — versions are immutable and can only be yanked.
A partial upload still requires manual recovery, not a blind rerun. Planning and documentation updates
do not create tags or publish packages. Create the annotated tag on `main`;
the protected release workflow publishes the selected packages.

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
