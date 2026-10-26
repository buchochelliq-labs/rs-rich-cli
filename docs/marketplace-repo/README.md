# Files for the rs-rich-marketplace repository

These files belong to
[`buchochelliq-labs/rs-rich-marketplace`](https://github.com/buchochelliq-labs/rs-rich-marketplace),
not to this repository. They are kept here because the session that wrote
them could not push there. Copy them across as they are laid out:

| Here | In rs-rich-marketplace |
|---|---|
| `.github/workflows/sign-index.yml` | `.github/workflows/sign-index.yml` |
| `scripts/build_index.py` | `scripts/build_index.py` |

GitHub runs workflows only from a repository's own top-level
`.github/workflows`, so the copy here never runs.

The format, the keys and the signing rules are in
[the marketplace design note](../design/marketplace.md). In short: when a
listing pull request merges to `main`, the workflow validates every
listing with rs-rich-cli's `scripts/validate_marketplace.py`, rebuilds
`index.json` with the serial one higher and a 30-day expiry, signs it with
the main key, checks the signature against the newest `keys/keys-N.json`,
and commits `index.json` and `index.json.minisig` to `main`. It also runs
every Monday, so the index is re-signed before it expires, and can be run
by hand.

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
   (`KeySet::official`, `OFFICIAL_KEYS` in `crates/rich-marketplace/src/keys.rs`);
   until a release carries it, `rich plugin` needs `--marketplace-keys
   keys-1.json` to trust the marketplace.

3. **Create the protected environment.** In the marketplace repository,
   Settings, Environments, New environment, named `marketplace-signing`:
   - **Required reviewers:** the maintainer. Every signing run waits for
     that approval.
   - **Deployment branches and tags:** selected branches, `main` only.
   - **Environment secrets:**
     - `MARKETPLACE_MINISIGN_KEY`: the whole of `main.key`, as the file
       reads (two lines: the untrusted comment and the encrypted key).
     - `MARKETPLACE_MINISIGN_PASSWORD`: its password.

   Do not add the secrets at repository level: there, any workflow on any
   branch could read them.

4. **Let the workflow push to `main`.** It commits the signed index with
   the workflow's own token (`contents: write` on the signing job only).
   If `main` is protected, allow GitHub Actions to bypass the rule for
   pushes, or the commit step fails. A push with the workflow's token
   starts no other workflow, so the commit does not sign itself again.

5. **Pin the validator.** Set the repository variable `RS_RICH_CLI_REF` to
   the rs-rich-cli tag whose `scripts/validate_marketplace.py` and
   `schemas/marketplace` apply. rs-rich-cli's release tags are
   `release-X.Y.Z` (say `release-0.0.22`); move the variable to the new
   tag with each release. Without it, `main` is used.

6. **Add `scripts/build_index.py`**, then merge a listing (or run the
   workflow by hand) and approve the run. The first index has serial 1.

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
