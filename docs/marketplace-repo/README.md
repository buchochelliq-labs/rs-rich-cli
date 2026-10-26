# Files for the rs-rich-marketplace repository

These files belong to
[`buchochelliq-labs/rs-rich-marketplace`](https://github.com/buchochelliq-labs/rs-rich-marketplace),
not to this repository. They are kept here because the session that wrote
them could not push there. Copy them across as they are laid out:

| Here | In rs-rich-marketplace |
|---|---|
| `.github/workflows/sign-index.yml` | `.github/workflows/sign-index.yml` |
| `.github/workflows/check-listings.yml` | `.github/workflows/check-listings.yml` |
| `scripts/build_index.py` | `scripts/build_index.py` |
| `scripts/check_artifacts.py`, `scripts/marketplace_common.py` | the same |
| `scripts/add_crate_version.py`, `scripts/test_scripts.py` | the same |
| `.gitattributes` | `.gitattributes` |

The marketplace's `live-0.0.23` branch holds these files as they are here,
with its listings, its artifacts and a README on submitting a listing.
Change both copies together.

GitHub runs workflows only from a repository's own top-level
`.github/workflows`, so the copy here never runs.

The format, the keys and the signing rules are in
[the marketplace design note](../design/marketplace.md). In short: when a
listing pull request merges to `main`, the workflow validates every
listing with rs-rich-cli's `scripts/validate_marketplace.py`, rebuilds
`index.json` with the serial one higher and a 30-day expiry, signs it with
the main key, checks the signature against the newest `keys/keys-N.json`,
and commits `index.json` and `index.json.minisig` to `main`, pushing with
a deploy key. It also runs every Monday, so the index is re-signed before
it expires, and can be run by hand.

`check-listings.yml` checks every pull request, with no secrets and a
read-only token (`pull_request`, so a fork's pull request is safe): every
listing validates; each new or changed version's artifact is downloaded
and its size and SHA-256 checked (a crate's against crates.io's index
checksum; this repository's own `artifacts/` read from the pull request);
a listed version never changes; setting or raising a tier needs a
maintainer's `tier-approved` label; and each new WASM plugin, micro pack and
theme is installed with `rich`, built from rs-rich-cli at
`RS_RICH_CLI_REF`, from a copy of the marketplace signed with a throwaway
key made in the run. It never runs on a push, so the signer's commit does
not start it.

## Installing it

1. **Make the two key pairs** on a machine you trust, offline if you can:

   ```bash
   minisign -G -p main.pub -s main.key       # asks for a password
   minisign -G -p backup.pub -s backup.key   # another password
   ```

   Keep `backup.key` offline and apart from `main.key`. It signs nothing
   but key rotations and never enters GitHub.

2. **Record the public halves.** Write `keys/keys-1.json` in the
   marketplace repository (the ids are the ones `minisign` prints, the
   keys the second line of each `.pub` file):

   ```json
   {
     "schema_version": 1,
     "generation": 1,
     "main": {"id": "…", "public_key": "RW…"},
     "backup": {"id": "…", "public_key": "RW…"}
   }
   ```

   The same file is compiled into rs-rich-marketplace
   (`KeySet::official`, from `crates/rich-marketplace/keys/keys-1.json`)
   since 0.0.23, so `rich plugin` trusts the official marketplace with no
   setting. Generation 1 is the maintainer's pair of 2026-10-26.

3. **Create the protected environment.** In the marketplace repository,
   Settings, Environments, New environment, named `marketplace-signing`:
   - **Required reviewers:** the maintainer. Every signing run waits for
     that approval.
   - **Deployment branches and tags:** selected branches, `main` only.
   - **Environment secrets:**
     - `MARKETPLACE_MINISIGN_KEY`: the whole of `main.key`, as the file
       reads (two lines: the untrusted comment and the encrypted key).
     - `MARKETPLACE_MINISIGN_PASSWORD`: its password.
     - `MARKETPLACE_DEPLOY_KEY`: the deploy key's private half (step 4).

   Do not add the secrets at repository level: there, any workflow on any
   branch could read them.

4. **Give the workflow a deploy key to push to `main`.** The signing job
   reads the repository with the workflow's token (`contents: read`) and
   pushes the signed index over SSH with a deploy key, which main's
   ruleset lets bypass it:

   ```bash
   ssh-keygen -t ed25519 -N "" -C rs-rich-marketplace-signer -f marketplace-deploy
   ```

   - Settings, Deploy keys, Add deploy key: paste `marketplace-deploy.pub`
     and tick **Allow write access**.
   - Settings, Rules, main's ruleset: add that deploy key to the bypass
     list.
   - In the `marketplace-signing` environment, add the whole private file
     `marketplace-deploy` as the secret `MARKETPLACE_DEPLOY_KEY`.
   - Keep a copy of the private file in the password manager, then delete
     both local files.

   A push by a deploy key starts workflows (one by the workflow's own
   token would not), but the signer's commit changes only `index.json`
   and `index.json.minisig`: the signing workflow's push trigger is
   limited to `packages/**` and `keys/**`, and `check-listings.yml` runs
   on pull requests only, so the commit starts neither.

5. **Pin the validator.** Set the repository variable `RS_RICH_CLI_REF` to
   the rs-rich-cli tag whose `scripts/validate_marketplace.py` and
   `schemas/marketplace` apply. rs-rich-cli's release tags are
   `release-X.Y.Z` (say `release-0.0.22`); move the variable to the new
   tag with each release. Without it, `main` is used. The pull request
   check builds `rich` at the same ref.

6. **Create the `tier-approved` label** (Issues, Labels). The pull request
   check refuses a listing whose tier is set or raised above `community`
   until a maintainer adds it.

7. **Merge the first listings**, the `live-0.0.23` pull request (official
   listings, artifacts, scripts and workflows), once steps 1 to 6 are
   done: the merge touches `packages/`, so it starts the signing workflow.
   Approve the run. The first index has serial 1; `rich plugin search`
   then works with no setting.

## Rotating a key

The workflow only signs indexes. A rotation is signed by hand, as the
design note says: write `keys/keys-N+1.json`, sign it with both keys of
generation N (or, for a lost main key, the backup key alone, naming the old
main key in `revoked`), commit the file and its `.main.minisig` and
`.backup.minisig`, then replace `MARKETPLACE_MINISIGN_KEY` and
`MARKETPLACE_MINISIGN_PASSWORD` with the new main key's before the next
run:

```bash
minisign -S -s main.key -m keys-2.json -x keys-2.json.main.minisig \
  -t "rs-rich-marketplace keys generation=2"
minisign -S -s backup.key -m keys-2.json -x keys-2.json.backup.minisig \
  -t "rs-rich-marketplace keys generation=2"
```

The workflow checks each new signature against the newest `keys-N.json`'s
main key, so a run between the commit and the secret change fails rather
than signing with the old key.
