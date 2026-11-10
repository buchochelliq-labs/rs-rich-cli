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
2. **One release, one run.** A release is every package of `rab.yml`
   on one commit of `main`, named by `rs-rich-cli`'s version. Its release
   tag, `release-X.Y.Z`, starts one `release.yml` run that checks it once,
   runs CI once and publishes **every package of it not yet on its registry**,
   crates.io and PyPI alike. Each package also has its own annotated record
   tag on the same commit, which starts nothing. Unchanged packages keep their
   versions, are already published, and are neither published nor verified
   again.

Releases run on [rab, the Release Asset Bundle](https://github.com/buchochelliq-labs/release-asset-bundle)
v2.1.1 (formerly release-kit, renamed with 2.0.0), the organisation's release
standard, generalised from this repository's own scripts: its rules are the
ones below (a published version is permanent; releases come from `main`; tags
are records; a release is one commit; re-run, don't repair; least privilege).
Its documentation is at
<https://buchochelliq-labs.github.io/release-asset-bundle/>.

| Tag | What it does |
|---|---|
| `release-0.0.25` | Pushed by `release.py tag`: publishes every package of release 0.0.25 not yet on its registry |
| `rs-rich-ext-v0.0.22` | A record of that crate in its release (`rs-rich-ext 0.0.22 (crates.io)`); starts nothing |
| `python-v0.0.13` | A record of the PyPI package `rs-rich` in its release (`rs-rich 0.0.13 (pypi)`); starts nothing |
| `vX.Y.Z` | Historical (every crate at one version, until 0.0.3) |

Versions accept SemVer prereleases (PEP 440 ones for PyPI), for example
`release-0.0.25-rc.1`. A manual dispatch of `release.yml` takes an **existing
release tag**, never a branch name. Every job checks out the gated commit,
including the reusable CI gate. Tags belong on `main` after merging the
release changes; this does not relax the ancestry rule.

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

`cargo-semver-checks` follows the same rule: a move from `0.0.x` to anything
else is a major one, so it accepts any change. `release.py plan
--semver-checks` therefore refuses nothing for a 0.0.x crate whose version
moved; CI's `public API and SemVer` job checks 0.0.x crates as if their bump
were a minor one, and fails on a breaking change that `api/semver-breaks.toml`
does not list (`scripts/semver_check.py`).

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
`scripts/release.py plan <tag> --semver-checks`, so the local validation list proves the selected
release tag as well as build/test health. Run locally, `plan` of a release
tag checks every crate of the release's `RELEASES.toml` (the run checks the
unpublished ones); a crate tag checks that crate. Its Python release checks are listed
explicitly (`test_release.py` and `test_release_readiness.py`) instead of through
a glob, so adding another release test module requires updating the wrapper on
purpose. Then rehearse the release with rab:
`python3 scripts/release_rehearsal.py` (see
[Release numbers and tagging](#release-numbers-and-tagging-rabyml)).

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

`publish-crates` publishes the crates of the selection, in `rab.yml`
order, with the kit's `release.py publish-crates`. It re-checks each selected
crate/version against crates.io (`preflight`: a hit aborts), runs one `cargo
publish --dry-run --locked -p a -p b …` for all of them (Cargo 1.90 and later
packages every crate and verifies each against the earlier ones' tarballs
before anything uploads), then one `cargo publish --locked -p … -p …` per run
of crates that share a credential: existing crates with the Trusted Publishing
exchange, a brand-new crate's first version with the environment's token, one
new crate at a time (crates.io's new-crate rate limit is waited out). Order is
kept, so each crate goes up after the crates it depends on. Releases are
serialized by the `release` concurrency group, which never cancels a run in
progress.

`publish-pypi` uploads the wheels and the sdist that the build jobs made and
tested, in the same run. A release whose wheels fail to build or fail their
test, or whose crates fail the SemVer check, publishes nothing anywhere.

### Which path is current

**rab (formerly release-kit) is the release path, from 0.0.25 on:**
`rab.yml`, the kit installed under `.github/rab/`, and `release.yml` on the
kit. Until rab has published its first release, the old path's pieces stay
beside it, and retire together once it has:

| Old piece | Now |
|---|---|
| `RELEASES.toml`, `scripts/release_cohort.py check`/`sync` | kept in step with `rab.yml` (CI checks both; `scripts/build_versioned_docs.py` still reads `RELEASES.toml`). Bump the manifests, then run both `sync`s |
| `release_cohort.py tag`/`status` | **not used**: `.github/rab/scripts/release.py tag`/`status` |
| `pypi-release.yml` | no longer starts on `python-v…` tags; a manual fallback (`gh workflow run pypi-release.yml -f tag=python-vX.Y.Z`) that can upload only while PyPI still trusts it |
| `scripts/release.py` | `plan --semver-checks` runs in `release.yml`'s `semver` job; its `preflight`, `publish` and `verify` are the kit's now |

### Release numbers and tagging (`rab.yml`)

`rab.yml` lists every package this repository publishes (every crate,
then the PyPI `rs-rich`), its manifest, its version and its place in the
publication order: each after the workspace crates it depends on. A release is
named by `rs-rich-cli`'s version (`release: name_from: rs-rich-cli`), and its
tag is `release-<name>`. Each package's record tag is `<name>-v<version>`,
except the PyPI package's, `python-v<version>`: the crate `rs-rich` already
owns `rs-rich-v…`. Each package's `paths` is what it ships (a crate's
`include`, plus `Cargo.lock` for the three binaries; for the PyPI package, its
directory, the workspace manifest and the crates it builds from): the files
the kit's split check compares. The PyPI package's `artifacts` is one maturin
wheel per target (pypi-release.yml's five, on the same runners) and an sdist;
each wheel is installed and imported (`crates/rich-py/tests/smoke.py`) where
its runner can run it, and the linux x86_64 one also runs the compatibility
suite against rich 15.0.0 and the stub check.

Every command is the installed kit's script,
`python3 .github/rab/scripts/release.py <command>` (Python 3.11+,
PyYAML, git; `tag` also uses the GitHub CLI):

| Command | What it does |
|---|---|
| `check` | `rab.yml` matches every manifest (versions, names, tag formats, order, artifacts). CI runs it |
| `vendored` | the installed copy is unmodified (it changes only by installing a newer kit). CI runs it |
| `sync` | after a version bump, rewrites the versions from the manifests |
| `status` | each package: its record tag, where it is, and `published` or `due` |
| `plan --selection JSON` | the build jobs a run would start |
| `tag [--dry-run]` | tags the release and waits for its run (below) |
| `gate TAG [--verify-only]`, `preflight`, `publish-crates`, `verify`, `consume` | what the release run runs; by hand only to see what a run would do |

`python3 scripts/release_rehearsal.py` runs `check`, `vendored`, `plan` (every
package, as if all were due) and `tag --dry-run` against a throwaway remote
that holds `HEAD` as `main` and `origin`'s tags, so nothing is pushed. CI runs
it in `release-tests`. While a release is tagged and not yet finished, its
`tag --dry-run` fails on any later commit that changes a package of it (a
split release): finish the release first.

To install a newer kit: check out its tag in a clone of
<https://github.com/buchochelliq-labs/release-asset-bundle>, run its
`scripts/release.py install` from this repository's root, review the diff
(it rewrites `.github/rab/` and `.github/workflows/rab-build.yml` and leaves
`release.yml` alone), compare `release.yml` with the kit's
`templates/release.yml`, and open a pull request. rab 2.0.0's install moved
this repository from release-kit 1.7.0's names (`release-kit.yml`,
`.github/release-kit/`, `release-kit-build.yml`); the `record` job came from
its template by hand.

#### Tags from before release-kit

Every tag before release 0.0.25 predates the kit: `v0.0.1` to `v0.0.4`, every
`<crate>-v…` and `python-v…` tag up to the 0.0.24 cohort's, and `release-0.0.20`
to `release-0.0.24`, written by `scripts/release_cohort.py` and the scripts
before it. Their messages start `<name> <version>`, not the kit's
`<name> <version> (<registry>)`, and their commits have neither `rab.yml`
nor release-kit's `release-kit.yml`.
The kit needs no option for them. It checks a tag's ownership only for a
package's *current* version, and only while that version is unpublished; on
2026-11-04 every one of the 200 old tags named a version already on its
registry (all annotated), so the kit never selects, moves or resumes them,
and `tag --dry-run` against the real tags passes. A release tagged with
`release_cohort.py tag` would not be resumed by the kit (its commit has
neither `rab.yml` nor `release-kit.yml`): finish such a release with the old
scripts, or bump the packages still due.

#### How to release

From an up-to-date `main` (`git switch main && git pull --ff-only`), with
authorization to publish, and once someone has walked Noctiluca's
[real-site checklist](guide/noctiluca/status.md#checking-on-real-sites)
(a news site, a video watch page, a live stream with chat, a docs site and
a forum) and put what they saw in the release notes:

```bash
python3 .github/rab/scripts/release.py status
python3 .github/rab/scripts/release.py tag --dry-run   # what is tagged, kept, skipped, moved
python3 .github/rab/scripts/release.py tag             # tag, push, and wait for the run
```

`tag`:

1. **Picks the commit:** `--commit <sha>`; else a release already tagged and
   not finished (`--release X.Y.Z`, or the newest one); else, with
   `--release`, the last commit on `main` naming it; else `HEAD`. The commit
   must be on `origin/main` as last fetched.
2. **Refuses** a package tag on the remote that is not that package's record,
   a `rab.yml` that fails `check` at that commit, and a split release:
   a commit that changes the `paths` of a package tagged under the same
   version on another commit.
3. **Tags every package not yet published** with an annotated record tag
   (`skip` for what is published, `keep` for a tag already there, `move` for
   one on a commit whose run provably published nothing), and pushes the
   records in **one atomic push**.
4. **Pushes the release tag** on its own (GitHub starts no workflow for a
   push of more than three tags).
5. **Waits** (up to `--timeout`, three hours by default) until the run has
   succeeded and every package is on its registry, stopping when the run
   fails.

The release run (`release.yml`), from the release tag:

1. **`gate`** (the kit's): the tag is annotated, records its release and
   commit, and is that commit's release tag; `rab.yml` there passes
   `check`; the commit is on `main`; no unpublished package's tag belongs to
   something else; the release is not split; the **selection** is every
   package not yet published, each with its record tag on the commit.
2. **`ci`:** `ci.yml` on the gated commit (`checkout_ref`).
3. **`build`** (the kit's `rab-build.yml`): the wheels and the sdist,
   each tested as above. **`semver`** (this repository's): every selected
   manifest and internal requirement agrees, and each crate's version bump is
   at least as big as its change by Cargo's rules (`scripts/release.py plan
   --semver-checks`, with `cargo-semver-checks` against the crate's last
   release; a 0.0.x bump fits any change).
4. **`publish-crates`** and **`publish-pypi`**, after all of the above:
   preflight, upload, then `verify` (each version served) and `consume` (each
   package used as a stranger would: `cargo install` for the three binaries,
   a consumer crate for each library, `pip install` in a new virtualenv).
5. **`publish-playground`**, after the crates are up: the docs playground's
   module, attached to the GitHub release `release-X.Y.Z`. It cannot fail the
   run.
6. **`record`** (the kit's), whatever happened before it: once the release
   tag is checked to still name the gated commit, it uploads the run's
   [release record](https://buchochelliq-labs.github.io/release-asset-bundle/release-record/)
   as the `release-record` artifact (each job's result, and each package's
   outcome, registry URL and built files' SHA-256). It needs every job above,
   publishes nothing and needs no write permission
   (`gh run download <run> -n release-record` reads it).

Only jobs named `publish…` upload, and every job that uploads is named so:
`tag` reads a run in which such a job started as one that may have published,
and then never moves its tags.

#### Re-running a release

Re-run, don't repair: fix the cause, then run `release.py tag` again (`tag
--dry-run` first shows what it will do). Never delete or move tags by hand,
and never use GitHub's "Re-run jobs" on a publish job.

- **The run ended before publishing anything** (the gate, CI, the build or
  the SemVer check failed): merge the fix to `main`, pull, and run `tag`. The
  run is provably dead, so `tag` moves the record tags and the release tag to
  `HEAD`, and the new run starts there.
- **The run failed after publishing some packages** (an index timeout, a
  missing secret, an expired token): fix the cause outside the tagged commit,
  then run `tag`. The release stays on its commit; `tag` skips what is
  published and pushes the release tag again, and the new run publishes the
  rest. A fix that must change files a due package ships cannot go on
  another commit under the same version: bump those packages.
- **PyPI stopped partway:** the version counts as published once any file is
  up, so a re-run uploads no more of it. Upload the missing wheels from the
  run's `release--rs-rich--…` artifacts by hand (`twine upload`).
- **Everything is up, but `verify` or `consume` failed:** start a verify-only
  run, `gh workflow run release.yml -f tag=release-X.Y.Z -f verify_only=true`
  (the gate's `--verify-only`): it uploads nothing and checks every package
  again.
- **A fix is needed in `release.yml` itself:** a tag push runs the workflow
  from the tagged commit, so dispatch the fixed one from `main`:
  `gh workflow run release.yml --ref main -f tag=release-X.Y.Z`. The gate
  still checks out and publishes the tag's commit. `tag` does not see a
  dispatched run on `main`: watch it with `gh run watch`.

The kit's [recovery guide](https://buchochelliq-labs.github.io/release-asset-bundle/recovery/)
has the details and the refusals that need a decision.

Publishing one crate by hand, by dispatching its record tag, is gone with the
old workflow: a release publishes whatever of it is due.

#### After a release: notes first, then the next versions

The docs site keeps a copy of each release's docs
(`scripts/build_versioned_docs.py`). Release X.Y.Z's are built from the commit
`docs-versions.toml` names for it; without a line there, from `main` while
`RELEASES.toml` still lists what X.Y.Z shipped, and from the last commit on
`main`'s first-parent line before that changes. So, after a release:

1. Commit the post-release docs (the release notes, the CHANGELOG's cohort
   heading, docs fixes for that release) with no version change.
2. In a later commit (the same pull request is fine), open the next
   release's versions (the manifests, then `release.py sync` and
   `release_cohort.py sync`), and add X.Y.Z's line to `docs-versions.toml`
   naming the last commit of step 1.

Docs fixed for X.Y.Z later show under **dev**; to show them under X.Y.Z too,
move its line to a newer commit that still has X.Y.Z's versions.

Each version builds with the tool its own tree pins: Zensical when its
`requirements-docs.txt` names it (0.0.24 on), else MkDocs and Material, which
built 0.0.23 and older (Zensical does not build those trees unchanged; their
raw HTML's URLs were written for MkDocs). The docs job installs both from
`requirements-docs.txt`.

#### Release 0.0.19

Release 0.0.19 was cut (2fc9772) before release tags: that commit's
`release.yml` does not start on `release-*`. It was finished one tag at a
time with the scripts of `main` before one-run releases (`git worktree add
../release-old 5989aac`, then that tree's `scripts/release_cohort.py tag`).
The first one-run release was 0.0.20; the first on rab (formerly release-kit) is 0.0.25.

### Registry authentication (Trusted Publishing)

The release publishes existing crates and the PyPI package with no
long-lived secret. Only `publish-crates` and `publish-pypi` request
`id-token: write`, each in its own protected environment (`crates-io`,
`pypi`); every checkout uses `persist-credentials: false`, and every
third-party action is pinned to a commit.

**crates.io.** After the preflight, `rust-lang/crates-io-auth-action`
exchanges the job's GitHub OIDC token for a short-lived crates.io token, only
when an existing crate is selected; the kit's `publish-crates` receives it.
`verify_only` runs never request one. The token is short-lived (about half
an hour): a release whose uploads outlast it fails partway, and is resumed as
above. Each crate needs a Trusted Publishing entry on crates.io
(crate → Settings → Trusted Publishing) with repository
`buchochelliq-labs/rs-rich-cli`, workflow `release.yml` and environment
`crates-io`. A crate set to "trusted publishing only" rejects token uploads
with `403 Forbidden`, which is how the first 0.0.9 core upload failed.

**A brand-new crate cannot start with Trusted Publishing.** crates.io only
offers the setting on a crate that already exists, so a new crate's first
version is uploaded with the `crates-io` environment's `CARGO_REGISTRY_TOKEN`
secret (a token with `publish-new`, and `publish-update` if it is shared). The
preflight reports which selected crates are new, the secret reaches the kit's
action only then, and `publish-crates` refuses before uploading anything if a
needed token is missing. After the first version is up, add the crate's
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

**PyPI.** `publish-pypi` uploads with `pypa/gh-action-pypi-publish`, which
exchanges the job's OIDC token: PyPI's trusted publisher for `rs-rich` must
name repository `buchochelliq-labs/rs-rich-cli`, workflow **`release.yml`** and
environment `pypi`. Until 0.0.24 it named `pypi-release.yml`; PyPI accepts more
than one trusted publisher per project, so the `release.yml` one is added
beside it, and the old one is removed when `pypi-release.yml` retires.

Publishing is **irreversible**: versions are immutable and can only be
yanked. Planning and documentation updates do not create tags or publish
packages; `release.py tag` does, from `main`, and the protected release
workflow publishes.

### Python package (PyPI)

The Python bindings (`crates/rich-py`, `rs-rich` on PyPI, `import rs_rich`)
are a package of the release like the crates: `rab.yml` lists them
last, with their own SemVer, independent of the crates, and the release run
builds, tests and publishes them when their version is not yet on PyPI. Their
record tag is `python-vX.Y.Z`; it starts nothing.

1. Bump `version` in `crates/rich-py/pyproject.toml` and `Cargo.toml` (CI's
   `check` fails if they differ), then `release.py sync`.
2. Release as above.

The `pypi` environment (Settings → Environments) is protected like
`crates-io`: release tags only, with a required reviewer if wanted. Uploads
are permanent: a version can be yanked but never replaced.

## Edition and minimum Rust

Every crate, in the workspace and outside it (the Python bindings, the
fuzz targets, the playground, the examples, the fixtures and the app
template), is on **edition 2024**, with the workspace's resolver 3, which
picks dependency versions that build on each crate's `rust-version`.

The minimum supported Rust version (MSRV) is `rust-version` in the root
`Cargo.toml`, and a crate that needs more says so in its own manifest
(rs-rich-lumis, for lumis). The policy, decided for 1.0:

- **The MSRV trails the latest stable by at most six releases.** It moves
  only when a change needs it (a dependency, a language feature), never on
  a schedule, and never past "latest stable minus six".
- **Raising it is a minor release** after 1.0, and is listed in the
  changelog and the release notes; before 1.0 it is listed the same way.
- **CI checks it:** the `minimum supported Rust version` job builds on
  exactly the declared versions.

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
| Publishing content that isn't on `main` | tags only ever go on `main`; rab's gate refuses a tag whose commit is not on it | prevents |
| An edited copy of rab | `release.py vendored` in CI's `release-tests` | detects, in the PR |
| An API change nobody reviewed | `api/<crate>.txt` snapshots, diffed by CI's `public API and SemVer` job | detects, in the PR |
| A version bump smaller than the change | `cargo-semver-checks` in `release.py plan --semver-checks` (`release.yml`'s `semver` job); for 0.0.x, every break listed in `api/semver-breaks.toml` (CI) | prevents |
| "Is it actually merged?" | `git merge-base --is-ancestor "$(gh pr view N --json mergeCommit -q .mergeCommit.oid)" origin/main` | detects |

That last one matters because the obvious tools lie: after a squash or rebase
merge, `git branch --contains` finds nothing and `git cherry` reports false
negatives. Testing the **merge commit** works regardless, because the merge commit
lives on the base branch and its SHA is never rewritten.
