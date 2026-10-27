Responses the script tests replay instead of calling the network.

- `crates-io-*-versions.json`: crates.io's `GET /api/v1/crates/NAME/versions`,
  recorded on 2026-10-26 and cut to the fields stats.py reads.
- `github-releases.json`: the shape of GitHub's `GET /repos/OWNER/REPO/releases`
  (the marketplace had no releases yet when it was written).
- `github-discussions*.json`: the shape of the GraphQL answer to stats.py's
  `DISCUSSIONS_QUERY`, with Discussions on (bot-opened, someone else's and
  wrong-category discussions) and off.
- `packages/`: listings as `packages/` had them on 2026-10-26, so the tests do not move when it does.
- `site/`: the fixture marketplace `scripts/check_site.mjs` serves to the web
  site: a copy of rs-rich-cli's `docs/tapes/fixtures/marketplace` (the 13
  official listings, `shout` and `solarized` (community) and `status-icons`
  (reviewed), their `index.json` and made-up `stats.json`), without its
  signature and keys, which the page does not read.
