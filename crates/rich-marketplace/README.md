# rs-rich-marketplace

The client side of the [rs-rich](https://github.com/buchochelliq-labs/rs-rich-cli)
plugin marketplace: it reads the marketplace's signed index and listings,
chooses a version the running host can load, and installs it. The contract
is the [marketplace design note](https://github.com/buchochelliq-labs/rs-rich-cli/blob/main/docs/design/marketplace.md).
This crate is an rs-rich addition, not a port: Python `rich` has no plugins.
There is no terminal code here; `rich plugin` in rs-rich-cli prompts and
prints.

- **Keys.** `KeySet` holds one generation of the main key (it signs
  indexes) and the backup key (it signs only the next generation).
  `KeySet::walk` accepts later generations one at a time: signed by both
  keys, or by the backup key alone when it revokes a lost main key.
- **The index.** `verify_index` checks `index.json.minisig` with minisign's
  hashed `ED` algorithm only (the legacy `Ed` is refused), the key id
  before the signature, the trusted comment's serial, the `schema_version`,
  a rollback against the last serial accepted, and the expiry.
- **Listings.** `VerifiedIndex::read_listing` checks a listing file's
  SHA-256 against the index, then the schema's rules and the ones beyond
  it, the same as `scripts/validate_marketplace.py`.
- **Resolution.** `resolve` picks the newest version that is not yanked and
  that this host loads, with rs-rich-plugin-api's own checks
  (`abi::is_supported`, `is_supported_api_version`).
- **Install.** `Installer` downloads over HTTPS only through a `Transport`
  (`HttpsTransport`, ureq, behind the default `http` feature; or
  `MemoryTransport`), reads at most the declared size, checks the SHA-256,
  checks the artifact by kind, moves it into place atomically and writes
  the lockfile. A host can check a WASM module in its sandbox with its own
  `ArtifactChecker`.
- **Load.** `Lockfile::verify` reads an installed file only after checking
  it against the lockfile: no network, and no index needed.

The official keys are not compiled in yet: until they are,
`KeySet::official` refuses and only explicit keys (in tests) verify
anything.

```rust,no_run
use std::time::SystemTime;
use rich_marketplace::{resolve, verify_index, Host, HttpsTransport, Installer, KeySet, Request};

# fn run(index: &[u8], signature: &str, listing: &[u8]) -> Result<(), Box<dyn std::error::Error>> {
let keys = KeySet::official()?;
let index = verify_index(&keys, index, signature, None, SystemTime::now())?;
let listing = index.read_listing("shout", listing)?;
let version = resolve(&listing, &Host::current(), &Request::Newest)?;
let installer = Installer::new("plugins", HttpsTransport::new());
installer.install(&index, &listing, version, SystemTime::now())?;
# Ok(()) }
```

## License

MIT
