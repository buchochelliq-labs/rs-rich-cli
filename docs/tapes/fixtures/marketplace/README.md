# A fixture marketplace

A copy of the plugin marketplace for the `plugin-search` tape and for
checking the [Marketplace page](../../../marketplace.md) offline
(`scripts/check_marketplace_page.mjs`). It is not the real marketplace and
nothing installs from it: it has listings and no artifacts.

- `packages/`: the 13 official listings of rs-rich-marketplace at its
  first release, and three examples from `schemas/marketplace/examples/valid`
  (`shout` and `solarized`, community; `status-icons`, reviewed) so that
  every tier and a yanked version show.
- `index.json`: every listing's SHA-256, serial 1, expiring in 2099 so the
  tape does not go stale. A real index expires within 90 days.
- `index.json.minisig` and `keys/keys-1.json`: signed with a throwaway
  minisign key made for this fixture, whose secret half was deleted. No
  `rich` trusts it unless given `--marketplace-keys keys/keys-1.json`.
- `stats.json`: made-up download and review counts in the shape of the
  marketplace's nightly stats, for the page.

To change a listing, make a new throwaway key pair, then rebuild and
re-sign:

```bash
cd docs/tapes/fixtures/marketplace
minisign -G -W -p main.pub -s main.key       # and a backup pair; write keys/keys-1.json from both
python3 ../../../../../rs-rich-marketplace/scripts/build_index.py --root .   # serial N
# set "expires" in index.json to "2099-12-31T00:00:00Z", then sign:
minisign -S -s main.key -m index.json -t "rs-rich-marketplace index serial=N"
rm main.key main.pub backup.key backup.pub
```

`python3 scripts/validate_marketplace.py --index docs/tapes/fixtures/marketplace/index.json`
checks it.
