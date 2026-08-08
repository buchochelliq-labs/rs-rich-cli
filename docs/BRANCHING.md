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
| `rc/<X.Y.Z>-rc.<N>` | fresh `origin/main` | `main` via PR, then tag `vX.Y.Z-rc.N` on `main` | minutes |
| `release/<X.Y.Z>` | fresh `origin/main` | `main` via PR, then tag `vX.Y.Z` on `main` | minutes |
| `hotfix/<X.Y.Z>` | the tag `vX.Y.(Z-1)` | nothing — tagged in place, forward-ported to `main` by PR | until forward-ported |

A merged branch is **dead**. It is auto-deleted, and pushing to it is a bug, not
a shortcut. If you need to add to merged work, cut a new branch from `origin/main`.

### Stacked branches

Opening a PR based on another PR's branch is fine. **Merging it while stacked is
not** — that is incident 2 exactly. Retarget before merge:

```bash
gh pr edit <number> --base main
```

CI enforces this: the `base branch` check fails any PR not targeting `main` or
`release/*`.

## Releases

Versions move in **lockstep** — all four crates share one number and one tag.
This is not a stylistic choice. In Cargo, `^0.0.1` is an *exact* requirement, so
`rs-rich-ext` pinned to `rs-rich 0.0.1` can never resolve against `0.0.2`; Cargo
already forces lockstep below `0.1.0`. Independent per-crate versions become
meaningful at `0.1.0` and can be revisited then.

Tags are annotated: `vX.Y.Z` for releases, `vX.Y.Z-rc.N` for candidates.

The full procedure lives in the **`release` skill** (`.claude/skills/release/`).
In outline:

1. `main` is green and `CHANGELOG.md` has content under `## [Unreleased]`.
2. Cut `rc/X.Y.Z-rc.1`, make the single version+changelog commit, PR it, merge.
3. Tag `vX.Y.Z-rc.N` on the merge commit **on `main`**; the release workflow
   publishes it.
4. Soak. Fixes land on `main` as ordinary PRs, then cut another rc.
5. Cut `release/X.Y.Z`, same shape, merge, tag `vX.Y.Z`.

### Publish order

Don't script it. `cargo publish --workspace --locked` derives the topological
order itself and cross-verifies dependents against sibling tarballs. Publishing
is **irreversible** — versions are immutable and can only be yanked.

## What is enforced, and what is merely written down

Documented discipline decays; these are the mechanisms.

| Risk | Mechanism | Prevents or detects |
|---|---|---|
| Pushing to a branch whose PR merged | `delete_branch_on_merge` — the branch ceases to exist | prevents |
| Merging a stacked PR into a dead base | `base branch` CI check | prevents |
| A required check that can never report | one aggregate `ci-ok` context, not per-matrix-job names | prevents |
| Merging a branch cut from a stale `main` | `strict: true` on required checks | prevents |
| A PR that says nothing useful | `pr body` CI check against the template | prevents |
| Publishing content that isn't on `main` | tags only ever go on `main`; the ancestor assertion above | detects |
| "Is it actually merged?" | `git merge-base --is-ancestor "$(gh pr view N --json mergeCommit -q .mergeCommit.oid)" origin/main` | detects |

That last one matters because the obvious tools lie: after a squash or rebase
merge, `git branch --contains` finds nothing and `git cherry` reports false
negatives. Testing the **merge commit** works regardless, because the merge commit
lives on the base branch and its SHA is never rewritten.
