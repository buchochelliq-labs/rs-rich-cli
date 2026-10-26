# The marketplace listing format (#232, #582)

**Status:** the format, its schemas and its validator are in 0.0.21 (plugin
API 0.0.5). The `rs-rich-marketplace` crate (0.0.1) that verifies, resolves
and installs from them is in 0.0.22 ([Extending](../PLUGINS.md#the-marketplace-client-0022));
its official keys (generation 1) are compiled in since 0.0.23. The
`rich plugin` commands that use it are in 0.0.22 too
([Extending](../PLUGINS.md#rich-plugin-installing-from-the-marketplace-0022)),
with the marketplace repository's signing workflow kept to hand over in
[`docs/marketplace-repo/`](https://github.com/buchochelliq-labs/rs-rich-cli/tree/main/docs/marketplace-repo).
**The marketplace goes live with 0.0.23** (the maintainer's call on
2026-10-26): the marketplace repository lists every official plugin
(Mermaid, DOT and lumis as crates, the `textcase` and `wordplay` WASM
plugins), the built-in micro packs (`status`, `dev`, `fun`) and the
official themes (`dusk`, `paper`, `ocean`, `high-contrast`, `mono`), and
checks every listing
pull request; it is live once the maintainer has set up the signing
environment and signed the first index
([0.0.23 plan, item 17](../plans/0.0.23.md)). This note is the contract
between the marketplace repository, the crate and the CLI. It is part of the
1.0 promise ([the plan to 1.0.0](../plans/1.0.0.md#track-b-the-plugin-marketplace-232-463)).
The optional `recommended` key is new in 0.0.23 ([Recommended](#recommended)),
and the docs' [Marketplace page](../marketplace.md) lists the marketplace
in a browser.

Plugins already load three ways ([Extending](../PLUGINS.md),
[Plugin loading](plugin-loading.md)): compiled in, as a native library from a
path, or as a WASM module from a path. The marketplace adds a way to *find,
trust, install and update* a package. It stores where each artifact is and
what it hashes to, never the artifact: crates stay on crates.io, and WASM
modules and asset packs on their authors' release pages. The one exception
is the official WASM plugins, micro packs and themes, which the marketplace
repository hosts itself, pinned by hash like any other: since the themes,
each version as a release asset,
`https://github.com/buchochelliq-labs/rs-rich-marketplace/releases/download/ID-vVERSION/ID-VERSION.EXT`
(one release per version, made after the merge by its
`publish-artifacts.yml`); the first ones keep their
`raw.githubusercontent.com` URLs on `main`, since a listed version never
changes. Either way the listing's pull request carries the file in
`artifacts/ID/`, where its check reads it. See [Stats](#stats) for what
the release counts are used for.

## Summary

- **One JSON file per package**, `packages/<id>.json`, in
  [`buchochelliq-labs/rs-rich-marketplace`](https://github.com/buchochelliq-labs/rs-rich-marketplace),
  validated against
  [`listing.schema.json`](https://github.com/buchochelliq-labs/rs-rich-cli/blob/main/schemas/marketplace/listing.schema.json).
- **Four kinds:** `crate`, `wasm-plugin`, `micro-pack` and `theme`. Only a
  WASM plugin and the two asset packs (micro packs and themes) install from
  the marketplace. A crate is listed, never installed.
- **Every version** names its artifact's URL, SHA-256 and size, what it
  needs from the host (`requires`), its capabilities and its permissions,
  and whether it is yanked and why.
- **One signed index,** `index.json`, records every listing file's SHA-256,
  a serial that only grows and an expiry; `index.json.minisig` is its
  minisign signature by the main key. A backup key exists only to rotate
  keys.
- **The host's versions are constants** in `rs-rich-plugin-api`
  (`PLUGIN_API_VERSION`, `abi::ABI_MAJOR`, `abi::ABI_MINOR`), with the
  checks the loaders make (`is_supported_api_version`, `abi::is_supported`),
  so the marketplace resolves exactly as the host loads.

## Why JSON

The listing is JSON, not TOML:

- **The schema applies directly.** A listing is checked against a JSON
  Schema in the marketplace's CI and in this repository; a TOML file would
  first be converted, and TOML's dates and times have no JSON Schema type.
- **It matches what the host already reads.** A micro package's
  `manifest.json` and a pack's `pack.json` are JSON with a
  `schema_version`, and `serde_json` is already a dependency of every crate
  that will read listings.
- **Every reader parses it the same way.** The bytes are signed and hashed,
  so what matters is that the CI (Python, `json`), the crate (`serde_json`)
  and a reviewer's tools agree on what they mean. Duplicate keys and
  non-finite numbers are refused by the validator and must be by the crate.

The cost is that JSON has no comments; `description` and `yank_reason` carry
what a comment would, and a listing is written by `rich plugin publish`
(0.0.22) rather than by hand.

## The listing

One file per package, `packages/<id>.json`. The keys, all required unless
marked optional:

| Key | Type | Meaning |
|---|---|---|
| `schema_version` | integer | `1` for this format. A client refuses a version it does not know. |
| `id` | string | Unique across every kind. The plugin name rule: lowercase letters, digits, `-`, `_` and `.`, starting with a letter or digit, at most 64 bytes (`rich_plugin_api::is_valid_name`). A micro pack's id is also a micro name segment (no `.`). |
| `kind` | string | `crate`, `wasm-plugin`, `micro-pack` or `theme`. |
| `name` | string | A human-readable name. |
| `description` | string | One line on what it adds. |
| `tier` | string | `official`, `reviewed` or `community` (see [Tiers](#tiers)). |
| `license` | string | An SPDX expression. |
| `authors` | strings | Optional. |
| `repository` | string | The source repository, `https://`. |
| `homepage` | string | Optional, `https://`. |
| `keywords` | strings | Optional, at most 8, for search. |
| `recommended` | object | Optional, set by a maintainer: `since` (`YYYY-MM-DD`) and `note` (at most 140 characters). See [Recommended](#recommended). |
| `crate` | object | For a `crate` only, and required there: `name` (the crate on crates.io) and optional `features` (the Cargo features that enable the plugin). |
| `versions` | array | Every version ever listed, **newest first** by SemVer precedence. |

Each version:

| Key | Type | Meaning |
|---|---|---|
| `version` | string | SemVer 2.0. For a WASM plugin, the `version` in its manifest; for a micro pack, its `pack.json` version. |
| `published` | string | The day it was listed, `YYYY-MM-DD`. |
| `requires` | object | What it needs from the host; the keys depend on the kind (below). |
| `artifact` | object | `url` (`https://`), `sha256` (64 lowercase hex digits) and `size` (bytes). |
| `source_revision` | string | The full Git commit the artifact was built from. Required for a WASM plugin, optional otherwise. |
| `capabilities` | array | `{"kind", "name"}` objects: what it adds (below). |
| `permissions` | strings | What it may do beyond the sandbox (below). |
| `yanked` | boolean | Optional; `true` withdraws the version (see [Yanking](#yanking)). |
| `yank_reason` | string | Required when `yanked` is `true`, refused otherwise. |

All text is one line with no control characters, so a listing can be
printed in a terminal as it is. Unknown keys are refused by the schema.

### Per kind

| Kind | `requires` | Host constant and check | Capability kinds | Permissions | Artifact |
|---|---|---|---|---|---|
| `wasm-plugin` | `abi`: `"MAJOR.MINOR"` | `rich_plugin_api::abi::ABI_MAJOR`, `ABI_MINOR`; `abi::is_supported(major, minor)`: the same major | `transform`, `highlighter`, `fence-markup`, `fence-ansi` (the ABI's `CapabilityKind`), at least one | only `ansi-output`, exactly when a `fence-ansi` capability is listed | `.wasm`, at most 16 MiB (`WasmLimits::default().module_bytes`) |
| `crate` | `plugin_api`: integer | `rich_plugin_api::PLUGIN_API_VERSION`; `is_supported_api_version(n)`: equal | the registrar's: `highlighter`, `code-highlighter`, `theme`, `box-style`, `renderer`, `fence-renderer`, `transform`, `action`, `component` | must include `native-code`; any of `filesystem-read`, `filesystem-write`, `network`, `process`, `environment`, `clipboard` | `https://static.crates.io/crates/NAME/NAME-VERSION.crate` |
| `micro-pack` | `micro_schema`: integer | `rich_micro::package::SCHEMA_VERSION`: equal | `asset`, named with micro names (`status/success`) | none | a `.zip` or `.richmicro` pack archive (a `pack.json` at its root), at most 8 MiB (micro's `Limits::default().max_archive_bytes`) |
| `theme` | `theme_format`: integer | `1`: upstream `rich`'s `[styles]` INI file, as `--theme-file` reads it | `theme`, at least one | none | `.ini` or `.theme`, at most 1 MiB (the CLI's theme file limit) |

`requires` names the host's own numbers; the format adds none of its own
except `theme_format`, because a theme file has no version of its own.

**Permissions** say what a package can do that the user should know before
installing it. For a WASM plugin they are *enforced*: the sandbox gives a
module no imports at all (no files, network, clock or processes), so the only
permission there is `ansi-output`, which says a `fence-ansi` capability's
output keeps its SGR styling (after the sanitizer). Asset packs are data and
have none. A crate's permissions are *declared and reviewed, not enforced*:
it is native code (`native-code`, always) and runs with the user's rights,
which is why it is never installed from the marketplace.

**The checks beyond the schema** (the validator makes them, and so must the
crate): versions are unique and newest first; a capability is not listed
twice in one version; `ansi-output` goes with `fence-ansi`; a crate's
artifact URL names its crate and version; and `official` is only for
packages whose repository is under `https://github.com/buchochelliq-labs/`.

### Worked examples

The hashes, sizes and revisions in the examples are placeholders of the
right shape.

A WASM plugin, with a yanked version
([`shout.json`](https://github.com/buchochelliq-labs/rs-rich-cli/blob/main/schemas/marketplace/examples/valid/shout.json),
shortened here to two versions):

```json
{
  "schema_version": 1,
  "id": "shout",
  "kind": "wasm-plugin",
  "name": "Shout",
  "description": "Upper-cases text and draws `shout` fences in bold.",
  "tier": "community",
  "license": "MIT OR Apache-2.0",
  "authors": [
    "Example Author"
  ],
  "repository": "https://github.com/example/rich-shout",
  "keywords": [
    "transform",
    "fence"
  ],
  "versions": [
    {
      "version": "0.2.0",
      "published": "2026-10-18",
      "requires": {
        "abi": "1.0"
      },
      "artifact": {
        "url": "https://github.com/example/rich-shout/releases/download/v0.2.0/shout.wasm",
        "sha256": "502a134eeed6db37578e8279a656a1ed669b4c9ca7f1de564f4a5e1054a375a5",
        "size": 2048
      },
      "source_revision": "e26c737c5e9741d94abfea61e9d2065c6b59f2e5",
      "capabilities": [
        {
          "kind": "transform",
          "name": "upper"
        },
        {
          "kind": "fence-markup",
          "name": "shout"
        },
        {
          "kind": "fence-ansi",
          "name": "shout-ansi"
        }
      ],
      "permissions": [
        "ansi-output"
      ]
    },
    {
      "version": "0.1.1",
      "published": "2026-10-12",
      "requires": {
        "abi": "1.0"
      },
      "artifact": {
        "url": "https://github.com/example/rich-shout/releases/download/v0.1.1/shout.wasm",
        "sha256": "67801fdd61417dd8cdca7a653c37aa611a883db8701df39398a83c51bcfd4c27",
        "size": 1536
      },
      "source_revision": "a11cbb361f5ebecf0cb93a9dd1bc633ffd7a2c03",
      "capabilities": [
        {
          "kind": "transform",
          "name": "upper"
        },
        {
          "kind": "fence-markup",
          "name": "shout"
        }
      ],
      "permissions": [],
      "yanked": true,
      "yank_reason": "The `upper` transform drops combining marks; use 0.2.0."
    }
  ]
}
```

A crate, listed only
([`mermaid.json`](https://github.com/buchochelliq-labs/rs-rich-cli/blob/main/schemas/marketplace/examples/valid/mermaid.json)).
Its `sha256` is crates.io's checksum of the `.crate` file, the one Cargo
records in `Cargo.lock`:

```json
{
  "schema_version": 1,
  "id": "mermaid",
  "kind": "crate",
  "name": "Mermaid diagrams",
  "description": "Renders Mermaid flowcharts, sequence and state diagrams in the terminal.",
  "tier": "official",
  "license": "MIT",
  "authors": [
    "rs-rich contributors"
  ],
  "repository": "https://github.com/buchochelliq-labs/rs-rich-cli",
  "homepage": "https://docs.rs/rs-rich-mermaid",
  "crate": {
    "name": "rs-rich-mermaid"
  },
  "versions": [
    {
      "version": "0.0.7",
      "published": "2026-10-20",
      "requires": {
        "plugin_api": 1
      },
      "artifact": {
        "url": "https://static.crates.io/crates/rs-rich-mermaid/rs-rich-mermaid-0.0.7.crate",
        "sha256": "6ca5b3f974d2ead0191b462b81164c750a3e03c6629f9a5783fb819d43a0e9ac",
        "size": 98304
      },
      "capabilities": [
        {
          "kind": "fence-renderer",
          "name": "mermaid"
        },
        {
          "kind": "renderer",
          "name": "mermaid"
        }
      ],
      "permissions": [
        "native-code",
        "process"
      ]
    }
  ]
}
```

A micro pack and a theme differ in `requires`, their capabilities and their
empty permissions:

```json
{"version": "1.0.0", "published": "2026-10-15",
 "requires": {"micro_schema": 1},
 "artifact": {"url": "https://github.com/example/status-icons/releases/download/v1.0.0/status-icons.zip",
              "sha256": "…", "size": 40960},
 "capabilities": [{"kind": "asset", "name": "status/success"},
                  {"kind": "asset", "name": "status/warning"}],
 "permissions": []}
```

```json
{"version": "1.2.0", "published": "2026-09-30",
 "requires": {"theme_format": 1},
 "artifact": {"url": "https://github.com/example/rich-solarized/releases/download/v1.2.0/solarized.ini",
              "sha256": "…", "size": 4096},
 "capabilities": [{"kind": "theme", "name": "solarized"}],
 "permissions": []}
```

The complete files are in
[`schemas/marketplace/examples/valid`](https://github.com/buchochelliq-labs/rs-rich-cli/tree/main/schemas/marketplace/examples/valid);
[`examples/invalid`](https://github.com/buchochelliq-labs/rs-rich-cli/tree/main/schemas/marketplace/examples/invalid)
holds listings that must be refused, each named for its fault: `bad-hash`
(upper-case hex), `short-hash`, `unknown-kind` (`native-plugin`: a native
library cannot be listed), `missing-abi`, `wasm-network-permission`,
`yanked-without-reason`, `crate-without-native-code`, `http-artifact`,
`unsorted-versions`, `ansi-without-permission`, `crate-url-mismatch`,
`official-outside-repository`, `duplicate-version`,
`recommended-long-note` and `recommended-bad-day`.

## The index

`index.json` at the marketplace root
([`index.schema.json`](https://github.com/buchochelliq-labs/rs-rich-cli/blob/main/schemas/marketplace/index.schema.json)):

```json
{
  "schema_version": 1,
  "serial": 42,
  "generated": "2026-10-20T12:00:00Z",
  "expires": "2026-11-19T12:00:00Z",
  "listings": {
    "mermaid": {"path": "packages/mermaid.json", "sha256": "…"},
    "shout": {"path": "packages/shout.json", "sha256": "…"}
  }
}
```

- `serial` grows by one with each signed index. A client refuses an index
  whose serial is below the last one it accepted (a rollback), and one with
  the same serial and different bytes.
- `expires` bounds how long a stale index is trusted (a freeze): the
  maintainer re-signs at least every 30 days, and a client refuses an
  expired index for anything that resolves or installs.
- `listings` maps each id to `packages/<id>.json` and the file's SHA-256.
  Every file under `packages/` is in the index, and every listing's `id` is
  the key it is filed under.

The marketplace's CI writes the index for each merged change; it is
published once the maintainer has signed it.

## Signing

The marketplace signs **the index**, not each listing: the index pins every
listing by hash, and every listing pins every artifact by hash, so one
signature covers everything, and the maintainer signs once per change rather
than once per file. A client that needs one listing verifies the index once
and the listing's hash.

**Algorithm.** [minisign](https://jedisct1.github.io/minisign/): Ed25519
over the file's BLAKE2b-512 hash (the `ED` signature algorithm, minisign's
default; the legacy unhashed `Ed` is refused). The crate verifies with a
pure-Rust verifier (such as `minisign-verify`). The signature is
`index.json.minisig`. Its trusted comment (which the signature also covers)
is `rs-rich-marketplace index serial=N`, and a client checks that `N` is the
index's serial, so a signature cannot be moved to another index.

**Keys.** Two Ed25519 key pairs, made by the maintainer with
`minisign -G` ([0.0.22 plan, decision 1](../plans/0.0.22.md#decisions)):

- **The main key** signs indexes, in CI. Its secret half and its password
  are secrets of a protected environment, `marketplace-signing`, in the
  marketplace repository: it runs only from `main` and only after the
  maintainer approves the run. When a listing pull request merges, the
  workflow rebuilds `index.json`, bumps the serial and signs it:
  `minisign -S -s main.key -m index.json -t "rs-rich-marketplace index serial=42"`.
  It commits the signed index to `main` with a deploy key, a secret of the
  same environment that main's ruleset lets bypass it; the workflow's own
  token only reads.
- **The backup key** signs nothing but key rotations. It never enters
  GitHub: it stays offline, apart from the main key, so a leaked main key
  can be rotated out (below).

Only the public halves enter either repository: they are compiled into
`rs-rich-marketplace` (the trust root cannot come from the repository it
verifies) and recorded in the marketplace repository as `keys/keys-1.json`.
A client trusts the minisign key ids as well as the keys, so a signature
made by any other key is refused before it is checked.

**What is verified, when:**

| When | What |
|---|---|
| Refreshing the index (`rich plugin update`, `search` with a stale cache) | First the key chain (below), then `index.json.minisig` against the current main key, the trusted comment's serial, `schema_version`, the serial against the last accepted one, and `expires`. The verified index is cached with its signature. |
| Reading a listing (`search`, `info`, `install`) | Its SHA-256 against the index, then the schema and the rules beyond it, and that its `id` is the key it is filed under. |
| Installing | The version resolves (below) and is not yanked. The artifact is downloaded over HTTPS only (redirects too), reading at most `size` bytes, into a temporary file beside the destination; its size and SHA-256 must match. Then by kind: a WASM module is loaded in the sandbox (`load_wasm`), and its manifest's name, version, ABI and capabilities must equal the listing's; a micro pack is read with the default limits, and its `pack.json` name and version must equal the id and version; a theme file must parse. Only then is it moved into place and written to the lockfile (id, kind, version, URL, SHA-256, tier and the index serial). |
| Every load (each run) | The installed file's SHA-256 against the lockfile; a file changed on disk is not loaded. No network. |
| `rich plugin verify` | Every installed file against the lockfile, and against the current index: a version yanked since is reported with its reason. |

In the crate, the WASM sandbox is a hook: `rs-rich-marketplace` does not
build the interpreter, so on its own it checks the module's header, and a
host with `wasm-plugins` (the CLI) passes an `ArtifactChecker` that runs
`load_wasm` and compares the manifest with `compare_wasm_manifest`. The
download is held in memory (at most 16 MiB) and written beside the
destination only once every check passed. The lockfile is
`marketplace.lock.json` in the install directory, each file at
`<id>/<id>-<version>.<ext>` beside it.

An expired or unverifiable index never stops installed packages from
loading: those are checked against the lockfile, which was written only
after a verified install.

### Key rotation

The keys form a chain. `keys/keys-N.json` lists the main and backup public
keys and their ids for generation `N`; `keys-1.json` is the pair compiled
into the crate. Each later generation is signed by the keys of the one
before it:

- **A planned rotation** (either key): `keys-N+1.json` is signed by both
  keys of generation `N` (`keys-N+1.json.main.minisig` and
  `keys-N+1.json.backup.minisig`).
- **A lost or compromised main key:** `keys-N+1.json` is signed by the
  backup key of generation `N` alone, and names the old main key in
  `revoked`. Clients accept it, and say that the main key was replaced.
- **The backup key cannot be replaced without the main key**, and a
  generation's backup signature is always required. If the backup key is
  lost or compromised, only a new release of the crate with a new compiled
  pair recovers, announced in its release notes; this is why the backup key
  is used for nothing else.

A generation's file, as the crate reads it (`KeySet::from_json`); an id is
minisign's key id as `minisign` prints it, and `revoked` is optional:

```json
{
  "schema_version": 1,
  "generation": 2,
  "main": {"id": "8C6B1E0F2A9D4C31", "public_key": "RWQxTJ0qDx5rjK…"},
  "backup": {"id": "1D0A7F3E5B2C9E48", "public_key": "RWRInixbPn8KHf…"},
  "revoked": ["057D5BF5EBC4B734"]
}
```

Both signatures of `keys-N.json` carry the trusted comment
`rs-rich-marketplace keys generation=N`, so a signature cannot be moved to
another generation:
`minisign -S -s backup.key -m keys-2.json -x keys-2.json.backup.minisig -t "rs-rich-marketplace keys generation=2"`.
A revoked key stays revoked in every later generation.

A client holds the newest generation it has verified. On refresh it fetches
the generations after it and accepts them one at a time, each against the
one before, before it verifies the index with the newest main key. A
release of the crate compiles in the newest generation, so a new install
does not walk the whole chain.

**The test key.** The crate's tests sign a fixture marketplace with key
pairs checked into its test fixtures
(`crates/rich-marketplace/tests/fixtures/keys`: two generations and a
stranger's key), made by the ignored `generate_test_keys` test with the
`minisign` crate as a dev-dependency (`minisign -G -W` makes the same
files). They are trusted only by a client built with explicit keys in a
test, never by default.

## Installing, and what is only listed

Only a `wasm-plugin`, a `micro-pack` and a `theme` install from the
marketplace. They are sandboxed code or data, so the install prompt can say
what they will do (their capabilities, permissions and tier) and mean it.

Once installed and enabled, each is used with no other step, after its file
is checked again against the lockfile (a changed file is not used, with a
warning): a WASM plugin loads on every run, a micro pack loads into micro's
user layer, and a **theme** is what `--theme ID` selects when the user's
config has no theme of that name. The config's own theme wins a clash, and a
disabled theme is not found.

A **`crate`** is listed only: it is native code that runs with the user's
rights, and it is built into a program rather than added to one. `rich
plugin install mermaid` refuses and prints what to do instead:

```text
mermaid is a Cargo crate (rs-rich-mermaid 0.0.7); add it to your program:
    cargo add rs-rich-mermaid@=0.0.7
```

and `rich plugin info` shows the same, with the crate's features. The
listing's hash lets the marketplace's CI check it against crates.io's
index, and lets a user compare it with the checksum in their `Cargo.lock`.
A **native library** (`.so`, `.dylib`, `.dll`) has no kind at all: it must
still be named by path ([Extending](../PLUGINS.md)).

### Resolving a version

A version is a candidate when it is not yanked, its `requires` is met by the
running host (the checks in the table above, which are the loaders' own), and,
for a WASM plugin, every capability kind is one the host knows (a newer ABI
minor may add kinds; such a version is skipped, not loaded and refused).
Pre-releases are candidates only when asked for by exact version. The newest
candidate wins. A host whose ABI major has moved on shows the newest version
for its own major, and says that newer versions need a newer host.

## Tiers

| Tier | Meaning | Who sets it |
|---|---|---|
| `official` | This repository's own packages: the Mermaid, DOT and lumis crates, the `textcase` and `wordplay` WASM plugins, the built-in micro packs and the official themes, built from this repository (the crates by its release workflow, the others reproducibly by `tests/official_artifacts.rs`, which pins their hashes). The repository must be under `buchochelliq-labs`. | The maintainer. |
| `reviewed` | A maintainer has read the source at `source_revision`, rebuilt the artifact and got the same hash. | The maintainer, per version. |
| `community` | The marketplace's CI checked the listing, the hash and (for a WASM plugin) that it loads in the sandbox against the current host. Nothing more. | The default. |

The tier is shown wherever a package is: in search results, in `info`, and
in the install prompt. A change that sets or raises a tier is refused by the
marketplace's CI unless a maintainer approves it (the `tier-approved`
label).

**Users can always tell ours.** Only `official` marks rs-rich's own
packages, the CLI and the docs site show it as a badge beside the id, and
nothing else may pass for it. Besides the tier rule (and the validator's:
`official` only for a repository under `buchochelliq-labs`), the
marketplace's pull request check (`scripts/check_artifacts.py`) refuses,
in a listing that is not official and is new or renamed:

- **reserved ids:** one starting with `rs-rich`, `rich-`, `richcli` or
  `official` (ignoring `-`, `_` and `.`, except in `rich-`), or `rich` or
  `rs`;
- **lookalike ids:** equal to an official id once `-`, `_` and `.` are
  dropped (`text-case`), or, for an official id of four characters or
  more, one insertion, deletion or substitution from it (`textcase2`,
  `wordp1ay`); and an id that is not ASCII (`textcаse`, with a Cyrillic
  а), which the schema's id pattern refuses too;
- **reserved wording:** "official" or "rs-rich" (so "by rs-rich") in the
  name, or "official" in the description, in any case and with any
  separators.

A maintainer's `name-approved` label lifts the name check for a genuine
case, such as "an unofficial port of …". A listing already merged is
checked again only when its id, name or description changes.

The same check gates a listing's `recommended` mark (★ Recommended):
adding, changing or removing it needs a maintainer's
`recommended-approved` label, separate from `tier-approved`.

## Stats

Download counts and ratings are **unsigned and display-only**. They live
outside the signed index, nothing installs, resolves or trusts by them,
and `rich` sends nothing to count anything: no telemetry and no install
pings. The counts are the ones GitHub and crates.io keep anyway.

The marketplace repository's `stats.yml` writes them nightly to
`stats.json` on its `stats` branch (never `main`), with the job's token:

`https://raw.githubusercontent.com/buchochelliq-labs/rs-rich-marketplace/stats/stats.json`

```json
{
  "schema_version": 1,
  "generated_at": "2026-10-27T03:41:07Z",
  "repository": "buchochelliq-labs/rs-rich-marketplace",
  "packages": {
    "textcase": {
      "downloads": {"total": 17, "versions": {"0.2.0": 17, "0.1.0": null}},
      "reviews": {"url": "https://github.com/buchochelliq-labs/rs-rich-marketplace/discussions/3",
                  "up": 4, "down": 1, "comments": 2}
    }
  }
}
```

- `downloads.versions`, per listed version: the release asset's
  `download_count` for a version the marketplace hosts as a release,
  crates.io's count for a crate, and `null` (not counted) for a version on
  a raw URL or whose release is not made yet. `total` is the sum of the
  counted versions, `null` when none is.
- `reviews`: the package's discussion in the marketplace repository's
  "Reviews" category, titled with its id and opened by `discussions.yml`
  (one written by anyone else is ignored): its URL, the 👍 and 👎
  reactions on its first post, and its comment count; `null` without one.

The schema is the marketplace repository's `schemas/stats.schema.json`.
`rich plugin info ID` shows the downloads, the rating and the reviews link,
and its JSON report gains a `stats` object (`null` without them); `rich
plugin search --sort downloads` sorts by the total. The client reads the
official file from that URL (another `https://` marketplace whose base
ends in `/main/` from the same base with `stats` in its place, else
`stats.json` beside its index; a local one from its directory), with a
3-second timeout, at most 4 MiB. A missing, slow or malformed file, or
another `schema_version`, is left out without a word; counts that are not
integers show as not counted, and a review link that is not on
`https://github.com/` is dropped.

**How the tiers are shown (0.0.23).** People see the tiers as **Official**,
**Approved** and **Community**: `reviewed` is shown as "Approved". The
listing format, the lockfile and the reports keep `reviewed`, since a 0.0.22
client refuses a tier it does not know; the reports add a `tier_label`. The
CLI marks the tier as `✓ official` (green), `approved` and `community`
(yellow), words that read the same without colour, and the
[Marketplace page](../marketplace.md) draws an "Official · by rs-rich"
badge. Both say who publishes a package (rs-rich for an official one, else
its repository's owner) and who authored it ("Authored by rs-rich", or the
listing's own `authors`, quoted when they name rs-rich, and its
repository). The marks come only from the `tier` and `recommended` fields:
a listing's name, description and authors are always shown as plain text.

## Recommended

`recommended` is an optional listing key: a maintainer's pick, given to any
package of any tier.

```json
"recommended": {"since": "2026-10-26", "note": "A small, sandboxed first plugin."}
```

- `since` is the day it was given, `YYYY-MM-DD`; `note` says why, in one
  line of at most 140 characters. Both are required, and no other key is
  allowed (the schema, `scripts/validate_marketplace.py`).
- **Only a maintainer sets it.** The marketplace's pull request check
  refuses a change that adds or changes `recommended` without a
  maintainer's label. It is part of the listing, so the signed index covers
  it, unlike the display-only `stats.json`.
- **Shown** as `★ recommended` by `rich plugin search`, `info` (with the
  note), `browse` and the install prompt, and as a ★ Recommended badge on
  the [Marketplace page](../marketplace.md); `rich plugin search
  --recommended` lists only these. rs-rich-marketplace reads it as
  `Listing::recommended`.
- **Status:** in the format since 0.0.23. A 0.0.22 client ignores the key
  (a client ignores keys it does not know within a schema version). The
  marketplace's CI validates with rs-rich-cli's validator pinned at
  `release-0.0.22` until 0.0.23 is tagged, which refuses the key, so no
  listing uses it before then.

## Yanking

A version is never deleted: `yanked: true` with a `yank_reason` withdraws
it, so lockfiles that name it can still be verified. Resolution never picks a
yanked version, and installing one by exact version is refused. An installed
yanked version keeps loading; `rich plugin list` and `verify` show the
reason, and `update` moves to the newest candidate. A security problem is
said in the reason.

## Changing the format

`schema_version` is `1`. The schema is strict (unknown keys are refused)
so that a mistake in a listing fails the marketplace's CI, while a client
ignores keys it does not know within a schema version it reads. So:

- **An optional key** can be added within version 1: the schema gains it,
  and older clients ignore it.
- **Anything an older client would misread** (a new required key, a changed
  meaning, a new kind) is version 2. A client refuses a version it does not
  know and says to update; the marketplace keeps serving version 1 beside it
  for a transition stated in that release's notes.

## Validating

[`scripts/validate_marketplace.py`](https://github.com/buchochelliq-labs/rs-rich-cli/blob/main/scripts/validate_marketplace.py)
checks listings and indexes with the standard library only: a small
evaluator of the JSON Schema keywords the schemas use (it refuses a schema
that uses any other), then the rules beyond the schema. With `--index` it
checks an index and every listing it names (hash, id, no stray files).
When the `jsonschema` package is installed it validates every document as
well, and the two must agree; this repository's CI installs it and runs
the script over the examples and its tests. Signatures are the crate's to
check (the standard library has no Ed25519). The marketplace repository's
CI runs the same script.

```bash
python3 scripts/validate_marketplace.py schemas/marketplace/examples/valid/*.json
python3 scripts/validate_marketplace.py --expect-invalid schemas/marketplace/examples/invalid/*.json
python3 scripts/validate_marketplace.py --index ../rs-rich-marketplace/index.json
```
